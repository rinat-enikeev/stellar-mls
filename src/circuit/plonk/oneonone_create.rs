//! `OneOnOneCreateCircuit` — TurboPlonk port of the 1v1 "founding"
//! constraint.
//!
//! Differs from `MembershipCircuit` only in what the prover must
//! demonstrate at create time:
//!
//!   Public inputs:  commitment, epoch (epoch always 0 at create)
//!   Witness:        secret_key_0, secret_key_1, salt
//!
//!   Constraints:
//!     1. leaf_0 = Poseidon(secret_key_0)                  — key ownership
//!     2. leaf_1 = Poseidon(secret_key_1)                  — key ownership
//!     3. root = MerkleRoot([leaf_0, leaf_1, 0, 0, …, 0])  — exactly-2-leaves invariant
//!     4. commitment = Poseidon(Poseidon(root, epoch), salt)  — binding
//!
//! Constraint 3 is the security property: an in-circuit depth-5 tree
//! with positions 0/1 supplied by the prover and positions 2..32
//! pinned to constant zero. The prover *cannot* place additional
//! non-zero leaves because no witness slot exists for them.
//!
//! ## Tier
//!
//! Hardcoded to depth=5 (the only tier sep-oneonone supports). A
//! per-tier port would parameterise `DEPTH`; for 1v1's single-tier
//! case the constant keeps the circuit cleaner.
//!
//! ## Down-stream membership compatibility
//!
//! The output `commitment` is byte-identical to what the
//! `MembershipCircuit` would produce against the same `(root, epoch,
//! salt)` triple, so a 1v1 group can be membership-verified later
//! against the same baked membership VK as anarchy / democracy /
//! oligarchy at depth=5.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use jf_relation::{Circuit, CircuitError, PlonkCircuit, Variable};

use super::poseidon::{poseidon_hash_one_gadget, poseidon_hash_two_gadget};

/// Tree depth. Tier 0 (Small) is the only 1v1 tier; depth=5 → 32
/// leaf slots, of which exactly 2 are populated.
pub const DEPTH: usize = 5;

/// Witness inputs for the 1v1 create circuit.
pub struct OneOnOneCreateWitness {
    /// `Poseidon(Poseidon(root, 0), salt)` — public.
    pub commitment: Fr,
    /// First member's BLS12-381 scalar — private.
    pub secret_key_0: Fr,
    /// Second member's BLS12-381 scalar — private.
    pub secret_key_1: Fr,
    /// 32-byte salt (LE-mod-order encoding) — private.
    pub salt: [u8; 32],
}

