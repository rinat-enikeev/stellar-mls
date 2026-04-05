use std::collections::HashMap;
use std::time::Instant;

use base64::Engine;
use serde_json::{json, Value};

use crate::config::Config;

/// Result of an FCM push dispatch attempt.
pub enum PushResult {
    /// Successfully delivered.
    Success,
    /// Token is no longer registered; subscription should be deleted.
    Unregistered,
    /// An error occurred.
    Error(String),
}

/// FCM HTTP v1 API client.
pub struct FcmClient {
    client: reqwest::Client,
    project_id: String,
    access_token: String,
    token_expires: Instant,
    service_account: Value,
}

impl FcmClient {
    /// Create a new FCM client. Returns None if FCM is not configured.
    pub fn new(config: &Config) -> Result<Option<Self>, String> {
        if !config.fcm_configured() {
            return Ok(None);
        }

        let sa_path = config.fcm_service_account_path.as_ref().unwrap();
        let project_id = config.fcm_project_id.as_ref().unwrap().clone();

        let sa_data = std::fs::read_to_string(sa_path)
            .map_err(|e| format!("failed to read FCM service account file {sa_path}: {e}"))?;

        let service_account: Value = serde_json::from_str(&sa_data)
            .map_err(|e| format!("failed to parse FCM service account JSON: {e}"))?;

        let client = reqwest::Client::new();

        eprintln!("FCM client initialized (project_id={project_id})");

        Ok(Some(FcmClient {
            client,
            project_id,
            access_token: String::new(),
            token_expires: Instant::now(),
            service_account,
        }))
    }

    /// Refresh the OAuth2 access token using the service account's private key.
    async fn refresh_access_token(&mut self) -> Result<(), String> {
        if !self.access_token.is_empty() && Instant::now() < self.token_expires {
            return Ok(());
        }

        let private_key_pem = self
            .service_account
            .get("private_key")
            .and_then(|v| v.as_str())
            .ok_or("missing private_key in service account")?;

        let client_email = self
            .service_account
            .get("client_email")
            .and_then(|v| v.as_str())
            .ok_or("missing client_email in service account")?;

        let token_uri = self
            .service_account
            .get("token_uri")
            .and_then(|v| v.as_str())
            .unwrap_or("https://oauth2.googleapis.com/token");

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let claims = json!({
            "iss": client_email,
            "scope": "https://www.googleapis.com/auth/firebase.messaging",
            "aud": token_uri,
            "iat": now,
            "exp": now + 3600,
        });

        let encoding_key = jsonwebtoken::EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
            .map_err(|e| format!("failed to parse FCM private key: {e}"))?;

        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);

        let jwt = jsonwebtoken::encode(&header, &claims, &encoding_key)
            .map_err(|e| format!("failed to generate FCM JWT: {e}"))?;

        // Exchange JWT for access token
        let response = self
            .client
            .post(token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &jwt),
            ])
            .send()
            .await
            .map_err(|e| format!("FCM token exchange failed: {e}"))?;

        if !response.status().is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown".to_string());
            return Err(format!("FCM token exchange error: {body}"));
        }

        let token_response: Value = response
            .json()
            .await
            .map_err(|e| format!("FCM token response parse error: {e}"))?;

        self.access_token = token_response
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or("missing access_token in FCM token response")?
            .to_string();

        let expires_in = token_response
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(3600);

        // Refresh 5 minutes early
        self.token_expires =
            Instant::now() + std::time::Duration::from_secs(expires_in.saturating_sub(300));

        Ok(())
    }

    /// Send a push notification via FCM HTTP v1 API.
    pub async fn send_push(
        &mut self,
        fcm_token: &str,
        data: &HashMap<String, String>,
    ) -> Result<PushResult, String> {
        self.refresh_access_token().await?;

        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.project_id
        );

        let body = json!({
            "message": {
                "token": fcm_token,
                "android": {
                    "priority": "high"
                },
                "data": data
            }
        });

        let response = self
            .client
            .post(&url)
            .header("authorization", format!("Bearer {}", self.access_token))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("FCM request failed: {e}"))?;

        let status = response.status().as_u16();

        match status {
            200 => Ok(PushResult::Success),
            404 => {
                eprintln!("FCM: token unregistered (404) for {fcm_token}");
                Ok(PushResult::Unregistered)
            }
            _ => {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown".to_string());
                // Check for UNREGISTERED error in response body
                if body.contains("UNREGISTERED") || body.contains("NOT_FOUND") {
                    eprintln!("FCM: token unregistered for {fcm_token}");
                    return Ok(PushResult::Unregistered);
                }
                eprintln!("FCM error {status}: {body}");
                Ok(PushResult::Error(format!("FCM status {status}: {body}")))
            }
        }
    }
}

/// Build the FCM data map for a push notification.
pub fn build_fcm_data(
    encrypted: &[u8],
    nonce: &[u8],
    tag: &[u8],
    event_id: &str,
    subscription_id: &str,
) -> HashMap<String, String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let enc_b64 = b64.encode(encrypted);
    let sub_id_short = if subscription_id.len() >= 8 {
        &subscription_id[..8]
    } else {
        subscription_id
    };

    let mut data = HashMap::new();

    // Check if encrypted content exceeds 3KB
    if enc_b64.len() > 3072 {
        data.insert("event_id".to_string(), event_id.to_string());
        data.insert("sub_id".to_string(), sub_id_short.to_string());
        data.insert("fetch".to_string(), "true".to_string());
    } else {
        data.insert("enc".to_string(), enc_b64);
        data.insert("nonce".to_string(), b64.encode(nonce));
        data.insert("tag".to_string(), b64.encode(tag));
        data.insert("event_id".to_string(), event_id.to_string());
        data.insert("sub_id".to_string(), sub_id_short.to_string());
    }

    data
}
