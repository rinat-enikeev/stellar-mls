use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect};
use axum::Json;
use futures::stream::Stream;
use serde::Serialize;
use std::convert::Infallible;
use std::str::FromStr;

use crate::model::{
    now_unix, CurrentSlot, Round, SignupStatus, StatusSnapshot, Tier, TierStatus,
};
use crate::SharedState;

pub async fn status(State(state): State<SharedState>) -> Json<StatusSnapshot> {
    Json(build_snapshot(&state))
}

pub async fn status_stream(
    State(state): State<SharedState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.status_tx.subscribe();
    // Emit a synthetic first frame immediately so the client has something
    // to render before the next status change arrives.
    let first = serde_json::to_string(&build_snapshot(&state)).unwrap_or_default();
    let stream = async_stream::stream! {
        yield Ok::<_, Infallible>(Event::default().data(first));
        loop {
            match rx.recv().await {
                Ok(payload) => yield Ok::<_, Infallible>(Event::default().data(payload)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Serialize)]
pub struct QueueEntry {
    pub position: usize,
    pub pubkey: String,
    pub joined_at: i64,
}

pub async fn queue(
    State(state): State<SharedState>,
    Path(tier): Path<String>,
) -> Result<Json<Vec<QueueEntry>>, (StatusCode, String)> {
    let tier = Tier::from_str(&tier).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let rows = state
        .store
        .queue_for_tier(tier)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let out = rows
        .into_iter()
        .enumerate()
        .map(|(i, s)| QueueEntry {
            position: i + 1,
            pubkey: s.pubkey,
            joined_at: s.joined_at,
        })
        .collect();
    Ok(Json(out))
}

pub async fn rounds(
    State(state): State<SharedState>,
    Path(tier): Path<String>,
) -> Result<Json<Vec<Round>>, (StatusCode, String)> {
    let tier = Tier::from_str(&tier).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let rows = state
        .store
        .list_rounds(tier)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(rows))
}

pub async fn round(
    State(state): State<SharedState>,
    Path((tier, round)): Path<(String, i64)>,
) -> Result<Json<Round>, (StatusCode, String)> {
    let tier = Tier::from_str(&tier).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let r = state
        .store
        .get_round(tier, round)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "round not found".into()))?;
    Ok(Json(r))
}

pub async fn round_artifact(
    State(state): State<SharedState>,
    Path((tier, round, name)): Path<(String, i64, String)>,
) -> Result<Redirect, (StatusCode, String)> {
    let tier = Tier::from_str(&tier).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let r = state
        .store
        .get_round(tier, round)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "round not found".into()))?;
    let hash = match name.as_str() {
        "state.srs" => r.srs_hash,
        "state.txt" => r.state_txt_hash,
        "receipt.txt" => r.receipt_hash,
        _ => return Err((StatusCode::NOT_FOUND, "unknown artifact".into())),
    };
    Ok(Redirect::to(&state.blossom.public_blob_url(&hash)))
}

pub async fn phase2_round_artifact(
    State(state): State<SharedState>,
    Path((tier, round, name)): Path<(String, i64, String)>,
) -> Result<Redirect, (StatusCode, String)> {
    let tier = Tier::from_str(&tier).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let rows = state
        .store
        .phase2_rounds()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let r = rows
        .into_iter()
        .find(|r| r.tier == tier && r.round == round)
        .ok_or((StatusCode::NOT_FOUND, "phase2 round not found".into()))?;
    match name.as_str() {
        "zkey" => {
            let h = r.zkey_blob_sha256.ok_or((
                StatusCode::NOT_FOUND,
                "zkey blob not stored for this round".into(),
            ))?;
            Ok(Redirect::to(&state.blossom.public_blob_url(&h)))
        }
        _ => Err((StatusCode::NOT_FOUND, "unknown artifact".into())),
    }
}

#[derive(Serialize)]
pub struct ParticipantStatus {
    pub pubkey: String,
    pub status: Option<String>,
    pub tier: Option<String>,
    pub queue_position: Option<i64>,
    pub slot_claimed_at: Option<i64>,
    pub slot_deadline: Option<i64>,
    pub committed_round: Option<i64>,
    pub prev_blobs: Option<PrevBlobs>,
}

#[derive(Serialize)]
pub struct PrevBlobs {
    pub round: i64,
    pub srs: String,
    pub srs_hash: String,
    pub txt: String,
    pub txt_hash: String,
    pub receipt: String,
    pub receipt_hash: String,
}

pub async fn participant(
    State(state): State<SharedState>,
    Path(pubkey): Path<String>,
) -> Result<Json<ParticipantStatus>, (StatusCode, String)> {
    let signup = state
        .store
        .participant_signup(&pubkey)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut out = ParticipantStatus {
        pubkey: pubkey.clone(),
        status: None,
        tier: None,
        queue_position: None,
        slot_claimed_at: None,
        slot_deadline: None,
        committed_round: None,
        prev_blobs: None,
    };
    if let Some(s) = signup {
        out.status = Some(match s.status {
            SignupStatus::Queued => "queued".into(),
            SignupStatus::Claimed => "claimed".into(),
            SignupStatus::Committed => "committed".into(),
            SignupStatus::Expired => "expired".into(),
            SignupStatus::Skipped => "skipped".into(),
            SignupStatus::Withdrawn => "withdrawn".into(),
        });
        out.tier = Some(s.tier.as_str().to_string());
        out.slot_claimed_at = s.slot_claimed_at;
        out.slot_deadline = s.slot_deadline;

        if matches!(s.status, SignupStatus::Queued) {
            out.queue_position = state
                .store
                .participant_queue_position(&pubkey, s.tier)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }

        if matches!(s.status, SignupStatus::Claimed) {
            // Surface previous-round artifact URLs so the frontend can link
            // them directly without a second round-trip.
            let head = state
                .store
                .head_round(s.tier)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            if head >= 0 {
                if let Some(r) = state
                    .store
                    .get_round(s.tier, head)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                {
                    out.prev_blobs = Some(PrevBlobs {
                        round: r.round,
                        srs: state.blossom.public_blob_url(&r.srs_hash),
                        srs_hash: r.srs_hash,
                        txt: state.blossom.public_blob_url(&r.state_txt_hash),
                        txt_hash: r.state_txt_hash,
                        receipt: state.blossom.public_blob_url(&r.receipt_hash),
                        receipt_hash: r.receipt_hash,
                    });
                }
            }
        }

        if matches!(s.status, SignupStatus::Committed) {
            out.committed_round = state
                .store
                .participant_committed_round(&pubkey, s.tier)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
    }
    Ok(Json(out))
}

pub async fn downloads(State(_state): State<SharedState>) -> impl IntoResponse {
    // Populated by the release workflow; for now return an empty list so the
    // download page renders cleanly in staging before the first release.
    let body = serde_json::json!({
        "tag": null,
        "assets": [],
        "minisign_pubkey": null,
        "cosign_identity": null,
    });
    Json(body)
}

fn build_snapshot(state: &SharedState) -> StatusSnapshot {
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
    StatusSnapshot {
        tiers,
        as_of: now_unix(),
    }
}

// Keep status_stream honest: suppress the warning about the unused enum arm.
#[allow(dead_code)]
fn _unused_signup_status(_: SignupStatus) {}
