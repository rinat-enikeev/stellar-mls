//! Merkle-membership gadget over Poseidon, built on jf-relation gates.
//!
//! Mirrors the verification half of `crate::merkle::PoseidonMerkleTree`:
//! walks a path from leaf to root, conditionally swapping (left, right)
//! at each level based on the index bit, then hashes via the
//! Poseidon gadget from `super::poseidon`.
//!
//! Public surface:
//!
//! - `compute_merkle_root_gadget(circuit, leaf, path, index_bits) ->
//!   Variable` — returns the recomputed root as a circuit variable.
//! - `verify_merkle_path_gadget(circuit, leaf, path, index_bits,
//!   expected_root)` — convenience wrapper that enforces equality
//!   against an expected-root variable.
//!
//! Inputs are presented as already-allocated `Variable` / `BoolVar`
//! handles; the gadget is allocation-free.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::Fr;
use jf_relation::{BoolVar, Circuit, CircuitError, PlonkCircuit, Variable};

use super::poseidon::poseidon_hash_two_gadget;

/// Recompute the Merkle root from a leaf + sibling path + index bits.
///
/// `path[i]` is the sibling at level i (counting from the leaf, index 0 =
/// leaf-level sibling). `index_bits[i]` is the i-th bit of the leaf
/// position; bit i = 0 means the current node is a left child at level i.
///
/// Cost per level: one `conditional_select` for the left-side wire, one
/// for the right-side wire, plus one Poseidon hash (~626 gates). Total
/// per Merkle path of depth `d`: `d × ~628` gates plus the per-level
/// selects.
pub fn compute_merkle_root_gadget(
    circuit: &mut PlonkCircuit<Fr>,
    leaf: Variable,
    path: &[Variable],
    index_bits: &[BoolVar],
) -> Result<Variable, CircuitError> {
    if path.len() != index_bits.len() {
        return Err(CircuitError::ParameterError(format!(
            "Merkle gadget: path.len() = {}, index_bits.len() = {} (must be equal)",
            path.len(),
            index_bits.len()
        )));
    }
    let mut current = leaf;
    for (sibling, is_right) in path.iter().copied().zip(index_bits.iter().copied()) {
        // is_right = 0  →  current is left child  →  hash(current, sibling)
        // is_right = 1  →  current is right child →  hash(sibling, current)
        let left = circuit.conditional_select(is_right, current, sibling)?;
        let right = circuit.conditional_select(is_right, sibling, current)?;
        current = poseidon_hash_two_gadget(circuit, left, right)?;
    }
    Ok(current)
}

