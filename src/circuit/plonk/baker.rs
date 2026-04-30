//! VK-baking helpers — produce deterministic verifying-key bytes for
//! the on-chain Soroban verifier.
//!
//! Phase C.2 contracts embed VKs as `static` byte slices:
//!
//! ```rust,ignore
//! pub const VK_BYTES: &[u8] = include_bytes!("membership-d11.vk.bin");
//! ```
//!
//! Those bytes are produced here via `bake_membership_vk(depth)`,
//! which mirrors the canonical-witness preprocessing path also
//! exercised by `test_vectors::verify_plonk_membership_vk_fingerprints`.
//! The cross-platform-anchor SHA-256 constants are the source of truth;
//! `bake_membership_vk` is verified against them in the same test.
//!
//! The CLI wrapper at `src/bin/bake_vk.rs` (under `feature =
//! "bake-vk-tool"`) calls `bake_membership_vk` and writes the result
//! to disk. Contracts then `include_bytes!` that file — no jf-plonk
//! types cross the contract boundary.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use ark_serialize_v05::CanonicalSerialize;
use jf_relation::PlonkCircuit;
use sha2::{Digest, Sha256};

use crate::circuit::plonk::membership::{synthesize_membership, MembershipWitness};
use crate::circuit::plonk::poseidon::{poseidon_hash_one_v05, poseidon_hash_two_v05};
use crate::prover::plonk;

// ---------------------------------------------------------------------------
// Pinned VK fingerprints — the cross-platform invariant.
//
// If any of these change, either:
// - the circuit shape changed (gate order, public-input order, gadget
//   internals) — review the diff carefully and update;
// - the SRS changed (build.rs would also have caught the hash mismatch
//   first);
// - jf-plonk's `preprocess` output format changed at the byte level —
//   in which case all consumers (Soroban verifier, mobile clients)
//   need a coordinated update too.
//
// Mirror these into `docs/cross-platform-test-vectors.json` under
// `plonk_membership_vk_fingerprints`. Non-Rust platforms compute the
// same SHA-256 over their `VerifyingKey::serialize_uncompressed`
// output and assert byte-equality.
// ---------------------------------------------------------------------------

/// SHA-256 of the canonical small-tier (depth=5) VK, hex-encoded.
pub const VK_SHA256_HEX_SMALL: &str =
    "a552b41c2e40167b74ccbec36d83cc931279d278e364cacb99a3a5ce9c26e5ab";

/// SHA-256 of the canonical medium-tier (depth=8) VK, hex-encoded.
pub const VK_SHA256_HEX_MEDIUM: &str =
    "b4f98a146dad1de3447dde3b686ac2ddf8b4cdb153ad69f407684558e749b3d6";

/// SHA-256 of the canonical large-tier (depth=11) VK, hex-encoded.
pub const VK_SHA256_HEX_LARGE: &str =
    "36e96f9bf3b834f81c73a2d402b33ef8c32bc01fe47cf6ee66978e29ab0d5849";

/// Look up the pinned SHA-256 hex digest for a given tier depth.
/// Returns `None` for any depth other than 5/8/11.
pub fn pinned_vk_sha256_hex(depth: usize) -> Option<&'static str> {
    match depth {
        5 => Some(VK_SHA256_HEX_SMALL),
        8 => Some(VK_SHA256_HEX_MEDIUM),
        11 => Some(VK_SHA256_HEX_LARGE),
        _ => None,
    }
}

/// Deterministic canonical witness for tier `(depth)`. The hash inputs
/// depend only on `depth`, so the circuit shape and resulting VK
/// depend only on `depth`.
///
/// Used as the canonical fixture for VK preprocessing and as the
/// cross-platform fingerprint anchor; non-Rust platforms reproducing
/// the canonical witness must match the same `(secret_keys,
/// prover_index, epoch, salt)` quadruple shown below.
pub fn build_canonical_membership_witness(depth: usize) -> MembershipWitness {
    // Deterministic secret-key set: 1, 2, ..., 8.
    let secret_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
    let prover_index = 3usize;
    let epoch: u64 = 1234;
    let salt: [u8; 32] = [0xEE; 32];

    // Native v0.5 leaf + tree build (matches what the gadget computes
    // in-circuit).
    let leaves: Vec<Fr> = secret_keys.iter().map(poseidon_hash_one_v05).collect();
    let num_leaves = 1usize << depth;
    let mut nodes = vec![Fr::from(0u64); 2 * num_leaves];
    for (i, leaf) in leaves.iter().enumerate() {
        nodes[num_leaves + i] = *leaf;
    }
    for i in (1..num_leaves).rev() {
        nodes[i] = poseidon_hash_two_v05(&nodes[2 * i], &nodes[2 * i + 1]);
    }
    let root = nodes[1];

    let mut path = Vec::with_capacity(depth);
    let mut cur = num_leaves + prover_index;
    for _ in 0..depth {
        let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
        path.push(nodes[sib]);
        cur /= 2;
    }

    let salt_fr = Fr::from_le_bytes_mod_order(&salt);
    let inner = poseidon_hash_two_v05(&root, &Fr::from(epoch));
    let commitment = poseidon_hash_two_v05(&inner, &salt_fr);

    MembershipWitness {
        commitment,
        epoch,
        secret_key: secret_keys[prover_index],
        poseidon_root: root,
        salt,
        merkle_path: path,
        leaf_index: prover_index,
        depth,
    }
}

