//! Tyranny `Create` and `Update` circuits.
//!
//! Tyranny is single-admin governance: only the pinned admin can
//! advance a group's membership commitment. The admin's identity is
//! committed at creation as `admin_pubkey_commitment = Poseidon(
//! admin_pubkey, group_id_fr)`, where `admin_pubkey = Poseidon(
//! admin_secret_key)` (the Poseidon-derived form used elsewhere in
//! this codebase). Per-group binding via `group_id_fr` closes
//! cross-group linkability — same admin in two groups produces
//! uncorrelated commitments to anyone who doesn't know the secret
//! key.
//!
//! Both circuits are anarchy's membership / update gadget plus an
//! admin-binding gate; the only public-input difference is the two
//! extra contract-supplied scalars `(admin_pubkey_commitment,
//! group_id_fr)`.
//!
//! ## Create — `synthesize_tyranny_create`
//!
//! Public inputs (4):
//!   `(commitment, epoch=0, admin_pubkey_commitment, group_id_fr)`
//!
//! Witness:
//!   `admin_secret_key, salt, member_root, merkle_path[depth], leaf_index_bits[depth]`
//!
//! Constraints:
//!   1. `leaf = Poseidon(admin_secret_key)`                        — admin = leaf
//!   2. `admin_pubkey_commitment == Poseidon(leaf, group_id_fr)`   — admin binding
//!   3. `MerkleOpen(leaf, path, idx, depth) == member_root`        — admin in tree
//!   4. `commitment == Poseidon(Poseidon(member_root, 0), salt)`   — group binding
//!
//! ## Update — `synthesize_tyranny_update`
//!
//! Public inputs (5):
//!   `(c_old, epoch_old, c_new, admin_pubkey_commitment, group_id_fr)`
//!
//! Witness:
//!   `admin_secret_key, salt_old, salt_new, member_root_old, member_root_new,
//!    merkle_path_old[depth], leaf_index_old_bits[depth]`
//!
//! Constraints (1+2+3 same as create against `member_root_old`):
//!   4. `c_old == Poseidon(Poseidon(member_root_old, epoch_old), salt_old)`
//!   5. `epoch_new = epoch_old + 1` (in-circuit constant)
//!   6. `c_new == Poseidon(Poseidon(member_root_new, epoch_new), salt_new)`
//!
//! New-tree membership is **not** constrained (binding-only,
//! consistent with `circuit::plonk::update`).
//!
//! ## Public-input range
//!
//! `epoch_old`, `group_id_fr` are unrange-checked Fr scalars. The
//! contract entrypoint MUST enforce `u64` range on `epoch_old` and
//! domain-canonical encoding on `group_id_fr` (BE-32-bytes derived
//! from `group_id`). Same caveat as the membership / anarchy-update
//! circuits.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use ark_ff_v05::PrimeField;
use jf_relation::{BoolVar, Circuit, CircuitError, PlonkCircuit, Variable};

use super::merkle::compute_merkle_root_gadget;
use super::poseidon::{poseidon_hash_one_gadget, poseidon_hash_two_gadget};

/// Witness inputs for the Tyranny **create** circuit.
pub struct TyrannyCreateWitness {
    /// `Poseidon(Poseidon(member_root, 0), salt)` — public.
    pub commitment: Fr,
    /// `Poseidon(Poseidon(admin_secret_key), group_id_fr)` — public.
    pub admin_pubkey_commitment: Fr,
    /// Per-group binding scalar — public, contract-derived.
    pub group_id_fr: Fr,

    /// Admin's BLS12-381 scalar — private. (`admin_pubkey =
    /// Poseidon(admin_secret_key)` per the codebase convention.)
    pub admin_secret_key: Fr,
    /// Poseidon Merkle root the admin's leaf belongs to — private.
    pub member_root: Fr,
    /// 32-byte salt; reduced mod r in-circuit — private.
    pub salt: [u8; 32],
    /// Sibling hashes from leaf to root — private; length = depth.
    pub merkle_path: Vec<Fr>,
    /// Admin's leaf position in the tree — private.
    pub leaf_index: usize,
    /// Tree depth.
    pub depth: usize,
}

/// Witness inputs for the Tyranny **update** circuit.
pub struct TyrannyUpdateWitness {
    /// Old binding commitment — public.
    pub c_old: Fr,
    /// Old epoch — public.
    pub epoch_old: u64,
    /// New binding commitment — public.
    pub c_new: Fr,
    /// `Poseidon(Poseidon(admin_secret_key), group_id_fr)` — public.
    pub admin_pubkey_commitment: Fr,
    /// Per-group binding — public.
    pub group_id_fr: Fr,

