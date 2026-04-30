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
use crate::circuit::plonk::update::{synthesize_update, UpdateWitness};
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

/// SHA-256 of the canonical small-tier (depth=5) **update** VK,
/// hex-encoded. Anchors the update circuit's preprocessing output the
/// same way `VK_SHA256_HEX_SMALL` anchors the membership circuit's.
pub const UPDATE_VK_SHA256_HEX_SMALL: &str =
    "61dbe507fe10fafa94f060d7f24675091c109b6229242051d887a4b27f9a634a";

/// SHA-256 of the canonical medium-tier (depth=8) **update** VK.
pub const UPDATE_VK_SHA256_HEX_MEDIUM: &str =
    "0e15eb665d2b978e41b5f555cdb765789cb7dc6046cf0236d0d0bdf30a2419ee";

/// SHA-256 of the canonical large-tier (depth=11) **update** VK.
pub const UPDATE_VK_SHA256_HEX_LARGE: &str =
    "73ae78e1d42d161ffdd44f27977f27c084c88d460f926eea5f6ca1d06d851245";

/// Look up the pinned SHA-256 hex digest for a given **membership** tier.
/// Returns `None` for any depth other than 5/8/11.
pub fn pinned_vk_sha256_hex(depth: usize) -> Option<&'static str> {
    match depth {
        5 => Some(VK_SHA256_HEX_SMALL),
        8 => Some(VK_SHA256_HEX_MEDIUM),
        11 => Some(VK_SHA256_HEX_LARGE),
        _ => None,
    }
}

