//! TurboPlonk prover backed by jf-plonk over the embedded EF KZG SRS.
//!
//! Phase B.4 (membership-only prover) per
//! `docs/implementation-plan-fflonk-migration.md`.
//!
//! This module exposes the three primitives that wrap
//! `jf_plonk::PlonkKzgSnark<Bls12_381>`:
//!
//! - `srs()` — lazy-loads the embedded EF KZG SRS.
//! - `preprocess(circuit) -> CircuitKeys` — deterministic per-circuit
//!   preprocessing into prover key + verifying key.
//! - `prove(rng, pk, circuit) -> Proof` — TurboPlonk prove.
//! - `verify(vk, public_inputs, proof) -> Result<(), PlonkError>` —
//!   TurboPlonk verify with the same SolidityTranscript (keccak256-
//!   based) the Soroban verifier will reproduce in Phase C. Returns a
//!   structured error so callers can distinguish failure modes
//!   (forged proof vs. malformed VK vs. transcript divergence).
//!
//! `preprocess` and `prove` take a concrete `&PlonkCircuit<Fr>` rather
//! than a generic `C: Circuit + Arithmetization<Fr>`. The renamed-
//! package alias for ark-bls12-381 v0.5 confuses rustc's trait-bound
//! resolution when the call site is generic; making the argument
//! type concrete sidesteps that. All circuits in this codebase build
//! on `PlonkCircuit<Fr>` directly so no expressiveness is lost — see
//! the longer rationale at the `preprocess` doc-comment below.
//!
//! ## Two `Fr`s reachable from `crate::prover`
//!
//! The legacy Groth16 path in `crate::prover` (the parent module)
//! uses **arkworks v0.4** `ark_bls12_381::Fr`. This module uses
//! **arkworks v0.5** `ark_bls12_381_v05::Fr` (renamed in
//! `Cargo.toml`) because that is what jf-plonk requires.
//!
//! When wiring a circuit through this module, always use the
//! `Fr` / `ScalarField` re-exported here. Mixing the two `Fr`s
//! produces "two types coming from two different versions of the
//! same crate" errors at the trait-bound level — the same class of
//! issue PR #167 already documented for the jf-* deps.
//!
//! Phase B.4 splits the prover module wholesale onto v0.5 and the
//! legacy v0.4 path is dropped in Phase E; until then both `Fr`s
//! coexist.

#![cfg(feature = "plonk")]

use std::sync::OnceLock;

use ark_bls12_381_v05::{Bls12_381, Fr};
use jf_pcs::prelude::UnivariateUniversalParams;
use jf_plonk::proof_system::structs::{Proof, ProvingKey, VerifyingKey};
use jf_plonk::proof_system::{PlonkKzgSnark, UniversalSNARK};
use jf_plonk::transcript::SolidityTranscript;
use jf_relation::PlonkCircuit;

/// Re-export of BLS12-381's scalar field for downstream callers.
pub use ark_bls12_381_v05::Fr as ScalarField;

use crate::prover::srs::load_ef_kzg_srs;

/// Process-wide cache of the deserialised universal SRS. The deserialiser is
/// non-trivial (4096 G1 + 65 G2 affine points) so we want to pay it once.
static CACHED_SRS: OnceLock<UnivariateUniversalParams<Bls12_381>> = OnceLock::new();

/// Returns a reference to the cached universal SRS, deserialising on first
/// call. Panics on deserialisation failure — at that point the embedded bytes
/// are corrupt and the build-time hash check should already have caught it.
pub fn srs() -> &'static UnivariateUniversalParams<Bls12_381> {
    CACHED_SRS.get_or_init(|| {
        load_ef_kzg_srs().expect(
            "load_ef_kzg_srs failed — embedded SRS bytes are corrupt; \
             the build-time SHA-256 assertion in build.rs should have \
             caught this. Did you build with `cargo build --features \
             plonk` against a sanitised checkout?",
        )
    })
}

/// Bundled prover and verifier key for a single circuit.
///
/// `preprocess` is deterministic in `(circuit, srs)`, so the same circuit
/// always yields the same `CircuitKeys` regardless of process state.
pub struct CircuitKeys {
    pub pk: ProvingKey<Bls12_381>,
    pub vk: VerifyingKey<Bls12_381>,
}

