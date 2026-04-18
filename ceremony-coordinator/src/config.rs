use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: String,
    pub db_path: PathBuf,
    pub blossom_url: String,
    pub blossom_public_url: String,
    pub nostr_relay: String,
    pub ceremony_tool_bin: PathBuf,
    pub work_dir: PathBuf,
    pub admin_pubkeys: Vec<String>,
    pub coordinator_nsec: Option<String>,
    pub slot_deadline_secs: i64,
    pub pow_bits: u32,
    pub allow_browser_contribute: bool,
    pub rate_limit_rpm: u32,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = env::var("CEREMONY_BIND").unwrap_or_else(|_| "0.0.0.0:9090".into());
        let db_path = env::var("CEREMONY_DB_PATH")
            .unwrap_or_else(|_| "/app/data/ceremony.db".into())
            .into();
        let blossom_url =
            env::var("CEREMONY_BLOSSOM_URL").unwrap_or_else(|_| "http://blossom:3000".into());
        let blossom_public_url = env::var("CEREMONY_BLOSSOM_PUBLIC_URL")
            .unwrap_or_else(|_| "https://blossom.onym.chat".into());
        let nostr_relay =
            env::var("CEREMONY_NOSTR_RELAY").unwrap_or_else(|_| "ws://nostr-relay:7777".into());
        let ceremony_tool_bin = env::var("CEREMONY_TOOL_BIN")
            .unwrap_or_else(|_| "/usr/local/bin/ceremony_tool".into())
            .into();
        let work_dir = env::var("CEREMONY_WORK_DIR")
            .unwrap_or_else(|_| "/app/work".into())
            .into();

        let admin_pubkeys = env::var("CEREMONY_ADMIN_PUBKEYS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let coordinator_nsec = env::var("CEREMONY_COORDINATOR_NSEC").ok().filter(|s| !s.is_empty());

        let slot_deadline_secs = env::var("CEREMONY_SLOT_DEADLINE_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(7200);

        let pow_bits = env::var("CEREMONY_POW_BITS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8);

        let allow_browser_contribute = env::var("CEREMONY_ALLOW_BROWSER_CONTRIBUTE")
            .ok()
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let rate_limit_rpm = env::var("CEREMONY_RATE_LIMIT_RPM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);

        Ok(Self {
            bind,
            db_path,
            blossom_url,
            blossom_public_url,
            nostr_relay,
            ceremony_tool_bin,
            work_dir,
            admin_pubkeys,
            coordinator_nsec,
            slot_deadline_secs,
            pow_bits,
            allow_browser_contribute,
            rate_limit_rpm,
        })
    }

    pub fn is_admin(&self, pubkey: &str) -> bool {
        self.admin_pubkeys.iter().any(|p| p.eq_ignore_ascii_case(pubkey))
    }
}
