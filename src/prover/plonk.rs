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
//! - `verify(vk, public_inputs, proof) -> bool` — TurboPlonk verify
//!   with the same SolidityTranscript (keccak256-based) the Soroban
//!   verifier will reproduce in Phase C.
//!
//! Generic over any `Circuit + Arithmetization` instance; when
//! `src/circuit/plonk/membership.rs` lands (B.3 follow-up) it plugs in
//! here without changes to this module.

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
    PlonkKzgSnark::<Bls12_381>::prove::<_, _, SolidityTranscript>(rng, circuit, pk, None)
}

/// Verify a TurboPlonk proof.
///
/// Returns `true` on success, `false` on any verification failure
/// (mismatched public inputs, malformed proof, transcript divergence, etc.).
/// The error type from jf-plonk is collapsed — callers that need the
/// specific failure should call into `PlonkKzgSnark::verify` directly.
pub fn verify(
    vk: &VerifyingKey<Bls12_381>,
    public_inputs: &[Fr],
    proof: &Proof<Bls12_381>,
) -> bool {
    PlonkKzgSnark::<Bls12_381>::verify::<SolidityTranscript>(vk, public_inputs, proof, None).is_ok()
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

    #[test]
    fn prove_then_verify_round_trip_on_trivial_circuit() {
        let circuit = build_square_circuit(7);

        // Deterministic RNG so the test is repeatable.
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);

        let keys = preprocess(&circuit).expect("preprocess");
        let proof = prove(&mut rng, &keys.pk, &circuit).expect("prove");

        // y = 49 — the public input the verifier checks against.
        let y = Fr::from(49u64);
        assert!(
            verify(&keys.vk, &[y], &proof),
            "verifier rejected a valid proof"
        );
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
            !verify(&keys.vk, &[wrong_y], &proof),
            "verifier accepted a proof against the wrong public input"
        );
    }

    #[test]
    fn preprocess_is_deterministic() {
        // Two builds of the same circuit produce byte-identical
        // verifying keys. This is the property that lets us bake the VK
        // into Soroban contract bytecode in Phase C — same source, same VK
        // bytes, every clean build.
        let c1 = build_square_circuit(7);
        let c2 = build_square_circuit(7);

        let k1 = preprocess(&c1).expect("preprocess 1");
        let k2 = preprocess(&c2).expect("preprocess 2");

        // Compare via canonical-serialise — the only API jf-plonk's keys
        // expose for byte-level equality.
        use ark_serialize_v05::CanonicalSerialize;
        let mut b1 = Vec::new();
        let mut b2 = Vec::new();
        k1.vk.serialize_uncompressed(&mut b1).unwrap();
        k2.vk.serialize_uncompressed(&mut b2).unwrap();
        assert_eq!(b1, b2, "preprocess is non-deterministic — VK bytes diverge");
    }
}