/// Look up the pinned SHA-256 hex digest for a given **update** tier.
/// Returns `None` for any depth other than 5/8/11.
pub fn pinned_update_vk_sha256_hex(depth: usize) -> Option<&'static str> {
    match depth {
        5 => Some(UPDATE_VK_SHA256_HEX_SMALL),
        8 => Some(UPDATE_VK_SHA256_HEX_MEDIUM),
        11 => Some(UPDATE_VK_SHA256_HEX_LARGE),
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

/// Deterministic canonical witness for the **update** circuit at tier
/// `(depth)`. Reuses the membership canonical's secret keys, prover
/// index, old epoch, and old salt, and adds a fresh `salt_new` so the
/// transition `(salt_old → salt_new)` is well-formed without a roster
/// change. The new tree equals the old tree by construction; the
/// circuit doesn't constrain new-tree membership, so this is a valid
/// canonical witness.
///
/// **Reusing `root` for both old and new is a property of the canonical
/// fixture, not a property of the circuit.** The update circuit binds
/// `c_new` to `(poseidon_root_new, epoch_new, salt_new)` but never
/// proves the prover knows a leaf in `poseidon_root_new` — see the
/// "Security model" section in `circuit::plonk::update` for full
/// detail. Production callers can supply any `poseidon_root_new`;
/// downstream consumers must not interpret it as an authenticated
/// roster.
///
/// Hash inputs depend only on `depth`, so the resulting VK depends
/// only on `depth`.
pub fn build_canonical_update_witness(depth: usize) -> UpdateWitness {
    // Match `build_canonical_membership_witness` exactly so canonical
    // proofs at the two circuits share the same `(secret_key, root,
    // epoch_old, salt_old)` quadruple.
    let secret_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
    let prover_index = 3usize;
    let epoch_old: u64 = 1234;
    let salt_old: [u8; 32] = [0xEE; 32];
    let salt_new: [u8; 32] = [0xFF; 32];

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

    // Compute c_old, c_new natively.
    let salt_old_fr = Fr::from_le_bytes_mod_order(&salt_old);
    let salt_new_fr = Fr::from_le_bytes_mod_order(&salt_new);
    let c_old = poseidon_hash_two_v05(
        &poseidon_hash_two_v05(&root, &Fr::from(epoch_old)),
        &salt_old_fr,
    );
    let c_new = poseidon_hash_two_v05(
        &poseidon_hash_two_v05(&root, &Fr::from(epoch_old + 1)),
        &salt_new_fr,
    );

    UpdateWitness {
        c_old,
        epoch_old,
        c_new,
        secret_key: secret_keys[prover_index],
        poseidon_root_old: root,
        salt_old,
        merkle_path_old: path,
        leaf_index_old: prover_index,
        // No roster change — old root doubles as new root for the
        // canonical witness. Production usage will pass a different
        // tree; the circuit doesn't constrain new-tree membership.
        poseidon_root_new: root,
        salt_new,
        depth,
    }
}

/// Errors raised by the baker. Disjoint from `jf_plonk::PlonkError` so
/// callers can distinguish "invalid input" from "preprocess failed".
#[derive(Debug)]
pub enum BakeError {
    /// `depth` is outside the supported tier set (5, 8, 11).
    UnsupportedDepth(usize),
    /// `synthesize_membership` / `synthesize_update` rejected the
    /// canonical witness — should not happen for the supported
    /// depths; indicates a code change broke the canonical-witness
    /// invariant.
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

/// Build the canonical update circuit for `depth`, run jf-plonk's
/// preprocessing against the embedded EF KZG SRS, and return the
/// arkworks-uncompressed verifying-key bytes.
///
/// Same determinism guarantees as `bake_membership_vk`. Cross-check the
/// SHA-256 against `pinned_update_vk_sha256_hex(depth)`.
pub fn bake_update_vk(depth: usize) -> Result<Vec<u8>, BakeError> {
    if pinned_update_vk_sha256_hex(depth).is_none() {
        return Err(BakeError::UnsupportedDepth(depth));
    }

    let witness = build_canonical_update_witness(depth);
    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_update(&mut circuit, &witness).map_err(BakeError::Synthesize)?;
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
/// `pinned_vk_sha256_hex` / `pinned_update_vk_sha256_hex`.
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

    /// Update-circuit equivalent of
    /// `bake_membership_vk_matches_pinned_for_all_tiers`. Anchors the
    /// update VK shape across builds and platforms — when the pinned
    /// hashes need to be regenerated (legitimate circuit change), this
    /// test fails first with the actual SHA-256s for all tiers, which
    /// can then be pasted into `UPDATE_VK_SHA256_HEX_*`.
    #[test]
    fn bake_update_vk_matches_pinned_for_all_tiers() {
        // Compute all tiers first so a single test run produces every
        // SHA-256 (rather than panicking on the first mismatch and
        // hiding the rest).
        let mut mismatches = Vec::new();
        for &depth in &[5usize, 8, 11] {
            let bytes = bake_update_vk(depth)
                .unwrap_or_else(|e| panic!("bake_update_vk(depth={depth}) failed: {e}"));
            let computed = vk_sha256_hex(&bytes);
            let pinned = pinned_update_vk_sha256_hex(depth).unwrap();
            if computed != pinned {
                mismatches.push(format!(
                    "depth={depth}: computed={computed}, pinned={pinned}"
                ));
            }
        }
        assert!(
            mismatches.is_empty(),
            "bake_update_vk drifted from pinned SHA-256s. Either the \
             update circuit shape changed (audit the diff) or the \
             canonical update witness changed (update both \
             UPDATE_VK_SHA256_HEX_* in baker.rs and any cross-platform \
             anchor):\n  {}",
            mismatches.join("\n  ")
        );
    }

    /// Update bake is deterministic across calls.
    #[test]
    fn bake_update_vk_is_deterministic() {
        let a = bake_update_vk(5).expect("first bake");
        let b = bake_update_vk(5).expect("second bake");
        assert_eq!(a, b, "update bake output is non-deterministic");
    }

    /// Unsupported depth is rejected up-front.
    #[test]
    fn bake_update_vk_rejects_unsupported_depth() {
        match bake_update_vk(7) {
            Err(BakeError::UnsupportedDepth(7)) => {}
            other => panic!("expected UnsupportedDepth(7), got {other:?}"),
        }
    }
}
