//! Phase 2 endpoints.
//!
//! - `GET  /phase2/summary`          — freeze state + per-tier Phase 1 head
//! - `GET  /phase2/rounds`           — all indexed Phase 2 rounds
//! - `POST /phase2/freeze`           — admin freezes Phase 1 (writes beacon height)
//! - `POST /phase2/rounds`           — admin records a Phase 2 round (zkey hash + attestation)
//! - `POST /phase2/rounds/upload`    — admin uploads a zkey blob to Blossom
//!                                      and records the round in one call
//!
//! Phase 2 runs via snarkjs (see `docker/phase2-helper/`) and the
//! coordinator only *indexes* the output; we never run the MPC here.

use axum::body::Bytes;
use axum::extract::{FromRequest, Request, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::{body_sha256_hex, reconstruct_url, verify_nip98};
use crate::model::{now_unix, Tier};
use crate::store::CeremonyState;
use crate::SharedState;
use rusqlite::params;

#[derive(Debug, Serialize)]
pub struct Phase2Summary {
    pub state: CeremonyState,
    pub tiers: Vec<Phase2TierSummary>,
}

#[derive(Debug, Serialize)]
pub struct Phase2TierSummary {
    pub tier: Tier,
    pub phase1_head_round: i64,
    pub phase1_srs_hash: Option<String>,
    pub phase1_contribution_id: Option<String>,
    pub phase2_head_round: Option<i64>,
}

pub async fn summary(State(state): State<SharedState>) -> impl IntoResponse {
    let cstate = state.store.ceremony_state().unwrap_or_default();
    let p2_rounds = state.store.phase2_rounds().unwrap_or_default();
    let mut tiers = Vec::with_capacity(3);
    for tier in Tier::ALL {
        let head = state.store.head_round(tier).unwrap_or(-1);
        let r = if head >= 0 {
            state.store.get_round(tier, head).ok().flatten()
        } else {
            None
        };
        let p2_head = p2_rounds
            .iter()
            .filter(|x| x.tier == tier)
            .map(|x| x.round)
            .max();
        tiers.push(Phase2TierSummary {
            tier,
            phase1_head_round: head,
            phase1_srs_hash: r.as_ref().map(|x| x.srs_hash.clone()),
            phase1_contribution_id: r.as_ref().map(|x| x.contribution_id.clone()),
            phase2_head_round: p2_head,
        });
    }
    Json(Phase2Summary {
        state: cstate,
        tiers,
    })
}

pub async fn rounds(State(state): State<SharedState>) -> Result<impl IntoResponse, (StatusCode, String)> {
    let rows = state
        .store
        .phase2_rounds()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(rows))
}

/// Verify that a request is from an admin pubkey. Returns the admin pk on
/// success. Used by every write endpoint in this module.
async fn require_admin(
    state: &SharedState,
    parts: &axum::http::request::Parts,
    body_bytes: &[u8],
) -> Result<String, (StatusCode, String)> {
    let url = reconstruct_url(parts);
    let auth_header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or((StatusCode::UNAUTHORIZED, "missing Authorization".into()))?
        .to_string();
    let body_hex = body_sha256_hex(body_bytes);
    let method = parts.method.as_str();
    let auth = verify_nip98(state, &auth_header, method, &url, body_hex.as_deref())
        .map_err(|e| (e.status(), e.to_string()))?;
    if !state.config.is_admin(&auth.pubkey) {
        return Err((StatusCode::FORBIDDEN, "not an admin pubkey".into()));
    }
    Ok(auth.pubkey)
}

#[derive(Debug, Deserialize)]
pub struct FreezeRequest {
    pub beacon_block_height: Option<i64>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FreezeResponse {
    pub ok: bool,
    pub state: CeremonyState,
}

pub async fn freeze(
    State(state): State<SharedState>,
    request: Request,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (parts, body) = request.into_parts();
    let body_bytes = axum::body::to_bytes(body, 16 * 1024)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("body: {e}")))?;
    let admin_pk = require_admin(&state, &parts, &body_bytes).await?;

    let req: FreezeRequest = if body_bytes.is_empty() {
        FreezeRequest { beacon_block_height: None, note: None }
    } else {
        serde_json::from_slice(&body_bytes)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("json: {e}")))?
    };

    state
        .store
        .freeze_phase1(&admin_pk, req.note.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(h) = req.beacon_block_height {
        state
            .store
            .set_beacon(h, None)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    state.store.log_event("phase1_freeze", None, None, Some(&admin_pk), None).ok();

    Ok(Json(FreezeResponse {
        ok: true,
        state: state.store.ceremony_state().unwrap_or_default(),
    }))
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BeaconRequest {
    pub block_height: i64,
    pub block_hash: Option<String>,
}

pub async fn set_beacon(
    State(state): State<SharedState>,
    request: Request,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (parts, body) = request.into_parts();
    let body_bytes = axum::body::to_bytes(body, 16 * 1024)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("body: {e}")))?;
    let admin_pk = require_admin(&state, &parts, &body_bytes).await?;
    let req: BeaconRequest = serde_json::from_slice(&body_bytes)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("json: {e}")))?;
    state
        .store
        .set_beacon(req.block_height, req.block_hash.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .store
        .log_event(
            "phase2_beacon_set",
            None,
            None,
            Some(&admin_pk),
            Some(&serde_json::to_string(&req).unwrap_or_default()),
        )
        .ok();
    Ok(Json(state.store.ceremony_state().unwrap_or_default()))
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Phase2RoundRequest {
    pub tier: Tier,
    pub round: i64,
    pub participant_pk: Option<String>,
    pub zkey_hash: String,
    pub attestation: Option<String>,
    pub zkey_blob_sha256: Option<String>,
}

pub async fn publish_round(
    State(state): State<SharedState>,
    request: Request,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (parts, body) = request.into_parts();
    let body_bytes = axum::body::to_bytes(body, 16 * 1024)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("body: {e}")))?;
    let admin_pk = require_admin(&state, &parts, &body_bytes).await?;
    let req: Phase2RoundRequest = serde_json::from_slice(&body_bytes)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("json: {e}")))?;

    insert_phase2_round(&state, &req)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .store
        .log_event(
            "phase2_publish",
            Some(req.tier),
            Some(req.round),
            req.participant_pk.as_deref().or(Some(&admin_pk)),
            None,
        )
        .ok();

    Ok((StatusCode::CREATED, Json(serde_json::json!({"ok": true}))))
}

