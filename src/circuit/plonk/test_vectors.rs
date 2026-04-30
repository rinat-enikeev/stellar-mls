//! Cross-platform test vectors for the PLONK MembershipCircuit.
//!
//! Phase B.5 per `docs/implementation-plan-fflonk-migration.md`.
//!
//! The cross-platform anchor is the **verifier-key SHA-256**: all
//! platforms (Rust, iOS, Android) build the same circuit shape from
//! the same canonical witness, deterministically preprocess against
//! the same embedded EF KZG SRS, and must produce a byte-identical
//! `VerifyingKey<Bls12_381>`. If a port drifts — wrong gate ordering,
//! wrong public-input order, wrong SRS extraction — the VK fingerprint
//! diverges, and this test fails before any cross-platform proof is
//! ever attempted.
//!
//! Pinning proof bytes themselves is intentionally **not** done here:
//! - PLONK proofs include Fiat-Shamir blinding, so re-generation under
//!   a different RNG seed produces different bytes;
//! - jf-plonk minor-version bumps may shuffle internal byte order;
//! - the verifier accepts any well-formed proof anyway.
//!
//! VK fingerprints are stable across all of those, since `preprocess`
//! is `(circuit, srs)`-deterministic (proven by
//! `prover::plonk::tests::preprocess_is_deterministic`).
//!
//! `docs/cross-platform-test-vectors.json` carries the same
//! fingerprints + canonical witnesses so non-Rust platforms can
//! reproduce them.

#![cfg(feature = "plonk")]
#![cfg(test)]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use ark_serialize_v05::CanonicalSerialize;
use jf_relation::{Circuit, PlonkCircuit};
use sha2::{Digest, Sha256};

use crate::circuit::plonk::membership::{synthesize_membership, MembershipWitness};
use crate::circuit::plonk::poseidon::{poseidon_hash_one_v05, poseidon_hash_two_v05};
use crate::prover::plonk;

