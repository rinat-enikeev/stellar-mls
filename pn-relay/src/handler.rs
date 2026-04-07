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

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::aead::Aead;
    use rand::RngCore;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = PathBuf::from(format!(
            "/tmp/pn_relay_handler_test_{}_{}_{id}",
            std::process::id(),
            prefix
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_test_state(dir: &std::path::Path) -> Arc<AppState> {
        let (x25519_private, x25519_public) = crypto::load_or_create_keypair(dir).unwrap();
        let storage_key = crypto::load_or_create_storage_key(dir).unwrap();
        let db = crate::db::Database::new(dir).unwrap();
        let (watcher_tx, _rx) = tokio::sync::watch::channel(());

        Arc::new(AppState {
            config: Config {
                bind_address: "127.0.0.1:0".to_string(),
                nostr_relay_url: "ws://localhost:7777".to_string(),
                data_dir: dir.to_string_lossy().to_string(),
                apns_key_path: None,
                apns_key_pem: None,
                apns_key_id: None,
                apns_team_id: None,
                apns_bundle_id: "test.bundle".to_string(),
                apns_environment: "development".to_string(),
                fcm_service_account_path: None,
                fcm_project_id: None,
                rate_limit_seconds: 5,
            },
            db,
            x25519_private,
            x25519_public,
            storage_key,
            watcher_tx,
        })
    }

    #[tokio::test]
    async fn test_info_endpoint_structure() {
        let dir = unique_temp_dir("info");
        let state = make_test_state(&dir);

        let Json(response) = handle_info(State(state.clone())).await;

        // Check required fields exist
        assert_eq!(response["version"], 1);
        assert!(response["x25519_public_key"].is_string());
        assert!(response["supported_platforms"].is_array());
        assert_eq!(response["max_filters_per_subscription"], 10);
        assert_eq!(response["max_subscriptions"], 1000);

        // UnifiedPush should always be in supported_platforms
        let platforms = response["supported_platforms"].as_array().unwrap();
        let platform_strs: Vec<&str> = platforms.iter().filter_map(|v| v.as_str()).collect();
        assert!(platform_strs.contains(&"android-up"));

        // APNs and FCM should NOT be listed since they are not configured
        assert!(!platform_strs.contains(&"ios"));
        assert!(!platform_strs.contains(&"android-fcm"));

        // x25519_public_key should be valid base64 and decode to 32 bytes
        let b64 = base64::engine::general_purpose::STANDARD;
        let key_bytes = b64
            .decode(response["x25519_public_key"].as_str().unwrap())
            .unwrap();
        assert_eq!(key_bytes.len(), 32);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let Json(response) = handle_health().await;
        assert_eq!(response["status"], "ok");
    }

    #[tokio::test]
    async fn test_subscription_rejects_wrong_version() {
        let dir = unique_temp_dir("wrong_version");
        let state = make_test_state(&dir);

        let envelope = serde_json::json!({
            "version": 99,
            "scheme": "x25519-aes-256-gcm-v1",
        });

        let response = handle_subscription(State(state), Json(envelope)).await;
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let body_json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(body_json["message"].as_str().unwrap().contains("version"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn test_subscription_rejects_wrong_scheme() {
        let dir = unique_temp_dir("wrong_scheme");
        let state = make_test_state(&dir);

        let envelope = serde_json::json!({
            "version": 1,
            "scheme": "rsa-2048",
        });

        let response = handle_subscription(State(state), Json(envelope)).await;
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::BAD_REQUEST);

        let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let body_json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(body_json["message"].as_str().unwrap().contains("scheme"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn test_subscription_full_roundtrip() {
        let dir = unique_temp_dir("full_roundtrip");
        let state = make_test_state(&dir);
        let b64 = base64::engine::general_purpose::STANDARD;

        // Build a valid encrypted envelope with a subscribe action
        let notification_key = [42u8; 32];
        let inner_payload = serde_json::json!({
            "action": "subscribe",
            "subscription_id": "roundtrip-sub-001",
            "platform": "ios",
            "token": "apns-device-token",
            "notification_key": b64.encode(notification_key),
            "filter": {"#t": ["tag_value_1"]},
        });

        // Construct the encrypted envelope (same logic as crypto test helper)
        let mut rng = rand::thread_rng();
        let mut eph_bytes = [0u8; 32];
        rng.fill_bytes(&mut eph_bytes);
        let ephemeral_secret = x25519_dalek::StaticSecret::from(eph_bytes);
        let ephemeral_public = x25519_dalek::PublicKey::from(&ephemeral_secret);

        let shared_secret = ephemeral_secret.diffie_hellman(&state.x25519_public);
        let hkdf = hkdf::Hkdf::<sha2::Sha256>::new(
            Some(b"sep-invitation-v1"),
            shared_secret.as_bytes(),
        );
        let mut aes_key = [0u8; 32];
        hkdf.expand(b"aes-256-gcm", &mut aes_key).unwrap();

        use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
        let cipher = Aes256Gcm::new_from_slice(&aes_key).unwrap();
        let mut nonce_bytes = [0u8; 12];
        rng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let plaintext = serde_json::to_vec(&inner_payload).unwrap();
        let encrypted = cipher
            .encrypt(nonce, plaintext.as_ref())
            .unwrap();
        let tag_start = encrypted.len() - 16;

        let envelope = serde_json::json!({
            "version": 1,
            "scheme": "x25519-aes-256-gcm-v1",
            "ephemeral_public_key": b64.encode(ephemeral_public.as_bytes()),
            "nonce": b64.encode(&nonce_bytes),
            "ciphertext": b64.encode(&encrypted[..tag_start]),
            "authentication_tag": b64.encode(&encrypted[tag_start..]),
        });

        let response = handle_subscription(State(state.clone()), Json(envelope)).await;
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);

        let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let body_json: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body_json["status"], "ok");

        // Verify the subscription was stored in the database
        let subs = state.db.get_subscriptions_for_tag("tag_value_1").unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].subscription_id, "roundtrip-sub-001");
        assert_eq!(subs[0].platform, "ios");

        std::fs::remove_dir_all(&dir).ok();
    }
}