fn insert_phase2_round(state: &SharedState, req: &Phase2RoundRequest) -> anyhow::Result<()> {
    let conn = state.store.get()?;
    conn.execute(
        "INSERT INTO phase2_rounds(tier, round, participant_pk, zkey_hash,
                                    attestation_hash, zkey_blob_sha256,
                                    nostr_event_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7)
         ON CONFLICT(tier, round) DO UPDATE SET
           participant_pk = excluded.participant_pk,
           zkey_hash = excluded.zkey_hash,
           attestation_hash = excluded.attestation_hash,
           zkey_blob_sha256 = COALESCE(excluded.zkey_blob_sha256, phase2_rounds.zkey_blob_sha256)",
        params![
            req.tier.as_str(),
            req.round,
            req.participant_pk,
            req.zkey_hash,
            req.attestation,
            req.zkey_blob_sha256,
            now_unix(),
        ],
    )?;
    Ok(())
}

/// Upload a zkey blob and record the corresponding Phase 2 round. The
/// client sends a multipart body with two fields:
///
///   - `meta` (JSON, matches `Phase2RoundRequest` minus `zkey_blob_sha256`)
///   - `zkey` (binary blob, variable size — up to client_max_body_size)
///
/// The coordinator pushes the blob to Blossom (so it's addressable at
/// `/api/v1/tiers/:tier/rounds/:round/artifacts/zkey` after), computes its
/// SHA-256, and writes the round row.
pub async fn upload_round(
    State(state): State<SharedState>,
    request: Request,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (parts, body) = request.into_parts();

    // NIP-98 for multipart follows the convention established by the
    // contribute upload: method+url bound, body hash omitted because we
    // can't hash it without buffering the whole body upfront.
    let url = reconstruct_url(&parts);
    let auth_header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or((StatusCode::UNAUTHORIZED, "missing Authorization".into()))?
        .to_string();
    let auth = verify_nip98(&state, &auth_header, "POST", &url, None)
        .map_err(|e| (e.status(), e.to_string()))?;
    if !state.config.is_admin(&auth.pubkey) {
        return Err((StatusCode::FORBIDDEN, "not an admin pubkey".into()));
    }

    let mut multipart =
        axum::extract::Multipart::from_request(Request::from_parts(parts, body), &())
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("multipart: {e}")))?;

    let mut meta_json: Option<Bytes> = None;
    let mut zkey_bytes: Option<Bytes> = None;
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
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("read field: {e}")))?;
        match name.as_str() {
            "meta" => meta_json = Some(bytes),
            "zkey" => zkey_bytes = Some(bytes),
            _ => {}
        }
    }

    let meta_json = meta_json.ok_or((StatusCode::BAD_REQUEST, "missing meta".into()))?;
    let zkey_bytes = zkey_bytes.ok_or((StatusCode::BAD_REQUEST, "missing zkey".into()))?;

    let mut meta: Phase2RoundRequest = serde_json::from_slice(&meta_json)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("meta: {e}")))?;

    // Compute the zkey hash, push to Blossom, confirm consistency with
    // whatever the admin claimed in `zkey_hash`.
    let sha_hex = hex::encode(Sha256::digest(&zkey_bytes));
    if meta.zkey_hash != sha_hex {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "zkey_hash mismatch: meta={} computed={}",
                meta.zkey_hash, sha_hex
            ),
        ));
    }

    let put = state
        .blossom
        .put(zkey_bytes.to_vec(), "application/octet-stream")
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("blossom: {e}")))?;
    meta.zkey_blob_sha256 = Some(put.sha256_hex);

    insert_phase2_round(&state, &meta)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .store
        .log_event(
            "phase2_upload",
            Some(meta.tier),
            Some(meta.round),
            Some(&auth.pubkey),
            None,
        )
        .ok();

    Ok((StatusCode::CREATED, Json(meta)))
}