/// Deterministic canonical witness for tier `(depth)`. Caller picks
/// `prover_index` and produces a complete `MembershipWitness` whose
/// hash inputs depend only on `depth` — so the circuit shape and the
/// resulting VK depend only on `depth`.
fn build_canonical_witness(depth: usize) -> MembershipWitness {
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

/// Build the canonical circuit for a tier and return its
/// (vk_sha256_hex, num_gates_after_finalize, public_inputs).
fn fingerprint(depth: usize) -> (String, usize, Vec<Fr>) {
    let witness = build_canonical_witness(depth);
    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_membership(&mut circuit, &witness).expect("synthesize");
    circuit.finalize_for_arithmetization().expect("finalize");

    let keys = plonk::preprocess(&circuit).expect("preprocess");
    let mut vk_bytes = Vec::new();
    keys.vk.serialize_uncompressed(&mut vk_bytes).expect("serialize vk");
    let vk_sha256 = Sha256::digest(&vk_bytes);
    let vk_sha256_hex = vk_sha256.iter().map(|b| format!("{:02x}", b)).collect();

    let public_inputs = vec![witness.commitment, Fr::from(witness.epoch)];
    (vk_sha256_hex, circuit.num_gates(), public_inputs)
}

/// Pinned VK fingerprints — the cross-platform invariant.
///
/// If any of these change, either:
/// - the circuit shape changed (gate order, public-input order, gadget
///   internals) — review the diff carefully and update;
/// - the SRS changed (build.rs would also have caught the hash mismatch
///   first);
/// - jf-plonk's `preprocess` output format changed at the byte level —
///   in which case all consumers (Soroban verifier, mobile clients)
///   need a coordinated update too.
///
/// Mirror these into `docs/cross-platform-test-vectors.json` under
/// `plonk_membership_vk_fingerprints`. Non-Rust platforms compute the
/// same SHA-256 over their `VerifyingKey::serialize_uncompressed`
/// output and assert byte-equality.
const PINNED_VK_SHA256_SMALL:  &str =
    "a552b41c2e40167b74ccbec36d83cc931279d278e364cacb99a3a5ce9c26e5ab";
const PINNED_VK_SHA256_MEDIUM: &str =
    "b4f98a146dad1de3447dde3b686ac2ddf8b4cdb153ad69f407684558e749b3d6";
const PINNED_VK_SHA256_LARGE:  &str =
    "36e96f9bf3b834f81c73a2d402b33ef8c32bc01fe47cf6ee66978e29ab0d5849";

/// Generate the VK fingerprints. Runs the canonical witness through
/// preprocess and prints the values. First-run output goes both into
/// `PINNED_VK_SHA256_*` constants above and into
/// `docs/cross-platform-test-vectors.json`.
///
/// Re-runs of this test are no-ops on success once the constants are
/// pinned — see `verify_plonk_membership_vk_fingerprints` for the
/// assertion test.
#[test]
fn print_plonk_membership_vk_fingerprints() {
    for &depth in &[5usize, 8, 11] {
        let (vk_hex, n_gates, pub_inputs) = fingerprint(depth);
        let tier_name = match depth {
            5 => "small",
            8 => "medium",
            11 => "large",
            _ => "?",
        };
        eprintln!(
            "[plonk-vk-fingerprint] depth={depth:>2} ({tier_name:>6}): \
             gates={n_gates}, vk_sha256={vk_hex}"
        );
        eprintln!("  public_inputs[0] (commitment) = {}", pub_inputs[0]);
        eprintln!("  public_inputs[1] (epoch)      = {}", pub_inputs[1]);
    }
}

/// Cross-platform-anchor test: VK fingerprints must match the pinned
/// values. Skipped (with a `eprintln` note) when the constants are
/// still placeholders.
#[test]
fn verify_plonk_membership_vk_fingerprints() {
    let cases = [
        (5usize, "small", PINNED_VK_SHA256_SMALL),
        (8, "medium", PINNED_VK_SHA256_MEDIUM),
        (11, "large", PINNED_VK_SHA256_LARGE),
    ];
    for (depth, tier, pinned) in cases {
        if pinned.starts_with('<') {
            eprintln!(
                "[plonk-vk-fingerprint] depth={depth} ({tier}): pinned constant is \
                 placeholder; run `print_plonk_membership_vk_fingerprints` and copy \
                 the output."
            );
            continue;
        }
        let (computed, _, _) = fingerprint(depth);
        assert_eq!(
            computed, pinned,
            "VK SHA-256 for depth={depth} ({tier}) drifted from the pinned canonical \
             value. Either the circuit/SRS changed (audit the diff) or the canonical \
             witness changed (update both PINNED_VK_SHA256_* and \
             docs/cross-platform-test-vectors.json)."
        );
    }
}

/// End-to-end consistency: the canonical witness produces a circuit
/// that proves and verifies under PlonkKzgSnark. This is the
/// per-platform self-check (Rust here, mirrored on iOS/Android once
/// their bindings are in place).
#[test]
fn canonical_witness_proves_and_verifies_for_all_tiers() {
    use rand_chacha::rand_core::SeedableRng;

    for &depth in &[5usize, 8, 11] {
        let witness = build_canonical_witness(depth);
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_membership(&mut circuit, &witness).expect("synthesize");
        circuit.finalize_for_arithmetization().expect("finalize");

        let keys = plonk::preprocess(&circuit).expect("preprocess");
        // Deterministic seed keeps the test reproducible across runs.
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let proof = plonk::prove(&mut rng, &keys.pk, &circuit).expect("prove");

        let public_inputs = vec![witness.commitment, Fr::from(witness.epoch)];
        plonk::verify(&keys.vk, &public_inputs, &proof)
            .unwrap_or_else(|e| panic!("verifier rejected canonical depth={depth} membership proof: {e:?}"));

        let wrong = vec![witness.commitment + Fr::from(1u64), Fr::from(witness.epoch)];
        assert!(
            plonk::verify(&keys.vk, &wrong, &proof).is_err(),
            "verifier accepted depth={depth} membership proof against wrong commitment"
        );
    }
}
