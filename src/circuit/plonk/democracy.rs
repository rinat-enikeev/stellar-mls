//! Democracy `Update` circuit (PLONK port).
//!
//! ## Status — simplified initial port
//!
//! The Groth16 reference at `src/circuit/democracy.rs` enforces a
//! K-of-N quorum + single-leaf-delta change between `root_old` and
//! `root_new`. **This PLONK port does not yet implement those
//! quorum-and-delta constraints.** It binds the occupancy commitments
//! and threshold into the public-input transcript (so any tampering
//! is caught at the verifier), but in-circuit semantics are reduced
//! to:
//!
//!   1. Standard single-signer Merkle membership at `root_old`.
//!   2. `c_old` binding to `(member_root_old, epoch_old,
//!       occupancy_commitment_old, salt_old)`.
//!   3. `c_new` binding to `(member_root_new, epoch_old+1,
//!       occupancy_commitment_new, salt_new)`.
//!   4. `threshold_numerator` allocated as PI (transcript-bound).
//!
//! This is the same security relaxation as `circuit::plonk::update`
//! re: new-tree membership, *plus* deferral of the quorum+delta
//! constraint stack. Documented loudly so a follow-up PR can land
//! the full quorum semantics without changing the contract surface.
//!
//! ## Public inputs (6, fixed order)
//!
//!   1. `c_old`
//!   2. `epoch_old`
//!   3. `c_new`
//!   4. `occupancy_commitment_old`
//!   5. `occupancy_commitment_new`
//!   6. `threshold_numerator`
//!
//! ## Commitment derivation
//!
//!   `c_X = Poseidon(Poseidon(Poseidon(member_root_X, epoch_X),
//!                            salt_X),
//!                   occupancy_commitment_X)`
//!
//! Note the third Poseidon level binds occupancy_commitment into
//! the on-chain commitment — different from anarchy's two-level
//! derivation. This guarantees that updating member counts requires
//! a fresh proof bound to the new occupancy.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use jf_relation::{BoolVar, Circuit, CircuitError, PlonkCircuit, Variable};

use super::merkle::compute_merkle_root_gadget;
use super::poseidon::{poseidon_hash_one_gadget, poseidon_hash_two_gadget};

/// Witness for the Democracy update circuit.
pub struct DemocracyUpdateWitness {
    pub c_old: Fr,
    pub epoch_old: u64,
    pub c_new: Fr,
    pub occupancy_commitment_old: Fr,
    pub occupancy_commitment_new: Fr,
    pub threshold_numerator: u64,

    pub secret_key: Fr,
    pub member_root_old: Fr,
    pub member_root_new: Fr,
    pub salt_old: [u8; 32],
    pub salt_new: [u8; 32],
    pub merkle_path_old: Vec<Fr>,
    pub leaf_index_old: usize,
    pub depth: usize,
}

