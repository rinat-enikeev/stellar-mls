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
//! The canonical-witness builder, the pinned VK SHA-256 constants, and
//! the bake helper itself live in [`super::baker`] and are used both
//! here (for the cross-platform fingerprint test) and by the
//! `bake-vk` CLI binary that produces the on-chain VK byte files.
//!
//! `docs/cross-platform-test-vectors.json` carries the same
//! fingerprints + canonical witnesses so non-Rust platforms can
//! reproduce them.

#![cfg(feature = "plonk")]
#![cfg(test)]

use ark_bls12_381_v05::Fr;
use ark_serialize_v05::CanonicalSerialize;
use jf_relation::PlonkCircuit;

use crate::circuit::plonk::baker::{
    bake_membership_vk, build_canonical_membership_witness, pinned_vk_sha256_hex, vk_sha256_hex,
};
use crate::circuit::plonk::membership::synthesize_membership;
use crate::prover::plonk;

/// Cross-platform-anchor test: VK fingerprints must match the pinned
/// values. Diagnostic info (gate count, public inputs) is logged via
/// `eprintln!` for visibility under `--nocapture`; the assertion is
/// the load-bearing part.
///
/// To bootstrap a fresh fingerprint set (after a deliberate circuit
/// change): set the pinned constants in `baker.rs` to the dummy
/// literal `""`, run `cargo test … verify_plonk_membership_vk_fingerprints
/// -- --nocapture`, and copy the printed `vk_sha256=…` values into
/// both `baker::VK_SHA256_HEX_*` and
/// `docs/cross-platform-test-vectors.json`.
#[test]
fn verify_plonk_membership_vk_fingerprints() {
    for &depth in &[5usize, 8, 11] {
        let tier = match depth {
            5 => "small",
            8 => "medium",
            11 => "large",
            _ => unreachable!(),
        };
        let vk_bytes = bake_membership_vk(depth)
            .unwrap_or_else(|e| panic!("bake_membership_vk(depth={depth}) failed: {e}"));
        let computed = vk_sha256_hex(&vk_bytes);
        let pinned = pinned_vk_sha256_hex(depth).expect("pinned constant for supported depth");

        let witness = build_canonical_membership_witness(depth);
        eprintln!(
            "[plonk-vk-fingerprint] depth={depth:>2} ({tier:>6}): \
             vk_bytes={} bytes, vk_sha256={computed}",
            vk_bytes.len()
        );
        eprintln!("  public_inputs[0] (commitment) = {}", witness.commitment);
        eprintln!("  public_inputs[1] (epoch)      = {}", Fr::from(witness.epoch));

        assert_eq!(
            computed, pinned,
            "VK SHA-256 for depth={depth} ({tier}) drifted from the pinned canonical \
             value. Either the circuit/SRS changed (audit the diff) or the canonical \
             witness changed (update both VK_SHA256_HEX_* in baker.rs and \
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
    use rand_chacha::rand_core::SeedableRng;

    for &depth in &[5usize, 8, 11] {
        let witness = build_canonical_membership_witness(depth);
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
            "uncompressed proof length drifted at depth={depth}"
        );
        assert_eq!(
            compressed.len(),
            PROOF_COMPRESSED_LEN,
            "compressed proof length drifted at depth={depth}"
        );

        // Sanity check that the serialised bytes parse back without
        // error. This is **not** a semantic round-trip — `Proof` does
        // not derive `PartialEq`, and we don't re-verify the parsed
        // proof here. Phase C.1's byte-level parser
        // (`super::proof_format`) handles the stronger oracle test.
        use ark_bls12_381_v05::Bls12_381;
        use ark_serialize_v05::CanonicalDeserialize;
        use jf_plonk::proof_system::structs::Proof;
        let _parsed: Proof<Bls12_381> =
            Proof::deserialize_uncompressed(&uncompressed[..])
                .expect("uncompressed proof bytes parse via CanonicalDeserialize");
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
        let witness = build_canonical_membership_witness(depth);
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
