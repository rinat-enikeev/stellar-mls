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
            let value: Value = serde_json::from_str(&output)
                .map_err(|e| format!("failed to parse stellar CLI JSON output: {e}; output={output}"))?;
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
            add_public_inputs_arg(&mut cmd, payload)?;
        }
        "update_commitment" => {
            add_hex_arg(&mut cmd, "--group-id", payload, "groupID")?;
            add_hex_arg(&mut cmd, "--new-commitment", payload, "newCommitment")?;
            add_int_arg(&mut cmd, "--new-epoch", payload, "newEpoch")?;
            add_proof_arg(&mut cmd, payload)?;
            add_public_inputs_arg(&mut cmd, payload)?;
        }
        "verify_membership" => {
            add_hex_arg(&mut cmd, "--group-id", payload, "groupID")?;
            add_proof_arg(&mut cmd, payload)?;
            add_public_inputs_arg(&mut cmd, payload)?;
        }
        "deactivate_group" => {
            add_hex_arg(&mut cmd, "--group-id", payload, "groupID")?;
            add_proof_arg(&mut cmd, payload)?;
            add_public_inputs_arg(&mut cmd, payload)?;
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

/// Decode public inputs and add as a JSON file-path argument.
fn add_public_inputs_arg(cmd: &mut Command, payload: &Value) -> Result<(), String> {
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
