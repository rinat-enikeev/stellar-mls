use std::process::Command;
use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Json;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;
use crate::validation::{self, RateLimiter, READ_ONLY_FUNCTIONS};

/// Shared application state.
pub struct AppState {
    pub config: Config,
    pub rate_limiter: RateLimiter,
}

/// Incoming request from mobile apps.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayerRequest {
    #[serde(alias = "contractID")]
    pub contract_id: String,
    pub function: String,
    pub payload: Value,
}

/// Response returned to mobile apps.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayerResponse {
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// POST / — main relayer endpoint.
pub async fn handle_invoke(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<RelayerRequest>,
) -> Response {
    let ip = addr.ip().to_string();

    // Auth check
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok());
    if let Err(e) = validation::validate_auth(&state.config, auth_header) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(RelayerResponse {
                accepted: false,
                transaction_hash: None,
                message: Some(e),
            }),
        )
            .into_response();
    }

    // Rate limit check
    if let Err(e) = state.rate_limiter.check(&ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(RelayerResponse {
                accepted: false,
                transaction_hash: None,
                message: Some(e),
            }),
        )
            .into_response();
    }

    // Validate request
    if let Err(e) =
        validation::validate_request(&state.config, &request.contract_id, &request.function, &request.payload)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(RelayerResponse {
                accepted: false,
                transaction_hash: None,
                message: Some(e),
            }),
        )
            .into_response();
    }

    // Build and execute the stellar CLI invocation
    match invoke_contract(&state.config, &request).await {
        Ok(output) => match success_response(&request.function, output) {
            Ok(response) => response,
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                Json(RelayerResponse {
                    accepted: false,
                    transaction_hash: None,
                    message: Some(e),
                }),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(RelayerResponse {
                accepted: false,
                transaction_hash: None,
                message: Some(e),
            }),
        )
            .into_response(),
    }
}

fn success_response(function: &str, output: String) -> Result<Response, String> {
    match function {
        "create_group" | "update_commitment" | "deactivate_group" => Ok((
            StatusCode::OK,
            Json(RelayerResponse {
                accepted: true,
                transaction_hash: None,
                message: if output.is_empty() { None } else { Some(output) },
            }),
        )
            .into_response()),
        "verify_membership" => {
            let valid = parse_bool_output(&output)?;
            Ok((StatusCode::OK, Json(json!({ "valid": valid }))).into_response())
        }
        "get_state" | "get_history" => {
            let mut value: Value = serde_json::from_str(&output)
                .map_err(|e| format!("failed to parse stellar CLI JSON output: {e}; output={output}"))?;
            normalize_commitment_fields(&mut value)?;
            Ok((StatusCode::OK, Json(value)).into_response())
        }
        other => Err(format!("unsupported function: {other}")),
    }
}

