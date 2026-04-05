use std::time::Instant;

use base64::Engine;
use serde_json::{json, Value};

use crate::config::Config;

/// Result of a push notification dispatch attempt.
pub enum PushResult {
    /// Successfully delivered.
    Success,
    /// Token is no longer valid; subscription should be deleted.
    Gone,
    /// An error occurred.
    Error(String),
}

/// APNs HTTP/2 push notification client.
pub struct ApnsClient {
    client: reqwest::Client,
    token: String,
    token_expires: Instant,
    key_id: String,
    team_id: String,
    bundle_id: String,
    environment: String,
    signing_key: jsonwebtoken::EncodingKey,
}

impl ApnsClient {
    /// Create a new APNs client. Returns None if APNs is not configured.
    pub fn new(config: &Config) -> Result<Option<Self>, String> {
        if !config.apns_configured() {
            return Ok(None);
        }

        let key_id = config.apns_key_id.as_ref().unwrap().clone();
        let team_id = config.apns_team_id.as_ref().unwrap().clone();

        let key_data = if let Some(pem) = &config.apns_key_pem {
            pem.replace("\\n", "\n").into_bytes()
        } else {
            let key_path = config.apns_key_path.as_ref().unwrap();
            std::fs::read(key_path)
                .map_err(|e| format!("failed to read APNs key file {key_path}: {e}"))?
        };

        let signing_key = jsonwebtoken::EncodingKey::from_ec_pem(&key_data)
            .map_err(|e| format!("failed to parse APNs .p8 key: {e}"))?;

        // Build an HTTP/2 capable client (ALPN negotiation via TLS handles h2)
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;

        eprintln!(
            "APNs client initialized (key_id={}, team_id={}, env={})",
            key_id, team_id, config.apns_environment
        );

        Ok(Some(ApnsClient {
            client,
            token: String::new(),
            token_expires: Instant::now(),
            key_id,
            team_id,
            bundle_id: config.apns_bundle_id.clone(),
            environment: config.apns_environment.clone(),
            signing_key,
        }))
    }

    /// Generate or return a cached JWT for APNs authentication.
    fn generate_jwt(&mut self) -> Result<&str, String> {
        // Cache for 50 minutes (Apple allows 60 min)
        if !self.token.is_empty() && Instant::now() < self.token_expires {
            return Ok(&self.token);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let header = jsonwebtoken::Header {
            alg: jsonwebtoken::Algorithm::ES256,
            kid: Some(self.key_id.clone()),
            ..Default::default()
        };

        let claims = serde_json::json!({
            "iss": self.team_id,
            "iat": now,
        });

        self.token = jsonwebtoken::encode(&header, &claims, &self.signing_key)
            .map_err(|e| format!("failed to generate APNs JWT: {e}"))?;
        self.token_expires = Instant::now() + std::time::Duration::from_secs(50 * 60);

        Ok(&self.token)
    }

    /// Send a push notification to an iOS device.
    pub async fn send_push(
        &mut self,
        device_token: &str,
        payload: &Value,
    ) -> Result<PushResult, String> {
        let jwt = self.generate_jwt()?.to_string();

        let host = if self.environment == "development" {
            "https://api.development.push.apple.com"
        } else {
            "https://api.push.apple.com"
        };

        let url = format!("{host}/3/device/{device_token}");

        let response = self
            .client
            .post(&url)
            .header("authorization", format!("bearer {jwt}"))
            .header("apns-topic", &self.bundle_id)
            .header("apns-push-type", "alert")
            .header("apns-priority", "10")
            .json(payload)
            .send()
            .await
            .map_err(|e| format!("APNs request failed: {e}"))?;

        let status = response.status().as_u16();

        match status {
            200 => Ok(PushResult::Success),
            410 => {
                eprintln!("APNs: device token gone (410) for {device_token}");
                Ok(PushResult::Gone)
            }
            _ => {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown".to_string());
                eprintln!("APNs error {status}: {body}");
                Ok(PushResult::Error(format!("APNs status {status}: {body}")))
            }
        }
    }
}

/// Build the APNs JSON payload for a push notification.
///
/// For normal messages where encrypted content fits within limits:
/// ```json
/// {
///   "aps": { "mutable-content": 1, "alert": {"title":"StellarChat","body":"New message"}, "sound": "default" },
///   "enc": "<base64>", "nonce": "<base64>", "tag": "<base64>",
///   "event_id": "<hex>", "sub_id": "<first 8 chars>"
/// }
/// ```
///
/// For large payloads (enc > 3KB):
/// ```json
/// {
///   "aps": { "mutable-content": 1, "content-available": 1, "alert": {"title":"StellarChat","body":"New message"} },
///   "event_id": "<hex>", "sub_id": "<first 8 chars>", "fetch": true
/// }
/// ```
pub fn build_apns_payload(
    encrypted: &[u8],
    nonce: &[u8],
    tag: &[u8],
    event_id: &str,
    subscription_id: &str,
) -> Value {
    let b64 = base64::engine::general_purpose::STANDARD;
    let enc_b64 = b64.encode(encrypted);
    let sub_id_short = if subscription_id.len() >= 8 {
        &subscription_id[..8]
    } else {
        subscription_id
    };

    // Check if encrypted content exceeds 3KB
    if enc_b64.len() > 3072 {
        json!({
            "aps": {
                "mutable-content": 1,
                "content-available": 1,
                "alert": {
                    "title": "StellarChat",
                    "body": "New message"
                }
            },
            "event_id": event_id,
            "sub_id": sub_id_short,
            "fetch": true
        })
    } else {
        json!({
            "aps": {
                "mutable-content": 1,
                "alert": {
                    "title": "StellarChat",
                    "body": "New message"
                },
                "sound": "default"
            },
            "enc": enc_b64,
            "nonce": b64.encode(nonce),
            "tag": b64.encode(tag),
            "event_id": event_id,
            "sub_id": sub_id_short
        })
    }
}
