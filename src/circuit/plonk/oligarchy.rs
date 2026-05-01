//! Oligarchy `Create` and `Update` circuits — simplified initial port.
//!
//! ## Status
//!
//! This is a **simplified initial port** of the Oligarchy verify
//! circuits. The original Groth16 reference (not in this repo —
//! pinned via VK bytes only) enforced:
//!
//!   * Member-root + admin-root binding to the on-chain commitment
//!     (verbose anti-mismatch binding at create).
//!   * K-of-N admin quorum on update.
//!   * Single-leaf delta on the member root.
//!
//! The PLONK ports below preserve the **public-input shape and
//! transcript binding** of the originals, but in-circuit they reduce
//! to a "witness produces this commitment" relation:
//!
//!   create: c = Poseidon(Poseidon(Poseidon(member_root, 0), salt),
//!                        Poseidon(occupancy_commitment, admin_root))
//!   update: c_old / c_new derivations as per democracy update,
//!          bound to occupancy_commitment_{old,new} and threshold.
//!
//! Documented loudly so a follow-up can land the full quorum + admin
//! semantics without reshaping the contract surface.
//!
//! ## Public inputs
//!
//! **Create** (6, fixed order):
//!   1. `commitment`
//!   2. `epoch` (always 0)
//!   3. `occupancy_commitment`
//!   4. `member_root`
//!   5. `admin_root`
//!   6. `salt_initial`
//!
//! **Update** (6, fixed order — same as democracy_update):
//!   1. `c_old`
//!   2. `epoch_old`
//!   3. `c_new`
//!   4. `occupancy_commitment_old`
//!   5. `occupancy_commitment_new`
//!   6. `admin_threshold_numerator`

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use jf_relation::{Circuit, CircuitError, PlonkCircuit};

use super::poseidon::poseidon_hash_two_gadget;

pub struct OligarchyCreateWitness {
    pub commitment: Fr,
    pub occupancy_commitment: Fr,
    pub member_root: Fr,
    pub admin_root: Fr,
    pub salt_initial: Fr,
}

pub struct OligarchyUpdateWitness {
    pub c_old: Fr,
    pub epoch_old: u64,
    pub c_new: Fr,
    pub occupancy_commitment_old: Fr,
    pub occupancy_commitment_new: Fr,
    pub admin_threshold_numerator: u64,

    pub member_root_old: Fr,
    pub member_root_new: Fr,
    pub salt_old: Fr,
    pub salt_new: Fr,
}

pub fn synthesize_oligarchy_create(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &OligarchyCreateWitness,
) -> Result<(), CircuitError> {
    let commitment_var = circuit.create_public_variable(witness.commitment)?;
    let _epoch_var = circuit.create_public_variable(Fr::from(0u64))?;
    let occ_var = circuit.create_public_variable(witness.occupancy_commitment)?;
    let member_root_var = circuit.create_public_variable(witness.member_root)?;
    let admin_root_var = circuit.create_public_variable(witness.admin_root)?;
    let salt_var = circuit.create_public_variable(witness.salt_initial)?;

    // Simplified binding: c = Poseidon(Poseidon(Poseidon(member_root, 0),
    // salt), Poseidon(occupancy_commitment, admin_root))
    let zero_var = circuit.zero();
    let inner = poseidon_hash_two_gadget(circuit, member_root_var, zero_var)?;
    let mid = poseidon_hash_two_gadget(circuit, inner, salt_var)?;
    let admin_mix = poseidon_hash_two_gadget(circuit, occ_var, admin_root_var)?;
    let computed_c = poseidon_hash_two_gadget(circuit, mid, admin_mix)?;
    circuit.enforce_equal(computed_c, commitment_var)?;

    Ok(())
}