    /// Admin's secret key — private.
    pub admin_secret_key: Fr,
    /// Member root at the OLD epoch — private.
    pub member_root_old: Fr,
    /// Member root for the NEW epoch — private (binding-only;
    /// new-tree membership not constrained).
    pub member_root_new: Fr,
    /// Salt the OLD commitment was bound to — private.
    pub salt_old: [u8; 32],
    /// Salt for the NEW commitment — private.
    pub salt_new: [u8; 32],
    /// Sibling hashes for the admin's leaf in the OLD tree — private.
    pub merkle_path_old: Vec<Fr>,
    /// Admin's leaf position in the OLD tree — private.
    pub leaf_index_old: usize,
    /// Tree depth.
    pub depth: usize,
}

fn validate_path_and_index(
    path_len: usize,
    leaf_index: usize,
    depth: usize,
    label: &str,
) -> Result<(), CircuitError> {
    if path_len != depth {
        return Err(CircuitError::ParameterError(format!(
            "{label}.merkle_path length {path_len} != depth {depth}"
        )));
    }
    if depth >= usize::BITS as usize {
        return Err(CircuitError::ParameterError(format!(
            "depth {depth} >= usize::BITS"
        )));
    }
    if leaf_index >= (1usize << depth) {
        return Err(CircuitError::ParameterError(format!(
            "{label}.leaf_index {leaf_index} out of range for depth {depth}"
        )));
    }
    Ok(())
}

/// Allocate the Tyranny create circuit. Public-input order is fixed:
/// `(commitment, epoch=0, admin_pubkey_commitment, group_id_fr)`.
pub fn synthesize_tyranny_create(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &TyrannyCreateWitness,
) -> Result<(), CircuitError> {
    validate_path_and_index(
        witness.merkle_path.len(),
        witness.leaf_index,
        witness.depth,
        "create",
    )?;

    // Public inputs (fixed order).
    let commitment_var = circuit.create_public_variable(witness.commitment)?;
    let epoch_var = circuit.create_public_variable(Fr::from(0u64))?;
    let admin_comm_var = circuit.create_public_variable(witness.admin_pubkey_commitment)?;
    let group_id_var = circuit.create_public_variable(witness.group_id_fr)?;

    // Witnesses.
    let admin_sk_var = circuit.create_variable(witness.admin_secret_key)?;
    let member_root_var = circuit.create_variable(witness.member_root)?;
    let salt_fr = Fr::from_le_bytes_mod_order(&witness.salt);
    let salt_var = circuit.create_variable(salt_fr)?;
    let path_vars: Vec<Variable> = witness
        .merkle_path
        .iter()
        .map(|s| circuit.create_variable(*s))
        .collect::<Result<_, _>>()?;
    let bit_vars: Vec<BoolVar> = (0..witness.depth)
        .map(|i| circuit.create_boolean_variable(((witness.leaf_index >> i) & 1) == 1))
        .collect::<Result<_, _>>()?;

    // 1. leaf = Poseidon(admin_secret_key) — leaf is also admin_pubkey.
    let leaf_var = poseidon_hash_one_gadget(circuit, admin_sk_var)?;

    // 2. admin_pubkey_commitment = Poseidon(leaf, group_id_fr).
    let computed_admin_comm =
        poseidon_hash_two_gadget(circuit, leaf_var, group_id_var)?;
    circuit.enforce_equal(computed_admin_comm, admin_comm_var)?;

    // 3. Merkle membership.
    let computed_root =
        compute_merkle_root_gadget(circuit, leaf_var, &path_vars, &bit_vars)?;
    circuit.enforce_equal(computed_root, member_root_var)?;

    // 4. commitment binding.
    let inner = poseidon_hash_two_gadget(circuit, member_root_var, epoch_var)?;
    let computed_commitment = poseidon_hash_two_gadget(circuit, inner, salt_var)?;
    circuit.enforce_equal(computed_commitment, commitment_var)?;

    Ok(())
}

