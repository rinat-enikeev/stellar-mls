use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use crate::auth::{body_sha256_hex, reconstruct_url, verify_nip98};
use crate::model::{SignupRequest, SignupResponse};
use crate::SharedState;

const MAX_SIGNUP_BODY: usize = 4 * 1024;

pub async fn signup(
    State(state): State<SharedState>,
    request: Request<Body>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (parts, body) = request.into_parts();
    let url = reconstruct_url(&parts);
    let method = parts.method.as_str().to_string();
    let auth_header = parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or((StatusCode::UNAUTHORIZED, "missing Authorization".into()))?
        .to_string();

    let body_bytes = to_bytes(body, MAX_SIGNUP_BODY)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("read body: {e}")))?;
    let body_hex = body_sha256_hex(&body_bytes);

    let auth = verify_nip98(
        &state,
        &auth_header,
        &method,
        &url,
        body_hex.as_deref(),
    )
    .map_err(|e| (e.status(), e.to_string()))?;

    let req: SignupRequest = serde_json::from_slice(&body_bytes)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("json: {e}")))?;

    state
        .store
        .upsert_participant(&auth.pubkey, req.display_name.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let signup_id = state
        .store
        .insert_signup(&auth.pubkey, req.tier)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let position = state
        .store
        .queue_position(signup_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .store
        .log_event("signup", Some(req.tier), None, Some(&auth.pubkey), None)
        .ok();

    Ok((
        StatusCode::CREATED,
        Json(SignupResponse { position, signup_id }),
    ))
}