/// Allocate the 1v1 create circuit. Returns the public-input
/// `Variable` for `commitment` (already `enforce_equal`'d to the
/// in-circuit derivation).
pub fn synthesize_oneonone_create(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &OneOnOneCreateWitness,
) -> Result<Variable, CircuitError> {
    // ---- Public inputs (allocate first, in fixed order) ----
    let commitment_var = circuit.create_public_variable(witness.commitment)?;
    let epoch_var = circuit.create_public_variable(Fr::from(0u64))?;

    // ---- Witnesses ----
    let sk_0_var = circuit.create_variable(witness.secret_key_0)?;
    let sk_1_var = circuit.create_variable(witness.secret_key_1)?;
    let salt_fr = Fr::from_le_bytes_mod_order(&witness.salt);
    let salt_var = circuit.create_variable(salt_fr)?;

    // ---- Constraints 1+2: leaf_i = Poseidon(secret_key_i) ----
    let leaf_0 = poseidon_hash_one_gadget(circuit, sk_0_var)?;
    let leaf_1 = poseidon_hash_one_gadget(circuit, sk_1_var)?;

    // ---- Constraint 3: root over [leaf_0, leaf_1, 0, 0, ..., 0] ----
    //
    // The naive "build all 31 internal nodes in-circuit" approach
    // collapses to 32_768 gates after finalisation — exactly the
    // n=32_768 SRS ceiling, and jf-plonk preprocess needs *strictly
    // more* powers than gates. Instead, exploit that positions
    // 2..32 are constant zero: every right-subtree above level 1 is
    // a Poseidon-of-zeros constant (Z_1, Z_2, ..., Z_{DEPTH-1})
    // computable at synthesise-time. The active left-spine then
    // takes one Poseidon-2 per level — DEPTH hashes total.
    use super::poseidon::poseidon_hash_two_v05;
    let zero_subtree_hashes: [Fr; DEPTH] = {
        let mut z = [Fr::from(0u64); DEPTH];
        // z[0] = Poseidon(0, 0) — hash of two zero leaves
        z[0] = poseidon_hash_two_v05(&Fr::from(0u64), &Fr::from(0u64));
        // z[i+1] = Poseidon(z[i], z[i]) — hash of two zero-subtrees
        for i in 1..DEPTH {
            z[i] = poseidon_hash_two_v05(&z[i - 1], &z[i - 1]);
        }
        z
    };

    // Active spine: leaf at position 0 / 1, zero-subtree on the right.
    let mut current = poseidon_hash_two_gadget(circuit, leaf_0, leaf_1)?;
    for i in 1..DEPTH {
        let z_const = circuit.create_constant_variable(zero_subtree_hashes[i - 1])?;
        current = poseidon_hash_two_gadget(circuit, current, z_const)?;
    }
    let root_var = current;

    // ---- Constraint 4: commitment binding ----
    //   inner      = Poseidon(root, epoch)
    //   commitment = Poseidon(inner, salt)
    let inner = poseidon_hash_two_gadget(circuit, root_var, epoch_var)?;
    let computed_commitment = poseidon_hash_two_gadget(circuit, inner, salt_var)?;
    circuit.enforce_equal(computed_commitment, commitment_var)?;

    Ok(commitment_var)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::plonk::poseidon::{poseidon_hash_one_v05, poseidon_hash_two_v05};

    /// Compute the canonical commitment natively for the 1v1 founding
    /// witness `(sk_0, sk_1, salt)`. Matches the gadget's in-circuit
    /// derivation modulo the LE-mod-r salt encoding.
    fn native_commitment(sk_0: Fr, sk_1: Fr, salt: [u8; 32]) -> Fr {
        let leaf_0 = poseidon_hash_one_v05(&sk_0);
        let leaf_1 = poseidon_hash_one_v05(&sk_1);
        let num_leaves = 1usize << DEPTH;
        let mut level: Vec<Fr> = Vec::with_capacity(num_leaves);
        level.push(leaf_0);
        level.push(leaf_1);
        for _ in 2..num_leaves {
            level.push(Fr::from(0u64));
        }
        while level.len() > 1 {
            let mut next = Vec::with_capacity(level.len() / 2);
            let mut i = 0;
            while i < level.len() {
                next.push(poseidon_hash_two_v05(&level[i], &level[i + 1]));
                i += 2;
            }
            level = next;
        }
        let root = level[0];
        let salt_fr = Fr::from_le_bytes_mod_order(&salt);
        let inner = poseidon_hash_two_v05(&root, &Fr::from(0u64));
        poseidon_hash_two_v05(&inner, &salt_fr)
    }

    fn build_witness(sk_0: Fr, sk_1: Fr, salt: [u8; 32]) -> OneOnOneCreateWitness {
        OneOnOneCreateWitness {
            commitment: native_commitment(sk_0, sk_1, salt),
            secret_key_0: sk_0,
            secret_key_1: sk_1,
            salt,
        }
    }

    /// Witness-level satisfiability for a well-formed founding witness.
    #[test]
    fn synthesize_satisfies_with_valid_witness() {
        let salt = [0xAA; 32];
        let witness = build_witness(Fr::from(1u64), Fr::from(2u64), salt);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oneonone_create(&mut circuit, &witness).unwrap();
        let public_inputs = vec![witness.commitment, Fr::from(0u64)];
        circuit
            .check_circuit_satisfiability(&public_inputs)
            .expect("valid witness should satisfy");
    }

    /// Tampered public commitment fails.
    #[test]
    fn synthesize_rejects_tampered_commitment() {
        let salt = [0xBB; 32];
        let witness = build_witness(Fr::from(7u64), Fr::from(11u64), salt);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oneonone_create(&mut circuit, &witness).unwrap();
        let wrong = vec![witness.commitment + Fr::from(1u64), Fr::from(0u64)];
        let result = circuit.check_circuit_satisfiability(&wrong);
        assert!(result.is_err(), "wrong commitment must not satisfy");
    }

    /// Wrong epoch fails — epoch is hardcoded to 0 in-circuit.
    #[test]
    fn synthesize_rejects_nonzero_epoch() {
        let salt = [0xCC; 32];
        let witness = build_witness(Fr::from(3u64), Fr::from(5u64), salt);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oneonone_create(&mut circuit, &witness).unwrap();
        let wrong = vec![witness.commitment, Fr::from(1u64)];
        let result = circuit.check_circuit_satisfiability(&wrong);
        assert!(result.is_err(), "non-zero epoch must not satisfy");
    }

    /// Gate-count snapshot. depth=5 → 31 internal-node Poseidon-2
    /// hashes + 2 leaf Poseidon-1 + 2 commitment-binding Poseidon-2.
    #[test]
    fn gate_count() {
        let salt = [0xDD; 32];
        let witness = build_witness(Fr::from(1u64), Fr::from(2u64), salt);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oneonone_create(&mut circuit, &witness).unwrap();
        let raw = circuit.num_gates();
        circuit.finalize_for_arithmetization().unwrap();
        let finalised = circuit.num_gates();
        eprintln!(
            "[gate-count] OneOnOneCreateCircuit (depth={DEPTH}): {raw} raw, {finalised} finalised"
        );
        assert!(
            finalised < 32768,
            "OneOnOneCreateCircuit finalises to {finalised} gates, must fit n=32768 SRS"
        );
    }

    /// Full prove → verify round trip.
    #[test]
    fn round_trip_prove_verify() {
        use rand_chacha::rand_core::SeedableRng;

        let salt = [0xEE; 32];
        let witness = build_witness(Fr::from(100u64), Fr::from(200u64), salt);

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oneonone_create(&mut circuit, &witness).unwrap();
        circuit.finalize_for_arithmetization().unwrap();

        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&circuit).expect("preprocess");
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &circuit).expect("prove");

        let public_inputs = vec![witness.commitment, Fr::from(0u64)];
        crate::prover::plonk::verify(&keys.vk, &public_inputs, &proof)
            .expect("verifier rejected valid 1v1 create proof");

        // Tampered commitment must fail.
        let wrong = vec![witness.commitment + Fr::from(1u64), Fr::from(0u64)];
        assert!(
            crate::prover::plonk::verify(&keys.vk, &wrong, &proof).is_err(),
            "verifier accepted wrong commitment"
        );
    }
}