/// Allocate the Tyranny update circuit. Public-input order is fixed:
/// `(c_old, epoch_old, c_new, admin_pubkey_commitment, group_id_fr)`.
pub fn synthesize_tyranny_update(
    circuit: &mut PlonkCircuit<Fr>,
    witness: &TyrannyUpdateWitness,
) -> Result<(), CircuitError> {
    validate_path_and_index(
        witness.merkle_path_old.len(),
        witness.leaf_index_old,
        witness.depth,
        "update",
    )?;

    // Public inputs (fixed order).
    let c_old_var = circuit.create_public_variable(witness.c_old)?;
    let epoch_old_var = circuit.create_public_variable(Fr::from(witness.epoch_old))?;
    let c_new_var = circuit.create_public_variable(witness.c_new)?;
    let admin_comm_var = circuit.create_public_variable(witness.admin_pubkey_commitment)?;
    let group_id_var = circuit.create_public_variable(witness.group_id_fr)?;

    // Witnesses.
    let admin_sk_var = circuit.create_variable(witness.admin_secret_key)?;
    let member_root_old_var = circuit.create_variable(witness.member_root_old)?;
    let member_root_new_var = circuit.create_variable(witness.member_root_new)?;
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

    // 1. leaf = Poseidon(admin_secret_key).
    let leaf_var = poseidon_hash_one_gadget(circuit, admin_sk_var)?;

    // 2. admin_pubkey_commitment binding.
    let computed_admin_comm =
        poseidon_hash_two_gadget(circuit, leaf_var, group_id_var)?;
    circuit.enforce_equal(computed_admin_comm, admin_comm_var)?;

    // 3. Old-tree membership.
    let computed_root_old = compute_merkle_root_gadget(
        circuit,
        leaf_var,
        &path_old_vars,
        &bit_old_vars,
    )?;
    circuit.enforce_equal(computed_root_old, member_root_old_var)?;

    // 4. c_old binding.
    let inner_old =
        poseidon_hash_two_gadget(circuit, member_root_old_var, epoch_old_var)?;
    let computed_c_old = poseidon_hash_two_gadget(circuit, inner_old, salt_old_var)?;
    circuit.enforce_equal(computed_c_old, c_old_var)?;

    // 5. epoch_new = epoch_old + 1; c_new binding.
    let epoch_new_var = circuit.add_constant(epoch_old_var, &Fr::from(1u64))?;
    let inner_new =
        poseidon_hash_two_gadget(circuit, member_root_new_var, epoch_new_var)?;
    let computed_c_new = poseidon_hash_two_gadget(circuit, inner_new, salt_new_var)?;
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

    fn build_create_witness(
        secret_keys: &[Fr],
        admin_index: usize,
        salt: [u8; 32],
        group_id_fr: Fr,
        depth: usize,
    ) -> TyrannyCreateWitness {
        let (root, paths) = build_tree(secret_keys, depth);
        let admin_sk = secret_keys[admin_index];
        let admin_leaf = poseidon_hash_one_v05(&admin_sk);
        let admin_comm = poseidon_hash_two_v05(&admin_leaf, &group_id_fr);
        let salt_fr = Fr::from_le_bytes_mod_order(&salt);
        let inner = poseidon_hash_two_v05(&root, &Fr::from(0u64));
        let commitment = poseidon_hash_two_v05(&inner, &salt_fr);
        TyrannyCreateWitness {
            commitment,
            admin_pubkey_commitment: admin_comm,
            group_id_fr,
            admin_secret_key: admin_sk,
            member_root: root,
            salt,
            merkle_path: paths[admin_index].clone(),
            leaf_index: admin_index,
            depth,
        }
    }

    fn build_update_witness(
        old_keys: &[Fr],
        admin_index: usize,
        new_keys: &[Fr],
        epoch_old: u64,
        salt_old: [u8; 32],
        salt_new: [u8; 32],
        group_id_fr: Fr,
        depth: usize,
    ) -> TyrannyUpdateWitness {
        let (root_old, paths_old) = build_tree(old_keys, depth);
        let (root_new, _) = build_tree(new_keys, depth);
        let admin_sk = old_keys[admin_index];
        let admin_leaf = poseidon_hash_one_v05(&admin_sk);
        let admin_comm = poseidon_hash_two_v05(&admin_leaf, &group_id_fr);
        let salt_old_fr = Fr::from_le_bytes_mod_order(&salt_old);
        let salt_new_fr = Fr::from_le_bytes_mod_order(&salt_new);
        let c_old = poseidon_hash_two_v05(
            &poseidon_hash_two_v05(&root_old, &Fr::from(epoch_old)),
            &salt_old_fr,
        );
        let c_new = poseidon_hash_two_v05(
            &poseidon_hash_two_v05(&root_new, &Fr::from(epoch_old + 1)),
            &salt_new_fr,
        );
        TyrannyUpdateWitness {
            c_old,
            epoch_old,
            c_new,
            admin_pubkey_commitment: admin_comm,
            group_id_fr,
            admin_secret_key: admin_sk,
            member_root_old: root_old,
            member_root_new: root_new,
            salt_old,
            salt_new,
            merkle_path_old: paths_old[admin_index].clone(),
            leaf_index_old: admin_index,
            depth,
        }
    }

    #[test]
    fn create_satisfies_with_valid_witness() {
        let secret_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_create_witness(
            &secret_keys,
            0,
            [0xAA; 32],
            Fr::from(7777u64),
            3,
        );
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_tyranny_create(&mut circuit, &w).unwrap();
        let pi = vec![w.commitment, Fr::from(0u64), w.admin_pubkey_commitment, w.group_id_fr];
        circuit.check_circuit_satisfiability(&pi).expect("valid");
    }

    #[test]
    fn create_rejects_wrong_admin_commitment() {
        let secret_keys: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_create_witness(
            &secret_keys,
            0,
            [0xAA; 32],
            Fr::from(7777u64),
            3,
        );
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_tyranny_create(&mut circuit, &w).unwrap();
        let pi = vec![
            w.commitment,
            Fr::from(0u64),
            w.admin_pubkey_commitment + Fr::from(1u64),
            w.group_id_fr,
        ];
        assert!(circuit.check_circuit_satisfiability(&pi).is_err());
    }

    #[test]
    fn update_satisfies_with_valid_witness() {
        let old: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let new: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_update_witness(
            &old,
            0,
            &new,
            42,
            [0xAA; 32],
            [0xBB; 32],
            Fr::from(7777u64),
            3,
        );
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_tyranny_update(&mut circuit, &w).unwrap();
        let pi = vec![
            w.c_old,
            Fr::from(w.epoch_old),
            w.c_new,
            w.admin_pubkey_commitment,
            w.group_id_fr,
        ];
        circuit.check_circuit_satisfiability(&pi).expect("valid");
    }

    #[test]
    fn update_rejects_wrong_admin_commitment() {
        let old: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let new: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let w = build_update_witness(
            &old,
            0,
            &new,
            42,
            [0xAA; 32],
            [0xBB; 32],
            Fr::from(7777u64),
            3,
        );
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_tyranny_update(&mut circuit, &w).unwrap();
        let pi = vec![
            w.c_old,
            Fr::from(w.epoch_old),
            w.c_new,
            w.admin_pubkey_commitment + Fr::from(1u64),
            w.group_id_fr,
        ];
        assert!(circuit.check_circuit_satisfiability(&pi).is_err());
    }

    /// Gate-count snapshot per tier.
    #[test]
    fn gate_count_per_tier() {
        for &depth in &[5usize, 8, 11] {
            let secret_keys: Vec<Fr> = (1u64..=2).map(Fr::from).collect();
            let w = build_create_witness(&secret_keys, 0, [0xCC; 32], Fr::from(11u64), depth);
            let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
            synthesize_tyranny_create(&mut c, &w).unwrap();
            c.finalize_for_arithmetization().unwrap();
            eprintln!("[gate-count] tyranny_create depth={depth}: {} gates", c.num_gates());
            assert!(c.num_gates() < 32768);

            let new: Vec<Fr> = (1u64..=2).map(Fr::from).collect();
            let wu = build_update_witness(
                &secret_keys, 0, &new, 1, [0xDD; 32], [0xEE; 32], Fr::from(11u64), depth,
            );
            let mut cu = PlonkCircuit::<Fr>::new_turbo_plonk();
            synthesize_tyranny_update(&mut cu, &wu).unwrap();
            cu.finalize_for_arithmetization().unwrap();
            eprintln!("[gate-count] tyranny_update depth={depth}: {} gates", cu.num_gates());
            assert!(cu.num_gates() < 32768);
        }
    }

    /// End-to-end prove → verify, tyranny create at depth=5.
    #[test]
    fn create_round_trip_d5() {
        use rand_chacha::rand_core::SeedableRng;
        let secret_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let w = build_create_witness(&secret_keys, 3, [0xEE; 32], Fr::from(11u64), 5);
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_tyranny_create(&mut c, &w).unwrap();
        c.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&c).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &c).unwrap();
        let pi = vec![w.commitment, Fr::from(0u64), w.admin_pubkey_commitment, w.group_id_fr];
        crate::prover::plonk::verify(&keys.vk, &pi, &proof).unwrap();
    }

    /// End-to-end prove → verify, tyranny update at depth=5.
    #[test]
    fn update_round_trip_d5() {
        use rand_chacha::rand_core::SeedableRng;
        let old: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let new: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let w = build_update_witness(
            &old, 3, &new, 1234, [0xEE; 32], [0xFF; 32], Fr::from(11u64), 5,
        );
        let mut c = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_tyranny_update(&mut c, &w).unwrap();
        c.finalize_for_arithmetization().unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&c).unwrap();
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &c).unwrap();
        let pi = vec![
            w.c_old, Fr::from(w.epoch_old), w.c_new, w.admin_pubkey_commitment, w.group_id_fr,
        ];
        crate::prover::plonk::verify(&keys.vk, &pi, &proof).unwrap();
    }
}
