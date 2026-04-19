//! Withdraw handler — lets a participant cancel their own signup.
//!
//! `POST /api/v1/tiers/:tier/withdraw`
//!
//! Authed via NIP-98. If the caller has an active `queued` or `claimed`
//! signup for this tier, flip it to `withdrawn`. `claimed` rows free the
//! slot immediately; `queued` rows leave the line. Anything else (no
//! signup, already committed, already expired) is a no-op returning 404.

use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rusqlite::params;
use serde::Serialize;
use std::str::FromStr;

use crate::auth::{reconstruct_url, verify_nip98};
use crate::model::Tier;
use crate::SharedState;

#[derive(Serialize)]
pub struct WithdrawResponse {
    pub withdrawn_signup_id: i64,
    pub previous_status: String,
}

pub async fn withdraw(
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

    let conn = state
        .store
        .get()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Find an active row for this participant in this tier.
    let row: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, status FROM signups
              WHERE pubkey = ?1 AND tier = ?2 AND status IN ('queued','claimed')
              ORDER BY joined_at DESC LIMIT 1",
            params![auth.pubkey, tier.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();

    let Some((id, previous_status)) = row else {
        return Err((StatusCode::NOT_FOUND, "no active signup to withdraw".into()));
    };

    conn.execute(
        "UPDATE signups SET status = 'withdrawn' WHERE id = ?1",
        params![id],
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .store
        .log_event(
            "signup_withdrawn",
            Some(tier),
            None,
            Some(&auth.pubkey),
            Some(&format!("{{\"from\":\"{previous_status}\"}}")),
        )
        .ok();

    Ok(Json(WithdrawResponse {
        withdrawn_signup_id: id,
        previous_status,
    }))
}
