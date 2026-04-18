//! Transcript (de)serialization helpers.
//!
//! `state.txt` and `receipt.txt` are key-value ASCII files produced by
//! `ceremony_tool`. This module contains the parsing primitives shared by
//! the native CLI, the coordinator (via subprocess), and the browser WASM
//! verifier (direct in-process).

use std::collections::BTreeMap;

use ark_bls12_381::{G1Affine, G2Affine};
use ark_serialize::CanonicalDeserialize;

use crate::ceremony::{phase2, ContributionProof, PowersOfTau};

/// Parse the `key=value` ASCII format used by `state.txt` and `receipt.txt`.
///
/// Blank lines and lines starting with `#` are ignored. Unknown keys are
/// preserved in the map.
pub fn parse_kv(contents: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (index, line) in contents.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("invalid line {}: {}", index + 1, line));
        };
        out.insert(key.to_string(), value.to_string());
    }
    Ok(out)
}

/// Fetch a required key, returning a descriptive error if absent.
pub fn required<'a>(kv: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    kv.get(key)
        .map(|value| value.as_str())
        .ok_or_else(|| format!("missing key '{key}'"))
}

/// Deserialize `state.srs` bytes (arkworks canonical, uncompressed) into
/// the in-memory `PowersOfTau` structure.
pub fn parse_srs(bytes: &[u8]) -> Result<PowersOfTau, String> {
    phase2::import_srs(bytes)
}

/// Reconstruct a `ContributionProof` from the hex fields embedded in a
/// parsed receipt. Accepts the key names written by `ceremony_tool`.
pub fn parse_proof(receipt: &BTreeMap<String, String>) -> Result<ContributionProof, String> {
    Ok(ContributionProof {
        tau_proof: (
            deserialize_g1_hex(required(receipt, "tau_proof_g1")?)?,
            deserialize_g1_hex(required(receipt, "tau_proof_delta_g1")?)?,
        ),
        alpha_proof: (
            deserialize_g2_hex(required(receipt, "alpha_proof_g2")?)?,
            deserialize_g2_hex(required(receipt, "alpha_proof_delta_g2")?)?,
        ),
        beta_proof: (
            deserialize_g1_hex(required(receipt, "beta_proof_g1")?)?,
            deserialize_g1_hex(required(receipt, "beta_proof_delta_g1")?)?,
        ),
    })
}

pub fn deserialize_g1_hex(value: &str) -> Result<G1Affine, String> {
    let bytes = hex_decode(value)?;
    G1Affine::deserialize_compressed(&bytes[..])
        .map_err(|e| format!("failed to deserialize G1 point: {e}"))
}

pub fn deserialize_g2_hex(value: &str) -> Result<G2Affine, String> {
    let bytes = hex_decode(value)?;
    G2Affine::deserialize_compressed(&bytes[..])
        .map_err(|e| format!("failed to deserialize G2 point: {e}"))
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 {
        return Err("hex string has odd length".to_string());
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let high = hex_value(bytes[i])?;
        let low = hex_value(bytes[i + 1])?;
        out.push((high << 4) | low);
        i += 2;
    }
    Ok(out)
}

fn hex_value(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(format!("invalid hex byte: 0x{byte:02x}")),
    }
}
