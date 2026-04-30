//! Shared helpers for the `gen-*-proof` CLI binaries.
//!
//! Each binary declares `mod common;` so this module is compiled
//! once per bin (Cargo's standard `src/bin/` convention). Items
//! that appear unused in a given bin trigger `dead_code` — they're
//! `#[allow(dead_code)]`-tagged below since the deduplication
//! benefit outweighs the warning suppression.

#![allow(dead_code)]
#![cfg(feature = "plonk")]

use std::fmt;
use std::fs;
use std::path::Path;

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;

#[derive(Debug)]
pub enum CliError {
    Usage(String),
    Io(std::io::Error),
    Circuit(jf_relation::CircuitError),
    Plonk(jf_plonk::errors::PlonkError),
    Serialize(ark_serialize_v05::SerializationError),
    Other(String),
}

impl CliError {
    pub fn usage(msg: impl Into<String>) -> Self {
        Self::Usage(msg.into())
    }
    pub fn io(e: std::io::Error) -> Self {
        Self::Io(e)
    }
    pub fn circuit(e: jf_relation::CircuitError) -> Self {
        Self::Circuit(e)
    }
    pub fn plonk(e: jf_plonk::errors::PlonkError) -> Self {
        Self::Plonk(e)
    }
    pub fn serialize(e: ark_serialize_v05::SerializationError) -> Self {
        Self::Serialize(e)
    }
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(s) => write!(f, "usage error: {s}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Circuit(e) => write!(f, "circuit error: {e:?}"),
            Self::Plonk(e) => write!(f, "plonk error: {e:?}"),
            Self::Serialize(e) => write!(f, "serialise error: {e:?}"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

/// Strip an optional `0x` / `0X` prefix and decode hex to bytes.
pub fn decode_hex(s: &str) -> Result<Vec<u8>, CliError> {
    let trimmed = s.trim_start_matches("0x").trim_start_matches("0X");
    if trimmed.len() % 2 != 0 {
        return Err(CliError::usage(format!(
            "hex string must have even length, got {} chars: {s:?}",
            trimmed.len()
        )));
    }
    let mut out = Vec::with_capacity(trimmed.len() / 2);
    let mut iter = trimmed.chars();
    while let (Some(h), Some(l)) = (iter.next(), iter.next()) {
        let hi = h.to_digit(16).ok_or_else(|| {
            CliError::usage(format!("non-hex character {h:?} in {s:?}"))
        })?;
        let lo = l.to_digit(16).ok_or_else(|| {
            CliError::usage(format!("non-hex character {l:?} in {s:?}"))
        })?;
        out.push(((hi << 4) | lo) as u8);
    }
    Ok(out)
}

/// Decode a 32-byte BE hex string into an `Fr` scalar via
/// `from_be_bytes_mod_order`. Accepts shorter inputs (pads on the
/// high-byte side); the resulting scalar is the BE big-integer mod r.
pub fn parse_fr_hex(s: &str) -> Result<Fr, CliError> {
    let bytes = decode_hex(s)?;
    if bytes.len() > 32 {
        return Err(CliError::usage(format!(
            "Fr hex must be ≤ 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut padded = [0u8; 32];
    padded[32 - bytes.len()..].copy_from_slice(&bytes);
    Ok(Fr::from_be_bytes_mod_order(&padded))
}

/// Parse a comma-separated list of `Fr` hex values.
pub fn parse_fr_list(s: &str) -> Result<Vec<Fr>, CliError> {
    if s.is_empty() {
        return Err(CliError::usage("empty Fr list"));
    }
    s.split(',').map(|p| parse_fr_hex(p.trim())).collect()
}

/// Decode a 32-byte salt hex string into a fixed-size array. Salt is
/// LE-mod-r encoded inside the circuit (matching the membership /
/// update circuits).
pub fn parse_salt_hex(s: &str) -> Result<[u8; 32], CliError> {
    let bytes = decode_hex(s)?;
    if bytes.len() != 32 {
        return Err(CliError::usage(format!(
            "salt must be exactly 32 bytes, got {}",
            bytes.len()
        )));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Write `bytes` to both `<dir>/<name>.bin` (raw) and
/// `<dir>/<name>.hex` (hex-encoded, no `0x`, single-line, trailing
/// newline).
pub fn write_bin_and_hex(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), CliError> {
    fs::write(dir.join(format!("{name}.bin")), bytes).map_err(CliError::io)?;
    let mut hex_str = String::with_capacity(bytes.len() * 2 + 1);
    for b in bytes {
        hex_str.push_str(&format!("{:02x}", b));
    }
    hex_str.push('\n');
    fs::write(dir.join(format!("{name}.hex")), &hex_str).map_err(CliError::io)?;
    Ok(())
}
