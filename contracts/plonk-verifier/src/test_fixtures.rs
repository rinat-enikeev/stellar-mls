//! Test fixture helpers used by the parser unit tests.
//!
//! The Soroban verifier crate is no_std and standalone — it can't
//! depend on the prover-side `sep-xxxx-circuits` to generate byte
//! fixtures. Instead, we build synthetic byte streams with valid
//! length prefixes and a deterministic sentinel-byte payload
//! (different first byte per slot), then assert each split landed
//! at the right offset.
//!
//! This is sufficient for testing the **parser logic** here. The
//! byte-format spec itself (offsets, length-prefix values,
//! payload-byte semantics) is pinned by the prover-side reference's
//! oracle test against `jf_plonk::Proof::deserialize_uncompressed` /
//! `VerifyingKey::deserialize_uncompressed`. The parsing logic is
//! copy-pasted from that reference, so byte-equivalence transfers.
//!
//! Real-fixture tests — `verifier::tests::accepts_canonical_proof`,
//! the rejection-mutation siblings, and
//! `verifier_aggregate::tests::msm_is_deterministic` — load the
//! depth-5 canonical artifacts under `tests/fixtures/` directly via
//! `include_bytes!`. Those bytes are produced (and checksummed) by
//! the prover-side
//! `circuit::plonk::verifier::tests::plonk_verifier_fixtures_match_or_regenerate`
//! test, which doubles as a drift detector when run without
//! `STELLAR_REGEN_FIXTURES=1`.

use crate::proof_format::{
    FR_LEN, G1_LEN, NUM_WIRE_SIGMA_EVALS, NUM_WIRE_TYPES, PROOF_LEN,
};
use crate::vk_format::{
    G2_LEN as VK_G2_LEN, NUM_K_CONSTANTS, NUM_POWERS_OF_G, NUM_POWERS_OF_H,
    NUM_SELECTOR_COMMS, NUM_SIGMA_COMMS, VK_LEN,
};

