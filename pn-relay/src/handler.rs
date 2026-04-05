use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use base64::Engine;
use serde_json::{json, Value};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::config::Config;
use crate::crypto;
use crate::db::{Database, Subscription};

/// Shared application state.
pub struct AppState {
    pub config: Config,
    pub db: Database,
    pub x25519_private: StaticSecret,
    pub x25519_public: PublicKey,
    pub storage_key: [u8; 32],
    pub watcher_tx: tokio::sync::watch::Sender<()>,
}

/// GET /v1/info - Returns server information and public key.
pub async fn handle_info(State(state): State<Arc<AppState>>) -> Json<Value> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let public_key_b64 = b64.encode(state.x25519_public.as_bytes());

    let mut platforms = Vec::new();
    if state.config.apns_configured() {
        platforms.push("ios");
    }
    if state.config.fcm_configured() {
        platforms.push("android-fcm");
    }
    // UnifiedPush is always available (no server-side config needed)
    platforms.push("android-up");

    Json(json!({
        "version": 1,
        "x25519_public_key": public_key_b64,
        "supported_platforms": platforms,
        "max_filters_per_subscription": 10,
        "max_subscriptions": 1000
    }))
}

/// POST /v1/subscription - Handle encrypted subscription management.
pub async fn handle_subscription(
    State(state): State<Arc<AppState>>,
    Json(envelope): Json<Value>,
) -> Response {
    // Validate envelope structure
    let version = envelope.get("version").and_then(|v| v.as_i64());
    let scheme = envelope.get("scheme").and_then(|v| v.as_str());

    if version != Some(1) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "unsupported version"})),
        )
            .into_response();
    }

    if scheme != Some("x25519-aes-256-gcm-v1") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "unsupported encryption scheme"})),
        )
            .into_response();
    }

    // Decrypt the envelope
    let decrypted = match crypto::decrypt_registration_envelope(&envelope, &state.x25519_private) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to decrypt registration envelope: {e}");
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": "decryption failed"})),
            )
                .into_response();
        }
    };

    // Determine action
    let action = match decrypted.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": "missing action field"})),
            )
                .into_response();
        }
    };

    match action.as_str() {
        "subscribe" => handle_subscribe(&state, &decrypted).await,
        "update" => handle_update(&state, &decrypted).await,
        "unsubscribe" => handle_unsubscribe(&state, &decrypted).await,
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "unknown action"})),
        )
            .into_response(),
    }
}

async fn handle_subscribe(state: &Arc<AppState>, decrypted: &Value) -> Response {
    let b64 = base64::engine::general_purpose::STANDARD;

    // Extract required fields
    let subscription_id = match decrypted.get("subscription_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": "missing subscription_id"})),
            )
                .into_response();
        }
    };

    let platform = match decrypted.get("platform").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": "missing platform"})),
            )
                .into_response();
        }
    };

    if !["ios", "android-fcm", "android-up"].contains(&platform.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"status": "error", "message": "unsupported platform"})),
        )
            .into_response();
    }

    let token = match decrypted.get("token").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": "missing token"})),
            )
                .into_response();
        }
    };

    // SECURITY: For UnifiedPush subscriptions, validate the endpoint URL at registration
    // time to prevent SSRF. The URL is also validated at dispatch time as defense-in-depth.
    if platform == "android-up" {
        if let Err(e) = crate::unified_push::validate_endpoint_url(&token) {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": format!("invalid UnifiedPush endpoint: {e}")})),
            )
                .into_response();
        }
    }

    let notification_key_b64 = match decrypted.get("notification_key").and_then(|v| v.as_str()) {
        Some(k) => k,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": "missing notification_key"})),
            )
                .into_response();
        }
    };

    let notification_key = match b64.decode(notification_key_b64) {
        Ok(k) if k.len() == 32 => k,
        Ok(k) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": format!("notification_key must be 32 bytes, got {}", k.len())})),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": format!("invalid base64 notification_key: {e}")})),
            )
                .into_response();
        }
    };

    let filter = match decrypted.get("filter") {
        Some(f) => f,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": "missing filter"})),
            )
                .into_response();
        }
    };

    let filter_json = filter.to_string();

    // Encrypt token and notification key for storage
    let encrypted_token = match crypto::encrypt_for_storage(token.as_bytes(), &state.storage_key) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to encrypt token for storage: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"status": "error", "message": "internal error"})),
            )
                .into_response();
        }
    };

    let encrypted_notif_key =
        match crypto::encrypt_for_storage(&notification_key, &state.storage_key) {
            Ok(k) => k,
            Err(e) => {
                eprintln!("Failed to encrypt notification key for storage: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"status": "error", "message": "internal error"})),
                )
                    .into_response();
            }
        };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let sub = Subscription {
        subscription_id,
        filter_json,
        encrypted_token,
        encrypted_notif_key,
        platform,
        created_at: now,
        last_pushed_at: 0,
    };

    if let Err(e) = state.db.insert_subscription(&sub) {
        eprintln!("Failed to insert subscription: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": "failed to save subscription"})),
        )
            .into_response();
    }

    // Signal the watcher to refresh
    let _ = state.watcher_tx.send(());

    // SECURITY: Do not log full subscription_id — it is a bearer token.
    // Log only a truncated hint for debugging.
    let id_hint = if sub.subscription_id.len() >= 8 {
        &sub.subscription_id[..8]
    } else {
        &sub.subscription_id
    };
    eprintln!("New subscription: id_hint={id_hint}..., platform={}", sub.platform);
    (StatusCode::OK, Json(json!({"status": "ok"}))).into_response()
}

async fn handle_update(state: &Arc<AppState>, decrypted: &Value) -> Response {
    let subscription_id = match decrypted.get("subscription_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": "missing subscription_id"})),
            )
                .into_response();
        }
    };

    let filter = match decrypted.get("filter") {
        Some(f) => f,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": "missing filter"})),
            )
                .into_response();
        }
    };

    let filter_json = filter.to_string();

    if let Err(e) = state
        .db
        .update_subscription_filter(subscription_id, &filter_json)
    {
        eprintln!("Failed to update subscription: {e}");
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"status": "error", "message": e})),
        )
            .into_response();
    }

    // Signal the watcher to refresh
    let _ = state.watcher_tx.send(());

    let id_hint = if subscription_id.len() >= 8 {
        &subscription_id[..8]
    } else {
        subscription_id
    };
    eprintln!("Updated subscription: id_hint={id_hint}...");
    (StatusCode::OK, Json(json!({"status": "ok"}))).into_response()
}

async fn handle_unsubscribe(state: &Arc<AppState>, decrypted: &Value) -> Response {
    let subscription_id = match decrypted.get("subscription_id").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"status": "error", "message": "missing subscription_id"})),
            )
                .into_response();
        }
    };

    if let Err(e) = state.db.delete_subscription(subscription_id) {
        eprintln!("Failed to delete subscription: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"status": "error", "message": "failed to delete subscription"})),
        )
            .into_response();
    }

    // Signal the watcher to refresh
    let _ = state.watcher_tx.send(());

    let id_hint = if subscription_id.len() >= 8 {
        &subscription_id[..8]
    } else {
        subscription_id
    };
    eprintln!("Deleted subscription: id_hint={id_hint}...");
    (StatusCode::OK, Json(json!({"status": "ok"}))).into_response()
}

/// GET /v1/health - Simple health check.
pub async fn handle_health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}
