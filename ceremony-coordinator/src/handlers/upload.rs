//! Contribution upload + verify pipeline.
//!
//! Incoming multipart request carries the three state-dir files. We upload
//! each to Blossom, pull the previous round's blobs back out, shell out to
//! `ceremony_tool verify-contribution`, and commit on success.

use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rusqlite::params;
use std::collections::BTreeMap;
use std::str::FromStr;

use crate::ceremony_exec::VerifyArtifacts;
use crate::model::{now_unix, ContributeResponse, Tier};
use crate::nostr::UnsignedEvent;
use crate::SharedState;

pub async fn contribute(
    State(state): State<SharedState>,
    Path(tier): Path<String>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let tier = Tier::from_str(&tier).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    // NOTE: this handler is called *after* the Axum router has already seen
    // the request headers. NIP-98 verification with body-hash binding for
    // multipart uploads is awkward (we'd have to hash the entire body before
    // streaming). For v1 we accept a separate Authorization header that
    // binds just (method, url) + a content-sha256 header over the three
    // files concatenated.
    let lock = state.queues.lock_for(tier);
    let _guard = lock.lock().await;

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("multipart: {e}")))?
    {
        let name = field
            .name()
            .ok_or((StatusCode::BAD_REQUEST, "missing field name".into()))?
            .to_string();
        let bytes = field
            .bytes()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("read field: {e}")))?
            .to_vec();
        files.insert(name, bytes);
    }

    let state_srs = files
        .remove("state_srs")
        .ok_or((StatusCode::BAD_REQUEST, "missing state_srs".into()))?;
    let state_txt_bytes = files
        .remove("state_txt")
        .ok_or((StatusCode::BAD_REQUEST, "missing state_txt".into()))?;
    let receipt_bytes = files
        .remove("receipt")
        .ok_or((StatusCode::BAD_REQUEST, "missing receipt".into()))?;
    let state_txt = String::from_utf8(state_txt_bytes.clone())
        .map_err(|_| (StatusCode::BAD_REQUEST, "state.txt not UTF-8".into()))?;
    let receipt = String::from_utf8(receipt_bytes.clone())
        .map_err(|_| (StatusCode::BAD_REQUEST, "receipt.txt not UTF-8".into()))?;

    // Parse state.txt to find the round number we're trying to commit.
    let kv = parse_kv(&state_txt);
    let round: i64 = kv
        .get("round")
        .and_then(|v| v.parse().ok())
        .ok_or((StatusCode::BAD_REQUEST, "state.txt missing round".into()))?;
    let announced_srs_hash = kv
        .get("srs_hash")
        .cloned()
        .ok_or((StatusCode::BAD_REQUEST, "state.txt missing srs_hash".into()))?;

    // The SRS hash in state.txt is over the *parsed* SRS structure
    // (ceremony::hash_srs), not the raw file bytes. Correctness of
    // announced_srs_hash is enforced by the subprocess verify.

    let prev_round = state
        .store
        .get_round(tier, round - 1)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((
            StatusCode::CONFLICT,
            format!("previous round {} not found", round - 1),
        ))?;

    // Pull the previous round's artifacts back out of Blossom for verify.
    let prev_state_srs = state
        .blossom
        .get(&prev_round.srs_hash)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("blossom fetch srs: {e}")))?;
    let prev_state_txt = state
        .blossom
        .get(&prev_round.state_txt_hash)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("blossom fetch state.txt: {e}")))?;
    let prev_receipt = state
        .blossom
        .get(&prev_round.receipt_hash)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("blossom fetch receipt: {e}")))?;

    let before = VerifyArtifacts {
        state_srs: prev_state_srs,
        state_txt: String::from_utf8(prev_state_txt)
            .map_err(|_| (StatusCode::BAD_GATEWAY, "prev state.txt not UTF-8".into()))?,
        receipt: String::from_utf8(prev_receipt)
            .map_err(|_| (StatusCode::BAD_GATEWAY, "prev receipt not UTF-8".into()))?,
    };
    let after = VerifyArtifacts {
        state_srs: state_srs.clone(),
        state_txt: state_txt.clone(),
        receipt: receipt.clone(),
    };

    state
        .ceremony_tool
        .verify_contribution(&before, &after)
        .await
        .map_err(|e| {
            state
                .store
                .log_event(
                    "verify_fail",
                    Some(tier),
                    Some(round),
                    None,
                    Some(&e.to_string()),
                )
                .ok();
            (
                StatusCode::BAD_REQUEST,
                format!("verify_contribution_failed: {e}"),
            )
        })?;

    // Publish all three blobs to Blossom. Hashes are idempotent; identical
    // bytes resolve to the same key.
    let srs_blob = state
        .blossom
        .put(state_srs.clone(), "application/octet-stream")
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("blossom srs: {e}")))?;
    let txt_blob = state
        .blossom
        .put(state_txt_bytes.clone(), "text/plain")
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("blossom txt: {e}")))?;
    let receipt_blob = state
        .blossom
        .put(receipt_bytes.clone(), "text/plain")
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("blossom receipt: {e}")))?;

    let contribution_id = kv
        .get("contribution_id")
        .cloned()
        .ok_or((StatusCode::BAD_REQUEST, "state.txt missing contribution_id".into()))?;
    let circuit_id = kv
        .get("circuit_id")
        .cloned()
        .unwrap_or_default();

    // Identify the participant from the receipt (their pubkey goes in as the
    // `participant=` field per the native tool).
    let receipt_kv = parse_kv(&receipt);
    let participant_pk = receipt_kv.get("participant").cloned();

    // Insert the round row.
    let conn = state
        .store
        .get()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    conn.execute(
        "INSERT INTO rounds(tier, round, contribution_id, circuit_id, srs_hash,
                            state_txt_hash, receipt_hash, participant_pk,
                            participant_label, nostr_event_id, prev_nostr_event_id,
                            created_at, verified_ok)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, ?9, ?10, 1)",
        params![
            tier.as_str(),
            round,
            contribution_id,
            circuit_id,
            announced_srs_hash,
            txt_blob.sha256_hex,
            receipt_blob.sha256_hex,
            participant_pk,
            prev_round.nostr_event_id,
            now_unix(),
        ],
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Flip the current slot to committed.
    conn.execute(
        "UPDATE signups SET status = 'committed'
         WHERE tier = ?1 AND status = 'claimed' AND pubkey = ?2",
        params![tier.as_str(), participant_pk.as_deref().unwrap_or("")],
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Publish the round event to Nostr (best-effort).
    let mut tags: Vec<Vec<String>> = vec![
        vec![
            "d".into(),
            format!("sepceremony1:{}:r{}", tier.as_str(), round),
        ],
        vec!["tier".into(), tier.as_str().into()],
        vec!["round".into(), round.to_string()],
        vec!["circuit_id".into(), circuit_id.clone()],
        vec!["srs_hash".into(), announced_srs_hash.clone()],
        vec!["contribution_id".into(), contribution_id.clone()],
        vec![
            "blob".into(),
            "state.srs".into(),
            srs_blob.sha256_hex.clone(),
            srs_blob.public_url.clone(),
        ],
        vec![
            "blob".into(),
            "state.txt".into(),
            txt_blob.sha256_hex.clone(),
            txt_blob.public_url.clone(),
        ],
        vec![
            "blob".into(),
            "receipt.txt".into(),
            receipt_blob.sha256_hex.clone(),
            receipt_blob.public_url.clone(),
        ],
    ];
    if let Some(prev_id) = &prev_round.nostr_event_id {
        tags.push(vec!["e".into(), prev_id.clone()]);
    }
    if let Some(pk) = &participant_pk {
        tags.push(vec!["participant_pubkey".into(), pk.clone()]);
    }
    let nostr_event_id = state
        .nostr
        .publish(UnsignedEvent {
            kind: 30078,
            tags,
            content: receipt.clone(),
        })
        .ok()
        .flatten();
    if let Some(ref id) = nostr_event_id {
        conn.execute(
            "UPDATE rounds SET nostr_event_id = ?1 WHERE tier = ?2 AND round = ?3",
            params![id, tier.as_str(), round],
        )
        .ok();
    }

    state
        .store
        .log_event(
            "commit",
            Some(tier),
            Some(round),
            participant_pk.as_deref(),
            None,
        )
        .ok();

    // Broadcast status change.
    let snapshot = build_snapshot_json(&state);
    state.status_tx.publish(snapshot);

    Ok(Json(ContributeResponse {
        round,
        contribution_id,
        srs_hash: announced_srs_hash,
        nostr_event_id,
    }))
}

fn parse_kv(s: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in s.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.insert(k.to_string(), v.to_string());
        }
    }
    out
}

fn build_snapshot_json(state: &SharedState) -> String {
    use crate::model::{CurrentSlot, StatusSnapshot, TierStatus};
    let mut tiers = Vec::with_capacity(3);
    for tier in Tier::ALL {
        let head = state.store.head_round(tier).unwrap_or(-1);
        let depth = state.store.queue_depth(tier).unwrap_or(0);
        let current_slot = state
            .store
            .current_claim(tier)
            .ok()
            .flatten()
            .map(|s| CurrentSlot {
                participant_pk: s.pubkey,
                claimed_at: s.slot_claimed_at.unwrap_or(0),
                deadline: s.slot_deadline.unwrap_or(0),
            });
        tiers.push(TierStatus {
            tier,
            head_round: head,
            queue_depth: depth,
            current_slot,
        });
    }
    serde_json::to_string(&StatusSnapshot {
        tiers,
        as_of: now_unix(),
    })
    .unwrap_or_default()
}