/// Errors raised by the baker. Disjoint from `jf_plonk::PlonkError` so
/// callers can distinguish "invalid input" from "preprocess failed".
#[derive(Debug)]
pub enum BakeError {
    /// `depth` is outside the supported tier set (5, 8, 11).
    UnsupportedDepth(usize),
    /// `synthesize_membership` rejected the canonical witness — should
    /// not happen for the supported depths; indicates a code change
    /// broke the canonical-witness invariant.
    Synthesize(jf_relation::CircuitError),
    /// `preprocess` failed; usually `IndexTooLarge` if the SRS is too
    /// small for the circuit.
    Preprocess(jf_plonk::errors::PlonkError),
    /// VK serialisation failed; arkworks `CanonicalSerialize` raised.
    Serialize(ark_serialize_v05::SerializationError),
}

impl core::fmt::Display for BakeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedDepth(d) => write!(f, "unsupported depth {d} (expected 5, 8, or 11)"),
            Self::Synthesize(e) => write!(f, "synthesize_membership failed: {e:?}"),
            Self::Preprocess(e) => write!(f, "preprocess failed: {e:?}"),
            Self::Serialize(e) => write!(f, "VK serialise failed: {e:?}"),
        }
    }
}

impl std::error::Error for BakeError {}

/// Build the canonical membership circuit for `depth`, run jf-plonk's
/// preprocessing against the embedded EF KZG SRS, and return the
/// arkworks-uncompressed verifying-key bytes.
///
/// Output is deterministic: the canonical witness is bit-deterministic,
/// the SRS is content-pinned, and `preprocess` is `(circuit, srs)`-
/// deterministic (proven by `prover::plonk::tests::preprocess_is_deterministic`).
/// Two invocations with the same `depth` therefore produce byte-identical
/// output.
///
/// Cross-check the SHA-256 of the returned bytes against
/// `pinned_vk_sha256_hex(depth)` — that comparison is the single anchor
/// shared by the cross-platform tests, the on-chain VK pin, and any
/// non-Rust prover.
pub fn bake_membership_vk(depth: usize) -> Result<Vec<u8>, BakeError> {
    if pinned_vk_sha256_hex(depth).is_none() {
        return Err(BakeError::UnsupportedDepth(depth));
    }

    let witness = build_canonical_membership_witness(depth);
    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_membership(&mut circuit, &witness).map_err(BakeError::Synthesize)?;
    circuit
        .finalize_for_arithmetization()
        .map_err(BakeError::Synthesize)?;

    let keys = plonk::preprocess(&circuit).map_err(BakeError::Preprocess)?;

    let mut vk_bytes = Vec::new();
    keys.vk
        .serialize_uncompressed(&mut vk_bytes)
        .map_err(BakeError::Serialize)?;
    Ok(vk_bytes)
}

/// SHA-256 of `bytes`, hex-encoded (lowercase). Format-compatible with
/// `pinned_vk_sha256_hex`.
pub fn vk_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each tier's bake produces the pinned SHA-256. This is the load-
    /// bearing test for the cross-platform anchor: if it fails, either
    /// (a) `bake_membership_vk` drifted, (b) the canonical witness
    /// changed, or (c) jf-plonk / arkworks shifted byte layout.
    #[test]
    fn bake_membership_vk_matches_pinned_for_all_tiers() {
        for &depth in &[5usize, 8, 11] {
            let bytes = bake_membership_vk(depth)
                .unwrap_or_else(|e| panic!("bake_membership_vk(depth={depth}) failed: {e}"));
            let computed = vk_sha256_hex(&bytes);
            let pinned = pinned_vk_sha256_hex(depth).unwrap();
            assert_eq!(
                computed, pinned,
                "bake_membership_vk(depth={depth}) drifted from pinned SHA-256. \
                 Either the circuit shape changed (audit the diff) or the canonical \
                 witness changed (update both VK_SHA256_HEX_* in baker.rs and \
                 docs/cross-platform-test-vectors.json)."
            );
        }
    }

    /// Determinism: re-baking the same tier produces byte-identical
    /// output. Cheap regression check that nothing in the preprocessing
    /// path picked up RNG or wall-clock state.
    #[test]
    fn bake_membership_vk_is_deterministic() {
        let a = bake_membership_vk(5).expect("first bake");
        let b = bake_membership_vk(5).expect("second bake");
        assert_eq!(a, b, "bake output is non-deterministic");
    }

    /// Unsupported depths are rejected up-front, before doing any
    /// preprocessing work.
    #[test]
    fn bake_membership_vk_rejects_unsupported_depth() {
        match bake_membership_vk(7) {
            Err(BakeError::UnsupportedDepth(7)) => {}
            other => panic!("expected UnsupportedDepth(7), got {other:?}"),
        }
    }
}