/// Deterministic per-circuit preprocessing.
///
/// Build the (prover-key, verifier-key) pair from the embedded SRS and the
/// circuit's structure (gate layout + permutation polynomials). The result
/// is independent of any witness assignment — the same circuit produces the
/// same keys every time.
///
/// The `CircuitKeys` should be cached per circuit per process; preprocessing
/// is `O(n log n)` in the number of gates and need only run once.
///
/// Concrete `PlonkCircuit<Fr>` rather than a generic `C: Arithmetization<Fr>`
/// — the renamed-package alias for `ark-bls12-381` 0.5 confuses rustc's
/// trait-bound resolution when generic. All circuits in this codebase
/// build on `PlonkCircuit<Fr>` directly so no expressiveness is lost.
pub fn preprocess(
    circuit: &PlonkCircuit<Fr>,
) -> Result<CircuitKeys, jf_plonk::errors::PlonkError> {
    let (pk, vk) = PlonkKzgSnark::<Bls12_381>::preprocess(srs(), circuit)?;
    Ok(CircuitKeys { pk, vk })
}

/// Generate a TurboPlonk proof for `circuit` against `pk`.
///
/// `circuit` carries both the gate layout and the witness assignment. The
/// `SolidityTranscript` (keccak256-based) is used so the resulting proof is
/// reproducible by the Soroban verifier in Phase C via
/// `env.crypto().keccak256()`.
pub fn prove<R>(
    rng: &mut R,
    pk: &ProvingKey<Bls12_381>,
    circuit: &PlonkCircuit<Fr>,
) -> Result<Proof<Bls12_381>, jf_plonk::errors::PlonkError>
where
    R: ark_std_v05::rand::CryptoRng + ark_std_v05::rand::RngCore,
{
    // CONTRACT (locked): `extra_transcript_init_msg = None`. The Soroban
    // verifier in Phase C MUST initialise its transcript with the same
    // empty seed — anything else (e.g. a domain-separator derived from a
    // circuit identifier) and the off-chain prover and on-chain verifier
    // diverge silently. If a future change here passes anything but
    // `None`, the same change has to land in every Soroban contract that
    // verifies these proofs in lockstep.
    PlonkKzgSnark::<Bls12_381>::prove::<_, _, SolidityTranscript>(rng, circuit, pk, None)
}

/// Verify a TurboPlonk proof.
///
/// Returns `Ok(())` on a successful verification, `Err(PlonkError)`
/// otherwise. The structured error lets the upstream Soroban-bound
/// verifier (and its callers) distinguish failure modes — a malformed
/// VK is genuinely a different bug from a forged proof, and dropping
/// that information was a real cost of the previous `bool` API.
///
/// Callers that just want a boolean can use `verify(...).is_ok()`.
pub fn verify(
    vk: &VerifyingKey<Bls12_381>,
    public_inputs: &[Fr],
    proof: &Proof<Bls12_381>,
) -> Result<(), jf_plonk::errors::PlonkError> {
    // Same contract as `prove`: `extra_transcript_init_msg = None`. The
    // Soroban verifier MUST mirror this. See the comment on `prove`.
    PlonkKzgSnark::<Bls12_381>::verify::<SolidityTranscript>(vk, public_inputs, proof, None)
}

#[cfg(test)]
mod tests {
    //! End-to-end prove → verify round trip on a trivial circuit, exercising
    //! the full PlonkKzgSnark<Bls12_381> pipeline against our embedded EF
    //! KZG SRS. Validates that:
    //!
    //! - the embedded SRS deserialises into a struct jf-pcs accepts,
    //! - `preprocess(circuit)` succeeds for a real circuit,
    //! - `prove` produces a proof,
    //! - `verify` accepts the proof against the matching public inputs and
    //!   rejects against tampered ones.
    //!
    //! The circuit proves "I know `x` such that `x*x = y`", with `x` a
    //! witness and `y` the only public input. This is the smallest non-trivial
    //! statement that uses jf-relation's gate API — exactly the surface every
    //! later PR touches when porting the membership / update / democracy
    //! circuits.
    use super::*;
    use jf_relation::{Circuit, PlonkCircuit};
    use rand_chacha::rand_core::SeedableRng;