pub fn synthesize_oligarchy_update(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &OligarchyUpdateWitness,
) -> Result<(), CircuitError> {
    let c_old_var = circuit.create_public_variable(witness.c_old)?;
    let epoch_old_var = circuit.create_public_variable(Fr::from(witness.epoch_old))?;
    let c_new_var = circuit.create_public_variable(witness.c_new)?;
    let occ_old_var = circuit.create_public_variable(witness.occupancy_commitment_old)?;
    let occ_new_var = circuit.create_public_variable(witness.occupancy_commitment_new)?;
    let _threshold_var =
        circuit.create_public_variable(Fr::from(witness.admin_threshold_numerator))?;

    let member_root_old_var = circuit.create_variable(witness.member_root_old)?;
    let member_root_new_var = circuit.create_variable(witness.member_root_new)?;
    let salt_old_var = circuit.create_variable(witness.salt_old)?;
    let salt_new_var = circuit.create_variable(witness.salt_new)?;

    // c_old = Poseidon(Poseidon(Poseidon(member_root_old, epoch_old), salt_old), occ_old)
    let inner_old = poseidon_hash_two_gadget(circuit, member_root_old_var, epoch_old_var)?;
    let mid_old = poseidon_hash_two_gadget(circuit, inner_old, salt_old_var)?;
    let computed_c_old = poseidon_hash_two_gadget(circuit, mid_old, occ_old_var)?;
    circuit.enforce_equal(computed_c_old, c_old_var)?;

    // c_new = Poseidon(Poseidon(Poseidon(member_root_new, epoch_old+1), salt_new), occ_new)
    let epoch_new_var = circuit.add_constant(epoch_old_var, &Fr::from(1u64))?;
    let inner_new = poseidon_hash_two_gadget(circuit, member_root_new_var, epoch_new_var)?;
    let mid_new = poseidon_hash_two_gadget(circuit, inner_new, salt_new_var)?;
    let computed_c_new = poseidon_hash_two_gadget(circuit, mid_new, occ_new_var)?;
    circuit.enforce_equal(computed_c_new, c_new_var)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::plonk::poseidon::poseidon_hash_two_v05;

    fn native_create_commitment(
        member_root: Fr,
        salt: Fr,
        occ: Fr,
        admin_root: Fr,
    ) -> Fr {
        let inner = poseidon_hash_two_v05(&member_root, &Fr::from(0u64));
        let mid = poseidon_hash_two_v05(&inner, &salt);
        let admin_mix = poseidon_hash_two_v05(&occ, &admin_root);
        poseidon_hash_two_v05(&mid, &admin_mix)
    }

    fn native_update_c(
        member_root: Fr,
        epoch: u64,
        salt: Fr,
        occ: Fr,
    ) -> Fr {
        let inner = poseidon_hash_two_v05(&member_root, &Fr::from(epoch));
        let mid = poseidon_hash_two_v05(&inner, &salt);
        poseidon_hash_two_v05(&mid, &occ)
    }

    #[test]
    fn create_satisfies() {
        let occ = Fr::from(100u64);
        let member_root = Fr::from(200u64);
        let admin_root = Fr::from(300u64);
        let salt = Fr::from(400u64);
        let c = native_create_commitment(member_root, salt, occ, admin_root);
        let w = OligarchyCreateWitness {
            commitment: c,
            occupancy_commitment: occ,
            member_root,
            admin_root,
            salt_initial: salt,
        };
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_create(&mut circuit, &w).unwrap();
        circuit
            .check_circuit_satisfiability(&[c, Fr::from(0u64), occ, member_root, admin_root, salt])
            .unwrap();
    }

    #[test]
    fn update_satisfies() {
        let member_root = Fr::from(11u64);
        let salt_old = Fr::from(22u64);
        let salt_new = Fr::from(33u64);
        let occ_old = Fr::from(44u64);
        let occ_new = Fr::from(55u64);
        let epoch_old = 7u64;
        let threshold = 5u64;
        let c_old = native_update_c(member_root, epoch_old, salt_old, occ_old);
        let c_new = native_update_c(member_root, epoch_old + 1, salt_new, occ_new);
        let w = OligarchyUpdateWitness {
            c_old,
            epoch_old,
            c_new,
            occupancy_commitment_old: occ_old,
            occupancy_commitment_new: occ_new,
            admin_threshold_numerator: threshold,
            member_root_old: member_root,
            member_root_new: member_root,
            salt_old,
            salt_new,
        };
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update(&mut circuit, &w).unwrap();
        circuit
            .check_circuit_satisfiability(&[
                c_old,
                Fr::from(epoch_old),
                c_new,
                occ_old,
                occ_new,
                Fr::from(threshold),
            ])
            .unwrap();
    }

    #[test]
    fn create_round_trip() {
        use rand_chacha::rand_core::SeedableRng;
        let occ = Fr::from(100u64);
        let member_root = Fr::from(200u64);
        let admin_root = Fr::from(300u64);
        let salt = Fr::from(400u64);
        let c = native_create_commitment(member_root, salt, occ, admin_root);
        let w = OligarchyCreateWitness {
            commitment: c,
            occupancy_commitment: occ,
            member_root,
            admin_root,
            salt_initial: salt,
        };
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_create(&mut circuit, &w).unwrap();
        circuit.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&circuit).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();
        let pi = vec![c, Fr::from(0u64), occ, member_root, admin_root, salt];
        crate::prover::plonk::verify(&keys.vk, &pi, &proof).unwrap();
    }

    #[test]
    fn update_round_trip() {
        use rand_chacha::rand_core::SeedableRng;
        let member_root = Fr::from(11u64);
        let salt_old = Fr::from(22u64);
        let salt_new = Fr::from(33u64);
        let occ_old = Fr::from(44u64);
        let occ_new = Fr::from(55u64);
        let epoch_old = 7u64;
        let threshold = 5u64;
        let c_old = native_update_c(member_root, epoch_old, salt_old, occ_old);
        let c_new = native_update_c(member_root, epoch_old + 1, salt_new, occ_new);
        let w = OligarchyUpdateWitness {
            c_old,
            epoch_old,
            c_new,
            occupancy_commitment_old: occ_old,
            occupancy_commitment_new: occ_new,
            admin_threshold_numerator: threshold,
            member_root_old: member_root,
            member_root_new: member_root,
            salt_old,
            salt_new,
        };
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oligarchy_update(&mut circuit, &w).unwrap();
        circuit.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&circuit).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();
        let pi = vec![
            c_old,
            Fr::from(epoch_old),
            c_new,
            occ_old,
            occ_new,
            Fr::from(threshold),
        ];
        crate::prover::plonk::verify(&keys.vk, &pi, &proof).unwrap();
    }
}
