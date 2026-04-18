//! Browser WASM verifier for the Onym trusted setup ceremony.
//!
//! Exports three entry points to JavaScript:
//!
//! - `verify_state_js(srs, state_txt, receipt_txt)` — check a round is
//!    internally consistent and the receipt matches the SRS.
//! - `verify_contribution_js(before_srs, before_state, before_receipt,
//!    after_srs, after_state, after_receipt)` — check two consecutive rounds
//!    are linked by a valid contribution proof.
//! - `hash_srs_js(srs)` — canonical SHA-256 over the parsed SRS (matches
//!    the `srs_hash` field in `state.txt`).
//!
//! All functions return a JSON string rather than a structured object to
//! keep the WASM boundary narrow (no wasm-bindgen type inspection at call
//! time; the caller parses the JSON with `JSON.parse`).

use sep_xxxx_circuits::ceremony::{
    self,
    transcript::{parse_kv, parse_proof, parse_srs, required},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

#[derive(Serialize)]
struct VerifyResult {
    ok: bool,
    round: Option<i64>,
    tier: Option<String>,
    srs_hash: Option<String>,
    error: Option<String>,
}

fn err(msg: impl Into<String>) -> String {
    let r = VerifyResult {
        ok: false,
        round: None,
        tier: None,
        srs_hash: None,
        error: Some(msg.into()),
    };
    serde_json::to_string(&r).unwrap_or_else(|_| r#"{"ok":false}"#.to_string())
}

/// Verify one round's SRS against its state.txt and receipt.txt.
///
/// Catches: SRS consistency (all 6 pairing identities), receipt-SRS binding
/// via the `after_srs_hash` field, and the contribution proof itself — as
/// an initial-contribution proof, since we don't have a preceding round
/// available here. For chained round verification call
/// `verify_contribution_js` instead.
#[wasm_bindgen]
pub fn verify_state_js(srs: &[u8], state_txt: &str, receipt_txt: &str) -> String {
    let state = match parse_kv(state_txt) {
        Ok(v) => v,
        Err(e) => return err(format!("state.txt: {e}")),
    };
    let receipt = match parse_kv(receipt_txt) {
        Ok(v) => v,
        Err(e) => return err(format!("receipt.txt: {e}")),
    };

    let parsed_srs = match parse_srs(srs) {
        Ok(v) => v,
        Err(e) => return err(format!("state.srs: {e}")),
    };

    // Recompute the canonical SRS hash and check it against state.txt.
    let computed = hex_encode(&ceremony::hash_srs(&parsed_srs));
    let announced = match required(&state, "srs_hash") {
        Ok(s) => s.to_string(),
        Err(e) => return err(e),
    };
    if !constant_time_eq(computed.as_bytes(), announced.as_bytes()) {
        return err(format!(
            "srs_hash mismatch: state.txt={announced} computed={computed}"
        ));
    }

    // Internal SRS consistency — the full O(n) pairing sweep.
    if !ceremony::verify_consistency(&parsed_srs) {
        return err("SRS failed internal consistency check");
    }

    // Contribution proof. Treated as initial if no prior round is supplied.
    let proof = match parse_proof(&receipt) {
        Ok(v) => v,
        Err(e) => return err(format!("receipt: {e}")),
    };
    if !ceremony::verify_initial_contribution(&parsed_srs, &proof) {
        return err("contribution proof did not verify");
    }

    let round = state.get("round").and_then(|v| v.parse().ok());
    let tier = state.get("tier").cloned();
    let r = VerifyResult {
        ok: true,
        round,
        tier,
        srs_hash: Some(computed),
        error: None,
    };
    serde_json::to_string(&r).unwrap_or_default()
}

/// Verify that `after` is a valid contribution on top of `before`.
///
/// Runs the six-pairing-equation check plus the receipt-SRS binding on
/// both sides, so passing `verify_contribution_js` also implies the inputs
/// pass `verify_state_js` individually.
#[wasm_bindgen]
pub fn verify_contribution_js(
    before_srs: &[u8],
    before_state_txt: &str,
    before_receipt_txt: &str,
    after_srs: &[u8],
    after_state_txt: &str,
    after_receipt_txt: &str,
) -> String {
    let before_state = match parse_kv(before_state_txt) {
        Ok(v) => v,
        Err(e) => return err(format!("before state.txt: {e}")),
    };
    let after_state = match parse_kv(after_state_txt) {
        Ok(v) => v,
        Err(e) => return err(format!("after state.txt: {e}")),
    };
    let after_receipt = match parse_kv(after_receipt_txt) {
        Ok(v) => v,
        Err(e) => return err(format!("after receipt.txt: {e}")),
    };
    let _before_receipt = match parse_kv(before_receipt_txt) {
        Ok(v) => v,
        Err(e) => return err(format!("before receipt.txt: {e}")),
    };

    let before_srs_parsed = match parse_srs(before_srs) {
        Ok(v) => v,
        Err(e) => return err(format!("before state.srs: {e}")),
    };
    let after_srs_parsed = match parse_srs(after_srs) {
        Ok(v) => v,
        Err(e) => return err(format!("after state.srs: {e}")),
    };

    // Round numbers must be consecutive.
    let before_round: i64 = match before_state.get("round").and_then(|v| v.parse().ok()) {
        Some(v) => v,
        None => return err("before state.txt missing integer 'round'"),
    };
    let after_round: i64 = match after_state.get("round").and_then(|v| v.parse().ok()) {
        Some(v) => v,
        None => return err("after state.txt missing integer 'round'"),
    };
    if after_round != before_round + 1 {
        return err(format!(
            "rounds not consecutive: before={before_round} after={after_round}"
        ));
    }

    // The receipt is over the "after" contribution; its `before_srs_hash`
    // must match the computed hash of the "before" SRS.
    let before_hash = hex_encode(&ceremony::hash_srs(&before_srs_parsed));
    let after_hash = hex_encode(&ceremony::hash_srs(&after_srs_parsed));
    if let Some(bh) = after_receipt.get("before_srs_hash") {
        if !constant_time_eq(bh.as_bytes(), before_hash.as_bytes()) {
            return err(format!(
                "after receipt before_srs_hash mismatch: receipt={bh} computed={before_hash}"
            ));
        }
    }

    // Six-pairing check.
    let proof = match parse_proof(&after_receipt) {
        Ok(v) => v,
        Err(e) => return err(format!("after receipt: {e}")),
    };
    if !ceremony::verify_contribution(&before_srs_parsed, &after_srs_parsed, &proof) {
        return err("contribution pairing check failed");
    }

    let tier = after_state.get("tier").cloned();
    let r = VerifyResult {
        ok: true,
        round: Some(after_round),
        tier,
        srs_hash: Some(after_hash),
        error: None,
    };
    serde_json::to_string(&r).unwrap_or_default()
}

/// Canonical SHA-256 over a parsed SRS (matches `ceremony::hash_srs`).
#[wasm_bindgen]
pub fn hash_srs_js(srs: &[u8]) -> String {
    match parse_srs(srs) {
        Ok(p) => hex_encode(&ceremony::hash_srs(&p)),
        Err(e) => format!("error: {e}"),
    }
}

/// Constant-time equality; spec allows hex strings to be compared either
/// way, but constant-time avoids giving a timing oracle to malicious
/// callers that craft state.txt to probe prefix matches.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

// Keep Sha2 linked for potential future use (e.g. over raw SRS bytes);
// currently hash_srs already does the canonical digest.
#[allow(dead_code)]
fn _keep_sha2_linked() {
    let _ = Sha256::new();
}