    /// Build a `PlonkCircuit` enforcing `witness * witness == public_input`,
    /// with `witness` and `public_input = witness * witness` baked in.
    fn build_square_circuit(secret: u64) -> PlonkCircuit<Fr> {
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();

        // Allocate the public input first (preserves wire-format ordering).
        let y = Fr::from(secret) * Fr::from(secret);
        let pub_var = circuit.create_public_variable(y).expect("public variable");

        // Allocate the witness.
        let witness_var = circuit
            .create_variable(Fr::from(secret))
            .expect("witness variable");

        // Enforce: witness * witness == public_input
        circuit
            .mul_gate(witness_var, witness_var, pub_var)
            .expect("mul_gate");

        circuit.finalize_for_arithmetization().expect("finalize");
        circuit
    }

    /// Distinct-shape circuit for the cross-VK test: proves
    /// `witness^3 == public_input`. Different gate count → different
    /// VK from `build_square_circuit`.
    fn build_cube_circuit(secret: u64) -> PlonkCircuit<Fr> {
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        let y = Fr::from(secret) * Fr::from(secret) * Fr::from(secret);
        let pub_var = circuit.create_public_variable(y).expect("public variable");
        let witness_var = circuit
            .create_variable(Fr::from(secret))
            .expect("witness variable");
        // tmp = witness * witness
        let tmp = circuit.mul(witness_var, witness_var).expect("mul");
        // pub_var = tmp * witness
        circuit
            .mul_gate(tmp, witness_var, pub_var)
            .expect("mul_gate");
        circuit.finalize_for_arithmetization().expect("finalize");
        circuit
    }

    #[test]
    fn prove_then_verify_round_trip_on_trivial_circuit() {
        let circuit = build_square_circuit(7);

        // Deterministic RNG so the test is repeatable.
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);

        let keys = preprocess(&circuit).expect("preprocess");
        let proof = prove(&mut rng, &keys.pk, &circuit).expect("prove");

