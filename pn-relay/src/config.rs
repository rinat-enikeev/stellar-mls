use std::env;

/// Push notification relay configuration, loaded from environment variables.
#[derive(Clone)]
pub struct Config {
    /// HTTP bind address.
    pub bind_address: String,
    /// WebSocket URL of the Nostr relay to watch.
    pub nostr_relay_url: String,
    /// Directory for SQLite DB and key files.
    pub data_dir: String,
    /// Path to .p8 APNs auth key file (optional).
    pub apns_key_path: Option<String>,
    /// Raw PEM content of APNs .p8 key (alternative to apns_key_path).
    pub apns_key_pem: Option<String>,
    /// APNs key ID (optional).
    pub apns_key_id: Option<String>,
    /// Apple team ID (optional).
    pub apns_team_id: Option<String>,
    /// APNs bundle ID.
    pub apns_bundle_id: String,
    /// APNs environment: "production" or "development".
    pub apns_environment: String,
    /// Path to Google service account JSON (optional).
    pub fcm_service_account_path: Option<String>,
    /// Firebase project ID (optional).
    pub fcm_project_id: Option<String>,
    /// Minimum seconds between pushes per subscription.
    pub rate_limit_seconds: i64,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let bind_address =
            env::var("PN_BIND").unwrap_or_else(|_| "0.0.0.0:8090".to_string());
        let nostr_relay_url =
            env::var("PN_NOSTR_RELAY").unwrap_or_else(|_| "ws://nostr-relay:7777".to_string());
        let data_dir =
            env::var("PN_DATA_DIR").unwrap_or_else(|_| "/app/data".to_string());

        let apns_key_path = env::var("APNS_KEY_PATH").ok().filter(|s| !s.is_empty());
        let apns_key_pem = env::var("APNS_KEY_PEM").ok().filter(|s| !s.is_empty());
        let apns_key_id = env::var("APNS_KEY_ID").ok().filter(|s| !s.is_empty());
        let apns_team_id = env::var("APNS_TEAM_ID").ok().filter(|s| !s.is_empty());
        let apns_bundle_id =
            env::var("APNS_BUNDLE_ID").unwrap_or_else(|_| "chat.onym.ios".to_string());
        let apns_environment =
            env::var("APNS_ENVIRONMENT").unwrap_or_else(|_| "production".to_string());

        let fcm_service_account_path = env::var("FCM_SERVICE_ACCOUNT_PATH").ok().filter(|s| !s.is_empty());
        let fcm_project_id = env::var("FCM_PROJECT_ID").ok().filter(|s| !s.is_empty());

        let rate_limit_seconds: i64 = env::var("PN_RATE_LIMIT_SECONDS")
            .unwrap_or_else(|_| "5".to_string())
            .parse()
            .map_err(|_| "PN_RATE_LIMIT_SECONDS must be a number".to_string())?;

        Ok(Config {
            bind_address,
            nostr_relay_url,
            data_dir,
            apns_key_path,
            apns_key_pem,
            apns_key_id,
            apns_team_id,
            apns_bundle_id,
            apns_environment,
            fcm_service_account_path,
            fcm_project_id,
            rate_limit_seconds,
        })
    }

    pub fn apns_configured(&self) -> bool {
        (self.apns_key_path.is_some() || self.apns_key_pem.is_some())
            && self.apns_key_id.is_some()
            && self.apns_team_id.is_some()
    }

    pub fn fcm_configured(&self) -> bool {
        self.fcm_service_account_path.is_some() && self.fcm_project_id.is_some()
    }
}