fn parse_bool_output(output: &str) -> Result<bool, String> {
    let trimmed = output.trim();
    if trimmed.eq_ignore_ascii_case("true") {
        return Ok(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Ok(false);
    }
    serde_json::from_str::<bool>(trimmed)
        .map_err(|e| format!("failed to parse boolean output: {e}; output={trimmed}"))
}

fn normalize_commitment_fields(value: &mut Value) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if key == "commitment" {
                    if let Some(s) = child.as_str() {
                        *child = Value::String(hex_to_base64_if_needed(s)?);
                    }
                } else {
                    normalize_commitment_fields(child)?;
                }
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                normalize_commitment_fields(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn hex_to_base64_if_needed(s: &str) -> Result<String, String> {
    if s.len() % 2 == 0 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        let bytes = hex::decode(s).map_err(|e| format!("invalid hex commitment: {e}"))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    } else {
        Ok(s.to_string())
    }
}

/// Invoke the Soroban contract via the `stellar` CLI.
async fn invoke_contract(config: &Config, request: &RelayerRequest) -> Result<String, String> {
    let function = &request.function;
    let payload = &request.payload;
    let is_read_only = READ_ONLY_FUNCTIONS.contains(&function.as_str());

    let mut cmd = Command::new("stellar");
    cmd.arg("contract")
        .arg("invoke")
        .arg("--rpc-url")
        .arg(&config.rpc_url)
        .arg("--network-passphrase")
        .arg(&config.network_passphrase)
        .arg("--network")
        .arg(&config.network)
        .arg("--id")
        .arg(&config.contract_id)
        .arg("--source-account")
        .arg(&config.identity_name);

    if is_read_only {
        cmd.arg("--send").arg("no");
    }

    cmd.arg("--");
    cmd.arg(function);

    // Build function-specific arguments
    match function.as_str() {
        "create_group" => {
            // Replace caller with relayer's own address (contract requires caller.require_auth())
            cmd.arg("--caller").arg(&config.public_address);
            add_hex_arg(&mut cmd, "--group-id", payload, "groupID")?;
            add_hex_arg(&mut cmd, "--commitment", payload, "commitment")?;
            add_int_arg(&mut cmd, "--tier", payload, "tier")?;
            add_proof_arg(&mut cmd, payload)?;
            add_membership_public_inputs_arg(&mut cmd, payload)?;
        }
        "update_commitment" => {
            // #59: UpdateCircuit binds c_new cryptographically. The relayer no
            // longer forwards client-supplied `new_commitment` or `new_epoch`;
            // c_new comes from the UpdatePublicInputs payload and the contract
            // derives new_epoch on-chain as stored_epoch + 1.
            add_hex_arg(&mut cmd, "--group-id", payload, "groupID")?;
            add_proof_arg(&mut cmd, payload)?;
            add_update_public_inputs_arg(&mut cmd, payload)?;
        }
        "verify_membership" => {
            add_hex_arg(&mut cmd, "--group-id", payload, "groupID")?;
            add_proof_arg(&mut cmd, payload)?;
            add_membership_public_inputs_arg(&mut cmd, payload)?;
        }
        "deactivate_group" => {
            add_hex_arg(&mut cmd, "--group-id", payload, "groupID")?;
            add_proof_arg(&mut cmd, payload)?;
            add_membership_public_inputs_arg(&mut cmd, payload)?;
        }
        "get_state" => {
            add_hex_arg(&mut cmd, "--group-id", payload, "groupID")?;
        }
        "get_history" => {
            add_hex_arg(&mut cmd, "--group-id", payload, "groupID")?;
            let max_entries = payload
                .get("maxEntries")
                .and_then(|v| v.as_u64())
                .unwrap_or(64);
            cmd.arg("--max-entries").arg(max_entries.to_string());
        }
        _ => return Err(format!("unsupported function: {function}")),
    }

    // Execute
    let output = tokio::task::spawn_blocking(move || cmd.output())
        .await
        .map_err(|e| format!("task join error: {e}"))?
        .map_err(|e| format!("failed to execute stellar CLI: {e}"))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(format!("{stderr} {stdout}").trim().to_string())
    }
}

/// Decode a base64 payload field and add as a hex argument.
fn add_hex_arg(
    cmd: &mut Command,
    flag: &str,
    payload: &Value,
    field: &str,
) -> Result<(), String> {
    let b64 = payload
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing field: {field}"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("invalid base64 for {field}: {e}"))?;
    cmd.arg(flag).arg(hex_encode(&bytes));
    Ok(())
}

/// Add an integer argument from the payload.
fn add_int_arg(
    cmd: &mut Command,
    flag: &str,
    payload: &Value,
    field: &str,
) -> Result<(), String> {
    let val = payload
        .get(field)
        .and_then(|v| v.as_u64())
        .ok_or_else(|| format!("missing or invalid field: {field}"))?;
    cmd.arg(flag).arg(val.to_string());
    Ok(())
}