/// Enforce that the leaf is in the tree whose root equals `expected_root`.
///
/// Convenience wrapper around `compute_merkle_root_gadget`.
pub fn verify_merkle_path_gadget(
    circuit: &mut PlonkCircuit<Fr>,
    leaf: Variable,
    path: &[Variable],
    index_bits: &[BoolVar],
    expected_root: Variable,
) -> Result<(), CircuitError> {
    let computed = compute_merkle_root_gadget(circuit, leaf, path, index_bits)?;
    circuit.enforce_equal(computed, expected_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::plonk::poseidon::poseidon_hash_two_v05;
    use rand_chacha::rand_core::SeedableRng;

    /// Build a small Poseidon Merkle tree natively in v0.5, return root +
    /// per-leaf opening proofs.
    fn build_test_tree(
        leaves: &[Fr],
        depth: usize,
    ) -> (Fr, Vec<(usize, Vec<Fr>)>) {
        let num_leaves = 1usize << depth;
        assert!(leaves.len() <= num_leaves);

        // Total nodes: 2^(depth+1). nodes[0] unused, nodes[1] = root.
        let total = 2 * num_leaves;
        let mut nodes = vec![Fr::from(0u64); total];

        // Place leaves at indices [num_leaves..2*num_leaves), pad with zero.
        for (i, leaf) in leaves.iter().enumerate() {
            nodes[num_leaves + i] = *leaf;
        }

        // Build internal nodes bottom-up.
        for i in (1..num_leaves).rev() {
            let l = nodes[2 * i];
            let r = nodes[2 * i + 1];
            nodes[i] = poseidon_hash_two_v05(&l, &r);
        }

        let root = nodes[1];

        // Generate one proof per leaf.
        let mut proofs = Vec::with_capacity(leaves.len());
        for leaf_idx in 0..leaves.len() {
            let mut path = Vec::with_capacity(depth);
            let mut cur = num_leaves + leaf_idx;
            for _ in 0..depth {
                let sibling_idx = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
                path.push(nodes[sibling_idx]);
                cur /= 2;
            }
            proofs.push((leaf_idx, path));
        }
        (root, proofs)
    }

    /// Witness-level: gadget recomputes the same root the native tree
    /// produces, for every leaf in a small test tree.
    #[test]
    fn gadget_recomputes_native_root_at_witness_level() {
        let depth = 3;
        let leaves: Vec<Fr> = (1u64..=6).map(Fr::from).collect();
        let (root, proofs) = build_test_tree(&leaves, depth);

        for (leaf_idx, path) in &proofs {
            let leaf = leaves[*leaf_idx];

            let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
            let leaf_var = circuit.create_variable(leaf).unwrap();
            let path_vars: Vec<Variable> = path
                .iter()
                .map(|s| circuit.create_variable(*s).unwrap())
                .collect();
            let bit_vars: Vec<BoolVar> = (0..depth)
                .map(|i| circuit.create_boolean_variable(((*leaf_idx >> i) & 1) == 1).unwrap())
                .collect();

            let computed = compute_merkle_root_gadget(&mut circuit, leaf_var, &path_vars, &bit_vars)
                .unwrap();
            assert_eq!(
                circuit.witness(computed).unwrap(),
                root,
                "gadget computed wrong root for leaf {leaf_idx}"
            );
        }
    }

    /// Full prove → verify round trip. Builds a circuit asserting
    /// `compute_merkle_root_gadget(leaf, path, index_bits) == public_root`,
    /// runs PlonkKzgSnark::{preprocess, prove, verify} against the embedded
    /// EF KZG SRS, and checks both the accept and reject paths.
    ///
    /// At depth=3 the circuit has ~3 × 628 ≈ 1884 gates plus a handful
    /// for input plumbing — comfortably within EF KZG's n=4096 SRS.
    #[test]
    fn merkle_round_trip_prove_verify_small_tree() {
        let depth = 3;
        let leaves: Vec<Fr> = (1u64..=6).map(Fr::from).collect();
        let (root, proofs) = build_test_tree(&leaves, depth);

        let leaf_idx = 5usize;
        let leaf = leaves[leaf_idx];
        let path = &proofs[leaf_idx].1;

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        let root_var = circuit.create_public_variable(root).unwrap();
        let leaf_var = circuit.create_variable(leaf).unwrap();
        let path_vars: Vec<Variable> = path
            .iter()
            .map(|s| circuit.create_variable(*s).unwrap())
            .collect();
        let bit_vars: Vec<BoolVar> = (0..depth)
            .map(|i| circuit.create_boolean_variable(((leaf_idx >> i) & 1) == 1).unwrap())
            .collect();

        verify_merkle_path_gadget(&mut circuit, leaf_var, &path_vars, &bit_vars, root_var)
            .unwrap();
        circuit.finalize_for_arithmetization().unwrap();

        let n_gates = circuit.num_gates();
        eprintln!("[gate-count] Merkle path depth={depth}: {n_gates} gates");

        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let keys = crate::prover::plonk::preprocess(&circuit).expect("preprocess");
        let proof = crate::prover::plonk::prove(&mut rng, &keys.pk, &circuit).expect("prove");
        assert!(
            crate::prover::plonk::verify(&keys.vk, &[root], &proof),
            "verifier rejected a valid Merkle-membership proof"
        );

        // Tampered root must fail.
        let wrong_root = root + Fr::from(1u64);
        assert!(
            !crate::prover::plonk::verify(&keys.vk, &[wrong_root], &proof),
            "verifier accepted a Merkle proof against the wrong root"
        );
    }

    /// A wrong index_bit makes the gadget compute a different root, so
    /// the verifier rejects. Catches a class of bugs where the
    /// conditional-select wires get crossed.
    #[test]
    fn merkle_round_trip_rejects_wrong_index_bits() {
        let depth = 3;
        let leaves: Vec<Fr> = (1u64..=4).map(Fr::from).collect();
        let (root, proofs) = build_test_tree(&leaves, depth);

        let leaf_idx = 2usize; // bits = 010 (LSB first)
        let leaf = leaves[leaf_idx];
        let path = &proofs[leaf_idx].1;

        // Build a circuit that uses WRONG index bits (flip bit 0).
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        let root_var = circuit.create_public_variable(root).unwrap();
        let leaf_var = circuit.create_variable(leaf).unwrap();
        let path_vars: Vec<Variable> = path
            .iter()
            .map(|s| circuit.create_variable(*s).unwrap())
            .collect();
        let wrong_idx = leaf_idx ^ 0b001;
        let bit_vars: Vec<BoolVar> = (0..depth)
            .map(|i| circuit.create_boolean_variable(((wrong_idx >> i) & 1) == 1).unwrap())
            .collect();

        verify_merkle_path_gadget(&mut circuit, leaf_var, &path_vars, &bit_vars, root_var)
            .unwrap();

        // Circuit-satisfiability check should fail before we even get to
        // the prover, since the gadget computes a root != public_input.
        let result = circuit.check_circuit_satisfiability(&[root]);
        assert!(
            result.is_err(),
            "circuit was satisfiable with wrong index bits — gadget is broken"
        );
    }
}