pub fn synthesize_democracy_update(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &DemocracyUpdateWitness,
) -> Result<(), CircuitError> {
    if witness.merkle_path_old.len() != witness.depth {
        return Err(CircuitError::ParameterError(format!(
            "merkle_path_old length {} != depth {}",
            witness.merkle_path_old.len(),
            witness.depth
        )));
    }
    if witness.depth >= usize::BITS as usize {
        return Err(CircuitError::ParameterError(format!(
            "depth {} >= usize::BITS",
            witness.depth
        )));
    }
    if witness.leaf_index_old >= (1usize << witness.depth) {
        return Err(CircuitError::ParameterError(format!(
            "leaf_index_old {} out of range",
            witness.leaf_index_old
        )));
    }

    // Public inputs (fixed order).
    let c_old_var = circuit.create_public_variable(witness.c_old)?;
    let epoch_old_var = circuit.create_public_variable(Fr::from(witness.epoch_old))?;
    let c_new_var = circuit.create_public_variable(witness.c_new)?;
    let occ_old_var = circuit.create_public_variable(witness.occupancy_commitment_old)?;
    let occ_new_var = circuit.create_public_variable(witness.occupancy_commitment_new)?;
    let _threshold_var =
        circuit.create_public_variable(Fr::from(witness.threshold_numerator))?;

    // Witnesses.
    let sk_var = circuit.create_variable(witness.secret_key)?;
    let root_old_var = circuit.create_variable(witness.member_root_old)?;
    let root_new_var = circuit.create_variable(witness.member_root_new)?;
    let salt_old_fr = Fr::from_le_bytes_mod_order(&witness.salt_old);
    let salt_new_fr = Fr::from_le_bytes_mod_order(&witness.salt_new);
    let salt_old_var = circuit.create_variable(salt_old_fr)?;
    let salt_new_var = circuit.create_variable(salt_new_fr)?;
    let path_old_vars: Vec<Variable> = witness
        .merkle_path_old
        .iter()
        .map(|s| circuit.create_variable(*s))
        .collect::<Result<_, _>>()?;
    let bit_old_vars: Vec<BoolVar> = (0..witness.depth)
        .map(|i| {
            circuit.create_boolean_variable(((witness.leaf_index_old >> i) & 1) == 1)
        })
        .collect::<Result<_, _>>()?;

    // 1. Membership: leaf opens to root_old.
    let leaf_var = poseidon_hash_one_gadget(circuit, sk_var)?;
    let computed_root_old =
        compute_merkle_root_gadget(circuit, leaf_var, &path_old_vars, &bit_old_vars)?;
    circuit.enforce_equal(computed_root_old, root_old_var)?;

    // 2. c_old = Poseidon(Poseidon(Poseidon(root_old, epoch_old), salt_old), occ_old).
    let inner_old =
        poseidon_hash_two_gadget(circuit, root_old_var, epoch_old_var)?;
    let mid_old = poseidon_hash_two_gadget(circuit, inner_old, salt_old_var)?;
    let computed_c_old = poseidon_hash_two_gadget(circuit, mid_old, occ_old_var)?;
    circuit.enforce_equal(computed_c_old, c_old_var)?;

    // 3. epoch_new = epoch_old + 1; c_new = Poseidon(Poseidon(Poseidon(root_new, epoch_new), salt_new), occ_new).
    let epoch_new_var = circuit.add_constant(epoch_old_var, &Fr::from(1u64))?;
    let inner_new = poseidon_hash_two_gadget(circuit, root_new_var, epoch_new_var)?;
    let mid_new = poseidon_hash_two_gadget(circuit, inner_new, salt_new_var)?;
    let computed_c_new = poseidon_hash_two_gadget(circuit, mid_new, occ_new_var)?;
    circuit.enforce_equal(computed_c_new, c_new_var)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::plonk::poseidon::{poseidon_hash_one_v05, poseidon_hash_two_v05};

    fn build_tree(secret_keys: &[Fr], depth: usize) -> (Fr, Vec<Vec<Fr>>) {
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
        let mut paths: Vec<Vec<Fr>> = Vec::with_capacity(secret_keys.len());
        for prover_index in 0..secret_keys.len() {
            let mut path = Vec::with_capacity(depth);
            let mut cur = num_leaves + prover_index;
            for _ in 0..depth {
                let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
                path.push(nodes[sib]);
                cur /= 2;
            }
            paths.push(path);
        }
        (root, paths)
    }

    fn build_witness(
        secret_keys: &[Fr],
        prover_index: usize,
        epoch_old: u64,
        salt_old: [u8; 32],
        salt_new: [u8; 32],
        occ_old: Fr,
        occ_new: Fr,
        threshold: u64,
        depth: usize,
    ) -> DemocracyUpdateWitness {
        let (root, paths) = build_tree(secret_keys, depth);
        let salt_old_fr = Fr::from_le_bytes_mod_order(&salt_old);
        let salt_new_fr = Fr::from_le_bytes_mod_order(&salt_new);
        let inner_old = poseidon_hash_two_v05(&root, &Fr::from(epoch_old));
        let mid_old = poseidon_hash_two_v05(&inner_old, &salt_old_fr);
        let c_old = poseidon_hash_two_v05(&mid_old, &occ_old);
        let inner_new = poseidon_hash_two_v05(&root, &Fr::from(epoch_old + 1));
        let mid_new = poseidon_hash_two_v05(&inner_new, &salt_new_fr);
        let c_new = poseidon_hash_two_v05(&mid_new, &occ_new);
        DemocracyUpdateWitness {
            c_old,
            epoch_old,
            c_new,
            occupancy_commitment_old: occ_old,
            occupancy_commitment_new: occ_new,
            threshold_numerator: threshold,
            secret_key: secret_keys[prover_index],
            member_root_old: root,
            member_root_new: root,
            salt_old,
            salt_new,
            merkle_path_old: paths[prover_index].clone(),
            leaf_index_old: prover_index,
            depth,
        }
    }

    #[test]
    fn satisfies_with_valid_witness() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(
            &sks,
            0,
            42,
            [0xAA; 32],
            [0xBB; 32],
            Fr::from(100u64),
            Fr::from(101u64),
            7,
            3,
        );
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update(&mut c, &w).unwrap();
        let pi = vec![
            w.c_old,
            Fr::from(w.epoch_old),
            w.c_new,
            w.occupancy_commitment_old,
            w.occupancy_commitment_new,
            Fr::from(w.threshold_numerator),
        ];
        c.check_circuit_satisfiability(&pi).unwrap();
    }

    #[test]
    fn rejects_tampered_occ_old() {
        let sks: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_witness(
            &sks,
            0,
            42,
            [0xAA; 32],
            [0xBB; 32],
            Fr::from(100u64),
            Fr::from(101u64),
            7,
            3,
        );
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update(&mut c, &w).unwrap();
        let pi = vec![
            w.c_old,
            Fr::from(w.epoch_old),
            w.c_new,
            w.occupancy_commitment_old + Fr::from(1u64),
            w.occupancy_commitment_new,
            Fr::from(w.threshold_numerator),
        ];
        assert!(c.check_circuit_satisfiability(&pi).is_err());
    }

    #[test]
    fn gate_count_per_tier() {
        for &depth in &[5usize, 8, 11] {
            let sks: Vec<Fr> = (1u64..=2).map(Fr::from).collect();
            let w = build_witness(
                &sks,
                0,
                1,
                [0xAA; 32],
                [0xBB; 32],
                Fr::from(0u64),
                Fr::from(1u64),
                3,
                depth,
            );
            let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
            synthesize_democracy_update(&mut c, &w).unwrap();
            c.finalize_for_arithmetization().unwrap();
            eprintln!("[gate-count] democracy_update depth={depth}: {} gates", c.num_gates());
            assert!(c.num_gates() < 32768);
        }
    }

    #[test]
    fn round_trip_d5() {
        use rand_chacha::rand_core::SeedableRng;
        let sks: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let w = build_witness(
            &sks,
            3,
            1234,
            [0xEE; 32],
            [0xFF; 32],
            Fr::from(8u64),
            Fr::from(9u64),
            5,
            5,
        );
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_democracy_update(&mut c, &w).unwrap();
        c.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&c).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &c).unwrap();
        let pi = vec![
            w.c_old,
            Fr::from(w.epoch_old),
            w.c_new,
            w.occupancy_commitment_old,
            w.occupancy_commitment_new,
            Fr::from(w.threshold_numerator),
        ];
        crate::prover::plonk::verify(&keys.vk, &pi, &proof).unwrap();
    }
}
