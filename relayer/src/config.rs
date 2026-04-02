use std::collections::HashSet;
use std::env;

/// Relayer configuration, loaded from environment variables.
pub struct Config {
    /// Stellar secret key (S...) for signing transactions.
    pub secret_key: String,
    /// Stellar public key (G...) derived from the secret key at startup.
    pub public_address: String,
    /// Whitelisted contract ID (C...).
    pub contract_id: String,
    /// Soroban RPC endpoint URL.
    pub rpc_url: String,
    /// Network passphrase used when invoking via explicit RPC URL.
    pub network_passphrase: String,
    /// Stellar network name (mainnet, testnet).
    pub network: String,
    /// HTTP bind address.
    pub bind_address: String,
    /// Valid bearer tokens. Empty = no auth required.
    pub auth_tokens: HashSet<String>,
    /// Rate limit: requests per minute per IP.
    pub rate_limit_per_minute: u32,
    /// Maximum request body size in bytes.
    pub max_payload_size: usize,
    /// Stellar CLI identity name (created from secret_key at startup).
    pub identity_name: String,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let secret_key = require_env("RELAYER_SECRET_KEY")?;
        let contract_id = require_env("RELAYER_CONTRACT_ID")?;
        let rpc_url = env::var("RELAYER_RPC_URL")
            .unwrap_or_else(|_| "https://soroban.stellar.org".to_string());
        let network_passphrase = env::var("RELAYER_NETWORK_PASSPHRASE").unwrap_or_else(|_| {
            match env::var("RELAYER_NETWORK").unwrap_or_else(|_| "mainnet".to_string()).as_str() {
                "testnet" => "Test SDF Network ; September 2015".to_string(),
                "futurenet" => "Test SDF Future Network ; October 2022".to_string(),
                "mainnet" => "Public Global Stellar Network ; September 2015".to_string(),
                _ => String::new(),
            }
        });
        let network = env::var("RELAYER_NETWORK").unwrap_or_else(|_| "mainnet".to_string());
        let bind_address =
            env::var("RELAYER_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
        let auth_tokens: HashSet<String> = env::var("RELAYER_AUTH_TOKENS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let rate_limit_per_minute: u32 = env::var("RELAYER_RATE_LIMIT")
            .unwrap_or_else(|_| "30".to_string())
            .parse()
            .map_err(|_| "RELAYER_RATE_LIMIT must be a number")?;
        let max_payload_size: usize = env::var("RELAYER_MAX_PAYLOAD_SIZE")
            .unwrap_or_else(|_| "8192".to_string())
            .parse()
            .map_err(|_| "RELAYER_MAX_PAYLOAD_SIZE must be a number")?;

        Ok(Config {
            secret_key,
            public_address: String::new(), // resolved at startup
            contract_id,
            rpc_url,
            network_passphrase,
            network,
            bind_address,
            auth_tokens,
            rate_limit_per_minute,
            max_payload_size,
            identity_name: "sep-relayer".to_string(),
        })
    }

    pub fn auth_required(&self) -> bool {
        !self.auth_tokens.is_empty()
    }
}

fn require_env(key: &str) -> Result<String, String> {
    env::var(key).map_err(|_| format!("{key} environment variable is required"))
}
