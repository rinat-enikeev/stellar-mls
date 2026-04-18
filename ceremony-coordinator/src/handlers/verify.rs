//! Server-side verify convenience for CLI clients.
//! Browser clients should prefer the WASM verifier.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use crate::ceremony_exec::VerifyArtifacts;
use crate::model::{Tier, VerifyStateRequest, VerifyStateResponse};
use crate::SharedState;
use std::collections::BTreeMap;
use std::str::FromStr;

pub async fn verify_state(
    State(state): State<SharedState>,
    Json(req): Json<VerifyStateRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Pull SRS bytes from Blossom by hash — avoids accepting arbitrary big
    // binary payloads in a verify endpoint.
    let srs_bytes = state
        .blossom
        .get(&req.srs_blob_sha256)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("blossom: {e}")))?;

    let artifacts = VerifyArtifacts {
        state_srs: srs_bytes,
        state_txt: req.state_txt.clone(),
        receipt: req.receipt.clone(),
    };

    match state.ceremony_tool.verify_state(&artifacts).await {
        Ok(_) => {
            let kv = parse_kv(&req.state_txt);
            let round = kv.get("round").and_then(|v| v.parse().ok());
            let tier = kv
                .get("tier")
                .and_then(|v| Tier::from_str(v).ok());
            let srs_hash = kv.get("srs_hash").cloned();
            Ok(Json(VerifyStateResponse {
                ok: true,
                round,
                tier,
                srs_hash,
                error: None,
            }))
        }
        Err(e) => Ok(Json(VerifyStateResponse {
            ok: false,
            round: None,
            tier: None,
            srs_hash: None,
            error: Some(e.to_string()),
        })),
    }
}

fn parse_kv(s: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in s.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.insert(k.to_string(), v.to_string());
        }
    }
    out
}
