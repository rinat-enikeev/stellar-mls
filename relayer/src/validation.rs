use base64::Engine;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::config::Config;

/// Allowed contract functions.
const ALLOWED_FUNCTIONS: &[&str] = &[
    "create_group",
    "create_group_v2",
    "create_oligarchy_group",
    "update_commitment",
    "verify_membership",
    "deactivate_group",
    "get_state",
    "get_state_v2",
    "get_admin_root",
    "get_history",
];

/// Functions that are read-only (use `--send no`).
pub const READ_ONLY_FUNCTIONS: &[&str] = &[
    "verify_membership",
    "get_state",
    "get_state_v2",
    "get_admin_root",
    "get_history",
];

/// Functions that include a proof field in the payload.
const PROOF_FUNCTIONS: &[&str] = &[
    "create_group",
    "create_group_v2",
    "create_oligarchy_group",
    "update_commitment",
    "verify_membership",
    "deactivate_group",
];

/// Expected decoded proof size (96 + 192 + 96 = 384 bytes).
const EXPECTED_PROOF_SIZE: usize = 384;

/// Validate a request against the relayer's security rules.
pub fn validate_request(
    config: &Config,
    contract_id: &str,
    function: &str,
    payload: &serde_json::Value,
) -> Result<(), String> {
    // 1. Contract ID whitelist
    if contract_id != config.contract_id {
        return Err(format!(
            "contract ID mismatch: expected {}, got {contract_id}",
            config.contract_id
        ));
    }

    // 2. Function whitelist
    if !ALLOWED_FUNCTIONS.contains(&function) {
        return Err(format!("function not allowed: {function}"));
    }

    // 3. Proof size validation (for functions that include a proof)
    if PROOF_FUNCTIONS.contains(&function) {
        if let Some(proof_b64) = payload.get("proof").and_then(|v| v.as_str()) {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(proof_b64)
                .map_err(|e| format!("invalid proof base64: {e}"))?;
            if decoded.len() != EXPECTED_PROOF_SIZE {
                return Err(format!(
                    "proof must be {EXPECTED_PROOF_SIZE} bytes, got {}",
                    decoded.len()
                ));
            }
        }
        // proof may not be present for get_state/get_history — that's fine
    }

    Ok(())
}

/// Validate bearer token if auth is required.
pub fn validate_auth(config: &Config, auth_header: Option<&str>) -> Result<(), String> {
    if !config.auth_required() {
        return Ok(());
    }
    let header = auth_header.ok_or("Authorization header required")?;
    let token = header
        .strip_prefix("Bearer ")
        .ok_or("Authorization must be: Bearer <token>")?;
    if config.auth_tokens.contains(token) {
        Ok(())
    } else {
        Err("invalid bearer token".to_string())
    }
}

/// Simple in-memory rate limiter by IP address.
pub struct RateLimiter {
    /// IP → (window_start, count)
    buckets: Mutex<HashMap<String, (Instant, u32)>>,
    max_per_minute: u32,
}