/// Write a u64 LE at `offset`. Used by reject-path tests to swap out
/// the canonical length prefix without rebuilding the whole stream.
pub(crate) fn fill_u64_le(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

/// Build a synthetic 1601-byte proof byte stream:
/// - All length prefixes set to the values the parser expects.
/// - Each commitment / evaluation slot has its first byte set to a
///   deterministic sentinel (`0x10..0x70` ranges) so tests can assert
///   each split landed correctly. Remaining payload bytes are zero.
/// - `plookup_proof` byte = 0x00 (valid None).
pub(crate) fn build_synthetic_proof_bytes() -> [u8; PROOF_LEN] {
    let mut b = [0u8; PROOF_LEN];

    // Layout offsets, derived from `proof_format`'s layout table:
    const OFF_WIRE_LEN: usize = 0;
    const OFF_WIRE_FIRST: usize = 8;
    const OFF_PROD_PERM: usize = OFF_WIRE_FIRST + NUM_WIRE_TYPES * G1_LEN; // 488
    const OFF_QUOT_LEN: usize = OFF_PROD_PERM + G1_LEN; // 584
    const OFF_QUOT_FIRST: usize = OFF_QUOT_LEN + 8; // 592
    const OFF_OPENING: usize = OFF_QUOT_FIRST + NUM_WIRE_TYPES * G1_LEN; // 1072
    const OFF_SHIFTED_OPENING: usize = OFF_OPENING + G1_LEN; // 1168
    const OFF_WIRES_EVAL_LEN: usize = OFF_SHIFTED_OPENING + G1_LEN; // 1264
    const OFF_WIRES_EVAL_FIRST: usize = OFF_WIRES_EVAL_LEN + 8; // 1272
    const OFF_SIGMA_EVAL_LEN: usize = OFF_WIRES_EVAL_FIRST + NUM_WIRE_TYPES * FR_LEN; // 1432
    const OFF_SIGMA_EVAL_FIRST: usize = OFF_SIGMA_EVAL_LEN + 8; // 1440
    const OFF_PERM_NEXT_EVAL: usize = OFF_SIGMA_EVAL_FIRST + NUM_WIRE_SIGMA_EVALS * FR_LEN; // 1568
    const OFF_PLOOKUP_OPT: usize = OFF_PERM_NEXT_EVAL + FR_LEN; // 1600

    fill_u64_le(&mut b, OFF_WIRE_LEN, NUM_WIRE_TYPES as u64);
    for i in 0..NUM_WIRE_TYPES {
        b[OFF_WIRE_FIRST + i * G1_LEN] = 0x10 + i as u8;
    }
    b[OFF_PROD_PERM] = 0x20;

    fill_u64_le(&mut b, OFF_QUOT_LEN, NUM_WIRE_TYPES as u64);
    for i in 0..NUM_WIRE_TYPES {
        b[OFF_QUOT_FIRST + i * G1_LEN] = 0x30 + i as u8;
    }
    b[OFF_OPENING] = 0x40;
    b[OFF_SHIFTED_OPENING] = 0x41;

    fill_u64_le(&mut b, OFF_WIRES_EVAL_LEN, NUM_WIRE_TYPES as u64);
    for i in 0..NUM_WIRE_TYPES {
        b[OFF_WIRES_EVAL_FIRST + i * FR_LEN] = 0x50 + i as u8;
    }

    fill_u64_le(&mut b, OFF_SIGMA_EVAL_LEN, NUM_WIRE_SIGMA_EVALS as u64);
    for i in 0..NUM_WIRE_SIGMA_EVALS {
        b[OFF_SIGMA_EVAL_FIRST + i * FR_LEN] = 0x60 + i as u8;
    }

    b[OFF_PERM_NEXT_EVAL] = 0x70;

    // plookup_proof = None → 0x00 (already zero from init).
    let _ = OFF_PLOOKUP_OPT;

    b
}

/// Build a synthetic 3002-byte VK byte stream with the supplied
/// `domain_size` / `num_inputs`. Each commitment slot has a
/// deterministic sentinel first byte (`0x10..0x60` ranges).
pub(crate) fn build_synthetic_vk_bytes(domain_size: u64, num_inputs: u64) -> [u8; VK_LEN] {
    let mut b = [0u8; VK_LEN];

    const OFF_DOMAIN_SIZE: usize = 0;
    const OFF_NUM_INPUTS: usize = 8;
    const OFF_SIGMA_LEN: usize = 16;
    const OFF_SIGMA_FIRST: usize = 24;
    const OFF_SELECTOR_LEN: usize = OFF_SIGMA_FIRST + NUM_SIGMA_COMMS * G1_LEN; // 504
    const OFF_SELECTOR_FIRST: usize = OFF_SELECTOR_LEN + 8; // 512
    const OFF_K_LEN: usize = OFF_SELECTOR_FIRST + NUM_SELECTOR_COMMS * G1_LEN; // 1760
    const OFF_K_FIRST: usize = OFF_K_LEN + 8; // 1768
    const OFF_OPEN_G: usize = OFF_K_FIRST + NUM_K_CONSTANTS * FR_LEN; // 1928
    const OFF_OPEN_H: usize = OFF_OPEN_G + G1_LEN; // 2024
    const OFF_OPEN_BETA_H: usize = OFF_OPEN_H + VK_G2_LEN; // 2216
    const OFF_POWERS_OF_H_LEN: usize = OFF_OPEN_BETA_H + VK_G2_LEN; // 2408
    const OFF_POWERS_OF_H_FIRST: usize = OFF_POWERS_OF_H_LEN + 8; // 2416
    const OFF_POWERS_OF_G_LEN: usize = OFF_POWERS_OF_H_FIRST + NUM_POWERS_OF_H * VK_G2_LEN; // 2800
    const OFF_POWERS_OF_G_FIRST: usize = OFF_POWERS_OF_G_LEN + 8; // 2808

    fill_u64_le(&mut b, OFF_DOMAIN_SIZE, domain_size);
    fill_u64_le(&mut b, OFF_NUM_INPUTS, num_inputs);

    fill_u64_le(&mut b, OFF_SIGMA_LEN, NUM_SIGMA_COMMS as u64);
    for i in 0..NUM_SIGMA_COMMS {
        b[OFF_SIGMA_FIRST + i * G1_LEN] = 0x10 + i as u8;
    }
    fill_u64_le(&mut b, OFF_SELECTOR_LEN, NUM_SELECTOR_COMMS as u64);
    for i in 0..NUM_SELECTOR_COMMS {
        b[OFF_SELECTOR_FIRST + i * G1_LEN] = 0x20 + i as u8;
    }
    fill_u64_le(&mut b, OFF_K_LEN, NUM_K_CONSTANTS as u64);
    for i in 0..NUM_K_CONSTANTS {
        b[OFF_K_FIRST + i * FR_LEN] = 0x30 + i as u8;
    }

    b[OFF_OPEN_G] = 0x40;
    b[OFF_OPEN_H] = 0x41;
    b[OFF_OPEN_BETA_H] = 0x42;

    fill_u64_le(&mut b, OFF_POWERS_OF_H_LEN, NUM_POWERS_OF_H as u64);
    for i in 0..NUM_POWERS_OF_H {
        b[OFF_POWERS_OF_H_FIRST + i * VK_G2_LEN] = 0x50 + i as u8;
    }
    fill_u64_le(&mut b, OFF_POWERS_OF_G_LEN, NUM_POWERS_OF_G as u64);
    for i in 0..NUM_POWERS_OF_G {
        b[OFF_POWERS_OF_G_FIRST + i * G1_LEN] = 0x60 + i as u8;
    }

    // is_merged + plookup_vk = 0x00 (already zero).

    b
}