/// Decode the proof from base64 and add as a JSON file-path argument.
/// The proof is 384 bytes: a(96) || b(192) || c(96).
fn add_proof_arg(cmd: &mut Command, payload: &Value) -> Result<(), String> {
    let proof_b64 = payload
        .get("proof")
        .and_then(|v| v.as_str())
        .ok_or("missing proof field")?;
    let proof_bytes = base64::engine::general_purpose::STANDARD
        .decode(proof_b64)
        .map_err(|e| format!("invalid proof base64: {e}"))?;
    if proof_bytes.len() != 384 {
        return Err(format!(
            "proof must be 384 bytes, got {}",
            proof_bytes.len()
        ));
    }
    let a = &proof_bytes[0..96];
    let b = &proof_bytes[96..288];
    let c = &proof_bytes[288..384];

    let proof_json = format!(
        "{{\"a\":\"{}\",\"b\":\"{}\",\"c\":\"{}\"}}",
        hex_encode(a),
        hex_encode(b),
        hex_encode(c)
    );

    // Write to a temp file since stellar CLI expects --proof-file-path
    let tmp = std::env::temp_dir().join(format!("sep-proof-{}.json", std::process::id()));
    std::fs::write(&tmp, &proof_json)
        .map_err(|e| format!("failed to write temp proof file: {e}"))?;
    cmd.arg("--proof-file-path")
        .arg(tmp.to_str().unwrap());
    Ok(())
}

/// Decode membership-circuit public inputs and add as a JSON file-path argument.
/// Used by create_group, verify_membership, and deactivate_group.
/// Expects `payload.publicInputs = { commitment: base64, epoch: u64 }`.
fn add_membership_public_inputs_arg(cmd: &mut Command, payload: &Value) -> Result<(), String> {
    let pi = payload
        .get("publicInputs")
        .ok_or("missing publicInputs field")?;
    let commitment_b64 = pi
        .get("commitment")
        .and_then(|v| v.as_str())
        .ok_or("missing publicInputs.commitment")?;
    let epoch = pi
        .get("epoch")
        .and_then(|v| v.as_u64())
        .ok_or("missing publicInputs.epoch")?;

    let commitment_bytes = base64::engine::general_purpose::STANDARD
        .decode(commitment_b64)
        .map_err(|e| format!("invalid commitment base64: {e}"))?;

    let pi_json = format!(
        "{{\"commitment\":\"{}\",\"epoch\":{}}}",
        hex_encode(&commitment_bytes),
        epoch
    );

    let tmp = std::env::temp_dir().join(format!("sep-pi-{}.json", std::process::id()));
    std::fs::write(&tmp, &pi_json)
        .map_err(|e| format!("failed to write temp public inputs file: {e}"))?;
    cmd.arg("--public-inputs-file-path")
        .arg(tmp.to_str().unwrap());
    Ok(())
}

/// Decode update-circuit public inputs and add as a JSON file-path argument.
/// Used by update_commitment. Expects:
/// `payload.publicInputs = { c_old: base64, epoch_old: u64, c_new: base64 }`.
/// The contract's `UpdatePublicInputs` binds all three values through the proof;
/// the relayer no longer propagates a client-supplied `new_commitment` / `new_epoch`
/// pair (#59 fix).
fn add_update_public_inputs_arg(cmd: &mut Command, payload: &Value) -> Result<(), String> {
    let pi = payload
        .get("publicInputs")
        .ok_or("missing publicInputs field")?;
    let c_old_b64 = pi
        .get("c_old")
        .and_then(|v| v.as_str())
        .ok_or("missing publicInputs.c_old")?;
    let epoch_old = pi
        .get("epoch_old")
        .and_then(|v| v.as_u64())
        .ok_or("missing publicInputs.epoch_old")?;
    let c_new_b64 = pi
        .get("c_new")
        .and_then(|v| v.as_str())
        .ok_or("missing publicInputs.c_new")?;

    let c_old_bytes = base64::engine::general_purpose::STANDARD
        .decode(c_old_b64)
        .map_err(|e| format!("invalid c_old base64: {e}"))?;
    let c_new_bytes = base64::engine::general_purpose::STANDARD
        .decode(c_new_b64)
        .map_err(|e| format!("invalid c_new base64: {e}"))?;

    let pi_json = format!(
        "{{\"c_old\":\"{}\",\"epoch_old\":{},\"c_new\":\"{}\"}}",
        hex_encode(&c_old_bytes),
        epoch_old,
        hex_encode(&c_new_bytes)
    );

    let tmp = std::env::temp_dir().join(format!("sep-upi-{}.json", std::process::id()));
    std::fs::write(&tmp, &pi_json)
        .map_err(|e| format!("failed to write temp update public inputs file: {e}"))?;
    cmd.arg("--public-inputs-file-path")
        .arg(tmp.to_str().unwrap());
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(nibble_to_hex(byte >> 4));
        out.push(nibble_to_hex(byte & 0x0f));
    }
    out
}

fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- hex_encode tests ----

    #[test]
    fn test_hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn test_hex_encode_bytes() {
        assert_eq!(hex_encode(&[0x01, 0x23, 0xab, 0xcd]), "0123abcd");
    }

    #[test]
    fn test_hex_encode_all_ff() {
        assert_eq!(hex_encode(&[0xff, 0xff, 0xff]), "ffffff");
    }

    // ---- parse_bool_output tests ----

    #[test]
    fn test_parse_bool_output_true() {
        assert_eq!(parse_bool_output("true").unwrap(), true);
    }

    #[test]
    fn test_parse_bool_output_false() {
        assert_eq!(parse_bool_output("false").unwrap(), false);
    }

    #[test]
    fn test_parse_bool_output_case_insensitive() {
        assert_eq!(parse_bool_output("TRUE").unwrap(), true);
        assert_eq!(parse_bool_output("False").unwrap(), false);
        assert_eq!(parse_bool_output("TrUe").unwrap(), true);
    }

    #[test]
    fn test_parse_bool_output_with_whitespace() {
        assert_eq!(parse_bool_output("  true  \n").unwrap(), true);
        assert_eq!(parse_bool_output("\tfalse\t").unwrap(), false);
    }

    #[test]
    fn test_parse_bool_output_invalid() {
        assert!(parse_bool_output("yes").is_err());
        assert!(parse_bool_output("1").is_err());
        assert!(parse_bool_output("").is_err());
    }

    // ---- hex_to_base64_if_needed tests ----

    #[test]
    fn test_hex_to_base64_valid() {
        // 0xdeadbeef -> 3q2+7w==
        let result = hex_to_base64_if_needed("deadbeef").unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&result)
            .unwrap();
        assert_eq!(decoded, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn test_hex_to_base64_odd_length_rejected() {
        // Odd-length strings are not valid hex, so the function returns the input unchanged
        let result = hex_to_base64_if_needed("abc").unwrap();
        assert_eq!(result, "abc");
    }

    // ---- normalize_commitment_fields tests ----

    #[test]
    fn test_normalize_commitment_flat_object() {
        let mut value = serde_json::json!({
            "commitment": "deadbeef",
            "epoch": 1
        });
        normalize_commitment_fields(&mut value).unwrap();

        // commitment should be base64 now
        let commitment = value["commitment"].as_str().unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(commitment)
            .unwrap();
        assert_eq!(decoded, vec![0xde, 0xad, 0xbe, 0xef]);

        // epoch should be unchanged
        assert_eq!(value["epoch"], 1);
    }

    #[test]
    fn test_normalize_commitment_nested_array() {
        let mut value = serde_json::json!([
            { "commitment": "0011ff", "name": "group1" },
            { "commitment": "aabb", "name": "group2" }
        ]);
        normalize_commitment_fields(&mut value).unwrap();

        // Both commitments should be converted
        let c0 = value[0]["commitment"].as_str().unwrap();
        let d0 = base64::engine::general_purpose::STANDARD.decode(c0).unwrap();
        assert_eq!(d0, vec![0x00, 0x11, 0xff]);

        let c1 = value[1]["commitment"].as_str().unwrap();
        let d1 = base64::engine::general_purpose::STANDARD.decode(c1).unwrap();
        assert_eq!(d1, vec![0xaa, 0xbb]);

        // Other fields should be untouched
        assert_eq!(value[0]["name"], "group1");
        assert_eq!(value[1]["name"], "group2");
    }
}