impl RateLimiter {
    pub fn new(max_per_minute: u32) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            max_per_minute,
        }
    }

    /// Returns Ok if the request is allowed, Err if rate-limited.
    pub fn check(&self, ip: &str) -> Result<(), String> {
        let mut buckets = self.buckets.lock().unwrap();
        let now = Instant::now();
        let entry = buckets.entry(ip.to_string()).or_insert((now, 0));

        // Reset window if more than 60 seconds have passed
        if now.duration_since(entry.0).as_secs() >= 60 {
            *entry = (now, 0);
        }

        entry.1 += 1;
        if entry.1 > self.max_per_minute {
            Err(format!(
                "rate limited: {} requests/minute exceeded",
                self.max_per_minute
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make_config(contract_id: &str, auth_tokens: HashSet<String>) -> Config {
        Config {
            secret_key: String::new(),
            public_address: String::new(),
            contract_id: contract_id.to_string(),
            rpc_url: String::new(),
            network_passphrase: String::new(),
            network: String::new(),
            bind_address: String::new(),
            auth_tokens,
            rate_limit_per_minute: 30,
            max_payload_size: 8192,
            identity_name: String::new(),
        }
    }

    fn config_no_auth(contract_id: &str) -> Config {
        make_config(contract_id, HashSet::new())
    }

    fn config_with_auth(contract_id: &str, token: &str) -> Config {
        let mut tokens = HashSet::new();
        tokens.insert(token.to_string());
        make_config(contract_id, tokens)
    }

    /// Helper: produce a base64-encoded proof of exactly `n` bytes.
    fn make_proof_base64(n: usize) -> String {
        let bytes = vec![0xABu8; n];
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    }

    // ---- Auth tests ----

    #[test]
    fn test_auth_not_required_when_no_tokens() {
        let config = config_no_auth("C123");
        assert!(validate_auth(&config, None).is_ok());
    }

    #[test]
    fn test_auth_required_when_tokens_configured() {
        let config = config_with_auth("C123", "validtoken");
        assert!(validate_auth(&config, None).is_err());
    }

    #[test]
    fn test_valid_bearer_token_accepted() {
        let config = config_with_auth("C123", "validtoken");
        assert!(validate_auth(&config, Some("Bearer validtoken")).is_ok());
    }

    #[test]
    fn test_invalid_bearer_token_rejected() {
        let config = config_with_auth("C123", "validtoken");
        let result = validate_auth(&config, Some("Bearer wrongtoken"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid bearer token"));
    }

    #[test]
    fn test_missing_auth_header_rejected() {
        let config = config_with_auth("C123", "validtoken");
        let result = validate_auth(&config, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Authorization header required"));
    }

    #[test]
    fn test_wrong_prefix_rejected() {
        let config = config_with_auth("C123", "validtoken");
        let result = validate_auth(&config, Some("Basic xyz"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Bearer"));
    }

    // ---- Request validation tests ----

    #[test]
    fn test_correct_contract_id_accepted() {
        let config = config_no_auth("CABC123");
        let payload = serde_json::json!({});
        assert!(validate_request(&config, "CABC123", "get_state", &payload).is_ok());
    }

    #[test]
    fn test_wrong_contract_id_rejected() {
        let config = config_no_auth("CABC123");
        let payload = serde_json::json!({});
        let result = validate_request(&config, "CWRONG", "get_state", &payload);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("contract ID mismatch"));
    }

    #[test]
    fn test_allowed_functions_accepted() {
        let config = config_no_auth("C1");
        let payload = serde_json::json!({});
        for func in ALLOWED_FUNCTIONS {
            assert!(
                validate_request(&config, "C1", func, &payload).is_ok(),
                "expected function '{}' to be allowed",
                func
            );
        }
    }

    #[test]
    fn test_disallowed_function_rejected() {
        let config = config_no_auth("C1");
        let payload = serde_json::json!({});
        let result = validate_request(&config, "C1", "initialize", &payload);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("function not allowed"));
    }

    #[test]
    fn test_proof_size_384_bytes_accepted() {
        let config = config_no_auth("C1");
        let proof = make_proof_base64(384);
        let payload = serde_json::json!({ "proof": proof });
        assert!(validate_request(&config, "C1", "create_group", &payload).is_ok());
    }

    #[test]
    fn test_proof_size_wrong_rejected() {
        let config = config_no_auth("C1");
        let proof = make_proof_base64(100);
        let payload = serde_json::json!({ "proof": proof });
        let result = validate_request(&config, "C1", "create_group", &payload);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("384 bytes"));
    }

    #[test]
    fn test_proof_not_required_for_get_state() {
        let config = config_no_auth("C1");
        let payload = serde_json::json!({});
        // get_state is not in PROOF_FUNCTIONS, so no proof field needed
        assert!(validate_request(&config, "C1", "get_state", &payload).is_ok());
    }

    #[test]
    fn test_invalid_base64_proof_rejected() {
        let config = config_no_auth("C1");
        let payload = serde_json::json!({ "proof": "not-base64!!!" });
        let result = validate_request(&config, "C1", "create_group", &payload);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid proof base64"));
    }

    // ---- Rate limiter tests ----

    #[test]
    fn test_rate_limit_allows_within_window() {
        let limiter = RateLimiter::new(5);
        for i in 0..5 {
            assert!(
                limiter.check("192.168.1.1").is_ok(),
                "request {} should be allowed",
                i + 1
            );
        }
    }

    #[test]
    fn test_rate_limit_rejects_over_limit() {
        let limiter = RateLimiter::new(3);
        for _ in 0..3 {
            assert!(limiter.check("10.0.0.1").is_ok());
        }
        let result = limiter.check("10.0.0.1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("rate limited"));
    }

    #[test]
    fn test_rate_limit_independent_per_ip() {
        let limiter = RateLimiter::new(2);
        // Use up IP A's quota
        assert!(limiter.check("ip_a").is_ok());
        assert!(limiter.check("ip_a").is_ok());
        assert!(limiter.check("ip_a").is_err());

        // IP B should still have its full quota
        assert!(limiter.check("ip_b").is_ok());
        assert!(limiter.check("ip_b").is_ok());
        assert!(limiter.check("ip_b").is_err());
    }
}
