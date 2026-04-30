//! Shared byte ↔ Fr / G1 / G2 conversion helpers.
//!
//! Multiple modules need the same handful of Soroban-`Fr` /
//! Soroban-`G1Affine` / Soroban-`G2Affine` constructors from byte
//! arrays, plus a fixed-size `decode_fr_array`. Centralising them
//! here:
//!
//! - Avoids the byte-for-byte duplication previously copy-pasted
//!   into `verifier_aggregate.rs`, `verifier_aggregate_evals.rs`,
//!   and `verifier.rs`.
//! - Means a future Soroban SDK change (e.g. an Fr-encoding tweak,
//!   though unlikely) only needs to touch one place.
//!
//! `pub(crate)` because the verifier crate's public surface should
//! remain the verifier modules themselves; downstream contracts use
//! `verifier::verify` and don't need to know about Fr-byte
//! conversions.

use soroban_sdk::crypto::bls12_381::{Fr, G1Affine, G2Affine};
use soroban_sdk::{BytesN, Env};

use crate::proof_format::{FR_LEN, G1_LEN};
use crate::vk_format::G2_LEN;

/// Convert a 32-byte arkworks-LE Fr representation into a Soroban
/// `Fr`. Soroban's `Fr::from_bytes` consumes BE, so we reverse first.
pub(crate) fn fr_from_le_bytes(env: &Env, le: &[u8; FR_LEN]) -> Fr {
    let mut be = [0u8; FR_LEN];
    for (o, &b) in be.iter_mut().zip(le.iter().rev()) {
        *o = b;
    }
    Fr::from_bytes(BytesN::from_array(env, &be))
}

/// Build the canonical zero `Fr`.
pub(crate) fn fr_zero(env: &Env) -> Fr {
    Fr::from_bytes(BytesN::from_array(env, &[0u8; FR_LEN]))
}

/// Build the canonical one `Fr` (BE: high bytes zero, low byte = 1).
pub(crate) fn fr_one(env: &Env) -> Fr {
    let mut bytes = [0u8; FR_LEN];
    bytes[FR_LEN - 1] = 0x01;
    Fr::from_bytes(BytesN::from_array(env, &bytes))
}

/// Parse a 96-byte arkworks-uncompressed G1 into a Soroban `G1Affine`.
///
/// **Not validation.** Soroban's `from_bytes` accepts any 96-byte
/// blob; on-curve / subgroup checks happen later when the bases are
/// fed into `g1_msm` / `pairing_check` host primitives. Off-curve
/// inputs trap inside the host call (consensus-safe — trap rejects
/// the proof — but the trap surfaces as a contract failure rather
/// than a `Result::Err`).
pub(crate) fn g1_from_bytes(env: &Env, bytes: &[u8; G1_LEN]) -> G1Affine {
    G1Affine::from_bytes(BytesN::from_array(env, bytes))
}

/// Parse a 192-byte arkworks-uncompressed G2 into a Soroban
/// `G2Affine`. Same not-validation semantics as
/// [`g1_from_bytes`].
pub(crate) fn g2_from_bytes(env: &Env, bytes: &[u8; G2_LEN]) -> G2Affine {
    G2Affine::from_bytes(BytesN::from_array(env, bytes))
}

/// Decode a fixed-size array of arkworks-LE Fr byte slots into a
/// `[Fr; N]`. Used in `verifier_aggregate` and `verifier` where the
/// proof's `wires_evals` / `wire_sigma_evals` come back as
/// `[[u8; FR_LEN]; N]`.
pub(crate) fn decode_fr_array<const N: usize>(
    env: &Env,
    arrays: &[[u8; FR_LEN]; N],
) -> [Fr; N] {
    core::array::from_fn(|i| fr_from_le_bytes(env, &arrays[i]))
}

/// Decode a fixed-size array of arkworks-uncompressed G1 byte slots
/// into a `[G1Affine; N]`. Same not-validation semantics as
/// [`g1_from_bytes`].
pub(crate) fn decode_g1_array<const N: usize>(
    env: &Env,
    arrays: &[[u8; G1_LEN]; N],
) -> [G1Affine; N] {
    core::array::from_fn(|i| g1_from_bytes(env, &arrays[i]))
}
