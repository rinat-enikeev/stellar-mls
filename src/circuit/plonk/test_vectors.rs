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

/// Cross-platform-anchor test: VK fingerprints must match the pinned
/// values. Diagnostic info (gate count, public inputs) is logged via
/// `eprintln!` for visibility under `--nocapture`; the assertion is
/// the load-bearing part.
///
/// To bootstrap a fresh fingerprint set (after a deliberate circuit
/// change): set the pinned constants to the dummy literal `""`,
/// run `cargo test … verify_plonk_membership_vk_fingerprints
/// -- --nocapture`, and copy the printed `vk_sha256=…` values into
/// both `PINNED_VK_SHA256_*` above and
/// `docs/cross-platform-test-vectors.json`.
#[test]
fn verify_plonk_membership_vk_fingerprints() {
    let cases = [
        (5usize, "small", PINNED_VK_SHA256_SMALL),
        (8, "medium", PINNED_VK_SHA256_MEDIUM),
        (11, "large", PINNED_VK_SHA256_LARGE),
    ];
    for (depth, tier, pinned) in cases {
        let (computed, n_gates, pub_inputs) = fingerprint(depth);
        eprintln!(
            "[plonk-vk-fingerprint] depth={depth:>2} ({tier:>6}): \
             gates={n_gates}, vk_sha256={computed}"
        );
        eprintln!("  public_inputs[0] (commitment) = {}", pub_inputs[0]);
        eprintln!("  public_inputs[1] (epoch)      = {}", pub_inputs[1]);
        assert_eq!(
            computed, pinned,
            "VK SHA-256 for depth={depth} ({tier}) drifted from the pinned canonical \
             value. Either the circuit/SRS changed (audit the diff) or the canonical \
             witness changed (update both PINNED_VK_SHA256_* and \
             docs/cross-platform-test-vectors.json)."
        );
    }
}

/// Pinned proof-bytes invariant.
///
/// `Proof<Bls12_381>` from jf-plonk serialises to a **fixed byte
/// length regardless of tier** because the proof shape (number of
/// commitments, evaluations, opening proofs) is determined by the
/// TurboPlonk circuit *structure* — same number of wire types and
/// selectors across all three membership tiers. The actual circuit
/// size (depth=5, 8, 11) only affects the SRS-degree and prover
/// time, not the proof byte count.
///
/// Empirically (Plookup disabled, no zk_blinding overrides):
///
///     uncompressed = 1601 bytes
///     compressed   = 977  bytes
///
/// Both values are pinned by the
/// `canonical_proof_serialised_byte_length_per_tier` test below and
/// are the wire-format invariant Phase C's Soroban verifier consumes.
/// If either drifts, the verifier's PlonkProof struct shape and
/// every cross-platform fixture must be revisited.
const PROOF_UNCOMPRESSED_LEN: usize = 1601;
const PROOF_COMPRESSED_LEN: usize = 977;

#[test]
fn canonical_proof_serialised_byte_length_per_tier() {
    use ark_serialize_v05::CanonicalSerialize;
    use rand_chacha::rand_core::SeedableRng;

    for &depth in &[5usize, 8, 11] {
        let witness = build_canonical_witness(depth);
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_membership(&mut circuit, &witness).expect("synthesize");
        circuit.finalize_for_arithmetization().expect("finalize");

        let keys = plonk::preprocess(&circuit).expect("preprocess");
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let proof = plonk::prove(&mut rng, &keys.pk, &circuit).expect("prove");

        let mut uncompressed = Vec::new();
        proof
            .serialize_uncompressed(&mut uncompressed)
            .expect("serialize uncompressed");
        let mut compressed = Vec::new();
        proof
            .serialize_compressed(&mut compressed)
            .expect("serialize compressed");

        let tier = match depth {
            5 => "small",
            8 => "medium",
            11 => "large",
            _ => "?",
        };
        eprintln!(
            "[plonk-proof-bytes] depth={depth:>2} ({tier:>6}): \
             uncompressed={} bytes, compressed={} bytes",
            uncompressed.len(),
            compressed.len()
        );
        assert_eq!(
            uncompressed.len(),
            PROOF_UNCOMPRESSED_LEN,
            "proof uncompressed length drifted at depth={depth} \
             (was {PROOF_UNCOMPRESSED_LEN}, now {} — review every \
             downstream consumer)",
            uncompressed.len()
        );
        assert_eq!(
            compressed.len(),
            PROOF_COMPRESSED_LEN,
            "proof compressed length drifted at depth={depth}"
        );

        // Round-trip via deserialize_uncompressed — sanity check that
        // the serialised bytes can be parsed back.
        use ark_bls12_381_v05::Bls12_381;
        use ark_serialize_v05::CanonicalDeserialize;
        use jf_plonk::proof_system::structs::Proof;
        let _round_trip: Proof<Bls12_381> =
            Proof::deserialize_uncompressed(&uncompressed[..])
                .expect("proof bytes round-trip via CanonicalDeserialize");
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

        // Negative path 1: tampered commitment — catches binding-constraint
        // weakening at the verifier.
        let wrong_commitment = vec![witness.commitment + Fr::from(1u64), Fr::from(witness.epoch)];
        assert!(
            plonk::verify(&keys.vk, &wrong_commitment, &proof).is_err(),
            "verifier accepted depth={depth} membership proof against wrong commitment"
        );

        // Negative path 2: tampered epoch — catches a public-input ordering
        // swap that the commitment-only tamper wouldn't detect (a swap could
        // accidentally pass if both inputs ended up at the same value, but
        // changing only the epoch position to a value that's never the
        // commitment forces the order-sensitive check).
        let wrong_epoch = vec![witness.commitment, Fr::from(witness.epoch + 1)];
        assert!(
            plonk::verify(&keys.vk, &wrong_epoch, &proof).is_err(),
            "verifier accepted depth={depth} membership proof against wrong epoch"
        );
    }
}
