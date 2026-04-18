//! Slot-claim handler.
//!
//! Only the head of the queue for a tier can claim. The coordinator mints a
//! deadline, flips the row to `claimed`, and returns blob URLs for the
//! current round so the participant can download and contribute locally.

use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rusqlite::params;
use std::str::FromStr;

use crate::auth::{reconstruct_url, verify_nip98};
use crate::model::{now_unix, BlobRef, ClaimDownload, ClaimResponse, Tier};
use crate::SharedState;

pub async fn claim(
    State(state): State<SharedState>,
    Path(tier): Path<String>,
    request: Request,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let tier = Tier::from_str(&tier).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let (parts, _body) = request.into_parts();
    let url = reconstruct_url(&parts);
    let auth_header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or((StatusCode::UNAUTHORIZED, "missing Authorization".into()))?
        .to_string();

    let auth = verify_nip98(&state, &auth_header, "POST", &url, None)
        .map_err(|e| (e.status(), e.to_string()))?;

    let lock = state.queues.lock_for(tier);
    let _guard = lock.lock().await;

    // Is this pubkey the head of the queue?
    let queue = state
        .store
        .queue_for_tier(tier)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let head = queue
        .first()
        .ok_or((StatusCode::CONFLICT, "queue empty".into()))?;
    if head.pubkey != auth.pubkey {
        return Err((
            StatusCode::CONFLICT,
            format!("not at head of queue (position depends on FIFO)"),
        ));
    }

    // If someone still holds an unexpired slot, refuse.
    let current = state
        .store
        .current_claim(tier)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(cur) = current {
        if cur.slot_deadline.unwrap_or(0) > now_unix() {
            return Err((StatusCode::CONFLICT, "slot held by another participant".into()));
        }
    }

    // Flip queued → claimed with a deadline.
    let now = now_unix();
    let deadline = now + state.config.slot_deadline_secs;
    let conn = state
        .store
        .get()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    conn.execute(
        "UPDATE signups
         SET status = 'claimed', slot_claimed_at = ?1, slot_deadline = ?2
         WHERE id = ?3 AND status = 'queued'",
        params![now, deadline, head.id],
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Fetch current head round for blob references.
    let head_round_idx = state
        .store
        .head_round(tier)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if head_round_idx < 0 {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "tier not initialised yet".into(),
        ));
    }
    let current_round = state
        .store
        .get_round(tier, head_round_idx)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "head round missing".into()))?;

    let next_round = current_round.round + 1;

    state
        .store
        .log_event(
            "slot_claim",
            Some(tier),
            Some(next_round),
            Some(&auth.pubkey),
            None,
        )
        .ok();

    Ok(Json(ClaimResponse {
        round: next_round,
        deadline,
        expected_before_srs_hash: current_round.srs_hash.clone(),
        download: ClaimDownload {
            state_srs: BlobRef {
                sha256: current_round.srs_hash.clone(),
                url: state.blossom.public_blob_url(&current_round.srs_hash),
            },
            state_txt: BlobRef {
                sha256: current_round.state_txt_hash.clone(),
                url: state.blossom.public_blob_url(&current_round.state_txt_hash),
            },
            receipt: BlobRef {
                sha256: current_round.receipt_hash.clone(),
                url: state.blossom.public_blob_url(&current_round.receipt_hash),
            },
        },
    }))
}