        // y = 49 — the public input the verifier checks against.
        let y = Fr::from(49u64);
        verify(&keys.vk, &[y], &proof).expect("verifier rejected a valid proof");
    }

    #[test]
    fn verifier_rejects_tampered_public_input() {
        let circuit = build_square_circuit(7);
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);

        let keys = preprocess(&circuit).expect("preprocess");
        let proof = prove(&mut rng, &keys.pk, &circuit).expect("prove");

        // Change the public input to a different value; verification must fail.
        let wrong_y = Fr::from(50u64);
        assert!(
            verify(&keys.vk, &[wrong_y], &proof).is_err(),
            "verifier accepted a proof against the wrong public input"
        );
    }

    #[test]
    fn preprocess_is_deterministic() {
        // Two builds of the same circuit produce byte-identical
        // verifying keys AND byte-identical proving keys. The VK
        // determinism is what lets us bake the VK into Soroban
        // contract bytecode in Phase C; PK determinism is what lets a
        // mobile client cache its preprocessed proving key once and
        // re-use it across reboots without re-running preprocess.
        let c1 = build_square_circuit(7);
        let c2 = build_square_circuit(7);

        let k1 = preprocess(&c1).expect("preprocess 1");
        let k2 = preprocess(&c2).expect("preprocess 2");

        // Compare via canonical-serialise — the only API jf-plonk's keys
        // expose for byte-level equality.
        use ark_serialize_v05::CanonicalSerialize;
        let mut vk1 = Vec::new();
        let mut vk2 = Vec::new();
        k1.vk.serialize_uncompressed(&mut vk1).unwrap();
        k2.vk.serialize_uncompressed(&mut vk2).unwrap();
        assert_eq!(vk1, vk2, "preprocess is non-deterministic — VK bytes diverge");

        let mut pk1 = Vec::new();
        let mut pk2 = Vec::new();
        k1.pk.serialize_uncompressed(&mut pk1).unwrap();
        k2.pk.serialize_uncompressed(&mut pk2).unwrap();
        assert_eq!(pk1, pk2, "preprocess is non-deterministic — PK bytes diverge");
    }

    /// A flipped byte inside the proof must fail verification. Catches
    /// the class of bugs where the verifier accepts any well-formed
    /// proof regardless of pairing-equation satisfaction.
    #[test]
    fn verifier_rejects_tampered_proof_bytes() {
        use ark_bls12_381_v05::Bls12_381;
        use ark_serialize_v05::{CanonicalDeserialize, CanonicalSerialize};

        let circuit = build_square_circuit(7);
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = preprocess(&circuit).expect("preprocess");
        let proof = prove(&mut rng, &keys.pk, &circuit).expect("prove");

        let mut bytes = Vec::new();
        proof.serialize_uncompressed(&mut bytes).unwrap();
        let original = bytes.clone();
        let y = Fr::from(49u64);

        // Flip a byte at several spread-out positions. For each flip:
        //   - If `Proof::deserialize_uncompressed` fails, the tampering
        //     produced a malformed group-element header; that's a valid
        //     rejection mode and we count it but keep going.
        //   - If deserialise succeeds, run `verify` and assert it
        //     rejects (the test's main concern).
        // We require at least ONE flip to reach `verify` and produce a
        // rejection — otherwise the test is vacuous (every flip
        // shortcircuited at deserialise) and we surface that.
        let mut deserialise_fails = 0usize;
        let mut verify_rejections = 0usize;
        let flip_positions: &[usize] = &[
            // First wires_poly_comms.len() prefix (u64 LE = 5)
            0,
            // Mid-G1Affine for first wire commitment (x bytes)
            64,
            // Last G2-related byte run inside the proof body (likely
            // hits an opening_proof byte)
            bytes.len() / 2,
            // Inside the wires_evals or wire_sigma_evals region —
            // these are 32 B Fr elements that round-trip cleanly
            // through deserialise (any bit pattern reduces mod r),
            // so the verifier IS the gate that has to reject.
            bytes.len() - 200,
            bytes.len() - 100,
            bytes.len() - 33,  // last full Fr eval
        ];
        for &flip_at in flip_positions {
            let mut tampered_bytes = original.clone();
            tampered_bytes[flip_at] ^= 0x55;
            match Proof::<Bls12_381>::deserialize_uncompressed(&tampered_bytes[..]) {
                Ok(p) => {
                    assert!(
                        verify(&keys.vk, &[y], &p).is_err(),
                        "verifier accepted a proof with a flipped byte at offset {flip_at} \
                         — verifier is broken"
                    );
                    verify_rejections += 1;
                }
                Err(_) => {
                    deserialise_fails += 1;
                }
            }
        }
        assert!(
            verify_rejections > 0,
            "every byte flip short-circuited at deserialise (deserialise_fails={deserialise_fails}); \
             test is vacuous — no flip ever reached `verify`. Adjust flip_positions to land inside \
             the Fr-evaluation region (last ~340 bytes, where any bit pattern reduces mod r)."
        );
    }

    /// A proof produced for circuit A must NOT verify under the VK of
    /// circuit B. Catches a verifier that ignores the public-input /
    /// constraint structure encoded in the VK and accepts any proof
    /// for any circuit.
    ///
    /// `build_square_circuit` and `build_cube_circuit` have different
    /// gate counts and different public-input values, so their VKs
    /// are byte-different — the assertion at the start of the test
    /// fails noisily if anyone changes the helpers in a way that
    /// re-unifies the two shapes.
    #[test]
    fn verifier_rejects_proof_under_wrong_vk() {
        let circuit_a = build_square_circuit(7); // shape: x*x = y; y_a = 49
        let circuit_b = build_cube_circuit(7);   // shape: x*x*x = y; y_b = 343

        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys_a = preprocess(&circuit_a).expect("preprocess A");
        let keys_b = preprocess(&circuit_b).expect("preprocess B");

        // Sanity: VKs really do differ (otherwise the test is vacuous).
        use ark_serialize_v05::CanonicalSerialize;
        let mut vk_a_bytes = Vec::new();
        let mut vk_b_bytes = Vec::new();
        keys_a.vk.serialize_uncompressed(&mut vk_a_bytes).unwrap();
        keys_b.vk.serialize_uncompressed(&mut vk_b_bytes).unwrap();
        assert_ne!(
            vk_a_bytes, vk_b_bytes,
            "test setup invalid: VKs are identical, cross-VK rejection vacuous"
        );

        let proof_a = prove(&mut rng, &keys_a.pk, &circuit_a).expect("prove A");

        // proof_a's public input is 49; verifying it under VK_B should fail.
        let y_a = Fr::from(49u64);
        assert!(
            verify(&keys_b.vk, &[y_a], &proof_a).is_err(),
            "verifier accepted a proof from circuit A under VK from circuit B \
             — verifier is broken"
        );
    }

    /// Helper: build a circuit proving `witness = a + b` where (a, b)
    /// are public inputs allocated in fixed order. Used by the
    /// public-input-ordering test to exercise the surface that bites
    /// Soroban verifier integration in Phase C.
    fn build_sum_circuit(a: u64, b: u64, witness: u64) -> PlonkCircuit<Fr> {
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        // Allocation ORDER matters for the verifier — the public-input
        // vector passed to `verify` must be `[a, b]`, not `[b, a]`.
        let a_var = circuit.create_public_variable(Fr::from(a)).expect("a");
        let b_var = circuit.create_public_variable(Fr::from(b)).expect("b");
        let w_var = circuit.create_variable(Fr::from(witness)).expect("witness");
        // Enforce w == a + b.
        let sum = circuit.add(a_var, b_var).expect("add");
        circuit.enforce_equal(w_var, sum).expect("enforce_equal");
        circuit.finalize_for_arithmetization().expect("finalize");
        circuit
    }

    /// Public inputs are positional, not by name. Verifying with
    /// `[a, b]` (the allocation order) succeeds; verifying with
    /// `[b, a]` (swapped) fails. Catches a Phase-C verifier that
    /// silently treats the public-input vector as a bag rather than
    /// an ordered list.
    #[test]
    fn verifier_respects_public_input_ordering() {
        let (a, b) = (3u64, 7u64);
        let circuit = build_sum_circuit(a, b, a + b);
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = preprocess(&circuit).expect("preprocess");
        let proof = prove(&mut rng, &keys.pk, &circuit).expect("prove");

        // Correct order: [a, b]
        verify(&keys.vk, &[Fr::from(a), Fr::from(b)], &proof)
            .expect("verifier rejected a valid proof under correct public-input order");

        // Swapped order: [b, a] — the constraint `witness = a + b`
        // is symmetric in a and b numerically, so swapping doesn't
        // change the *value* of `a + b`. But the **VK** binds
        // public-input slot 0 to `a` and slot 1 to `b` separately
        // (each slot has its own IC commitment). So a swap with
        // distinct values `a ≠ b` produces a different challenge
        // transcript and the verifier rejects.
        //
        // (If a == b, the swap is a no-op and verification passes —
        // that's the same proof. We pick `a = 3, b = 7` to ensure
        // a != b.)
        assert_ne!(a, b, "test setup invalid: a == b makes the swap trivial");
        assert!(
            verify(&keys.vk, &[Fr::from(b), Fr::from(a)], &proof).is_err(),
            "verifier accepted a proof with swapped public inputs — \
             public-input ordering is not enforced"
        );
    }

    /// `preprocess` on a circuit that hasn't been finalized must fail
    /// — the gate count and permutation polynomials aren't yet
    /// arithmetized into the form jf-plonk consumes. Catches the bug
    /// where a caller forgets `finalize_for_arithmetization()` and
    /// preprocess produces a malformed proving key that proves
    /// vacuous proofs.
    #[test]
    fn preprocess_rejects_unfinalized_circuit() {
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        let y = Fr::from(49u64);
        let pub_var = circuit.create_public_variable(y).expect("public");
        let w_var = circuit.create_variable(Fr::from(7u64)).expect("witness");
        circuit.mul_gate(w_var, w_var, pub_var).expect("mul_gate");
        // Intentionally NOT calling finalize_for_arithmetization() —
        // preprocess must surface the unfinalized state, not silently
        // produce a broken pk/vk.

        let result = preprocess(&circuit);
        assert!(
            result.is_err(),
            "preprocess accepted an unfinalized circuit — must require finalize_for_arithmetization() first"
        );
    }
}
