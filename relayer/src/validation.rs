use base64::Engine;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::config::Config;

/// Allowed contract functions.
const ALLOWED_FUNCTIONS: &[&str] = &[
    "create_group",
    "update_commitment",
    "verify_membership",
    "deactivate_group",
    "get_state",
    "get_history",
];

/// Functions that are read-only (use `--send no`).
pub const READ_ONLY_FUNCTIONS: &[&str] = &["verify_membership", "get_state", "get_history"];

/// Functions that include a proof field in the payload.
const PROOF_FUNCTIONS: &[&str] = &[
    "create_group",
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
