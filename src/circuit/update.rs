//! UpdateCircuit — Groth16 circuit that binds a membership proof at the old
//! epoch to the specific C_new being authorized on-chain.
//!
//! Public inputs: c_old, epoch_old, c_new (in this fixed allocation order)
//! Witness: secret_key, poseidon_root_old, salt_old, merkle_path_old,
//!          leaf_index_old, poseidon_root_new, salt_new
//!
//! Constraints:
//! 1. leaf = Poseidon(secret_key), and it opens along path_old at idx_old to
//!    poseidon_root_old
//! 2. c_old = Poseidon(Poseidon(poseidon_root_old, epoch_old), salt_old)
//! 3. c_new = Poseidon(Poseidon(poseidon_root_new, epoch_old + 1), salt_new)
//!
//! See docs/update-circuit-binding-design.md §5 for the formal relation and
//! docs/vuln-unbound-new-commitment.md for the defect this circuit closes.

use ark_bls12_381::Fr;
use ark_crypto_primitives::sponge::poseidon::PoseidonConfig;
use ark_crypto_primitives::sponge::Absorb;
use ark_ff::PrimeField;
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::prelude::*;
use ark_relations::r1cs::{
    ConstraintSynthesizer, ConstraintSystemRef, SynthesisError,
};

use crate::circuit::{poseidon_hash_one_gadget, poseidon_hash_two_gadget};
use crate::poseidon::poseidon_config;

/// The SEP-XXXX update circuit.
///
/// Proves: "I know a secret key that is a leaf in the old tree of commitment
/// C_old at epoch e_old, and I know (root_new, salt_new) such that
/// C_new = Poseidon(Poseidon(root_new, e_old + 1), salt_new)."
#[derive(Clone)]
pub struct UpdateCircuit<F: PrimeField + Absorb> {
    // === Public inputs (allocated in fixed order: c_old, epoch_old, c_new) ===
    pub c_old: Option<F>,
    pub epoch_old: Option<u64>,
    pub c_new: Option<F>,

    // === Witnesses ===
    pub secret_key: Option<F>,
    pub poseidon_root_old: Option<F>,
    pub salt_old: Option<[u8; 32]>,
    pub merkle_path_old: Option<Vec<F>>,
    pub leaf_index_old: Option<usize>,
    pub poseidon_root_new: Option<F>,
    pub salt_new: Option<[u8; 32]>,

    /// Tree depth (determines circuit tier).
    pub depth: usize,

    /// Poseidon config (not a witness, but needed for constraint generation).
    pub poseidon_config: PoseidonConfig<F>,
}

impl<F: PrimeField + Absorb> UpdateCircuit<F> {
    /// Create a new update circuit with all witness values set.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        c_old: F,
        epoch_old: u64,
        c_new: F,
        secret_key: F,
        poseidon_root_old: F,
        salt_old: [u8; 32],
        merkle_path_old: Vec<F>,
        leaf_index_old: usize,
        poseidon_root_new: F,
        salt_new: [u8; 32],
        depth: usize,
    ) -> Self {
        Self {
            c_old: Some(c_old),
            epoch_old: Some(epoch_old),
            c_new: Some(c_new),
            secret_key: Some(secret_key),
            poseidon_root_old: Some(poseidon_root_old),
            salt_old: Some(salt_old),
            merkle_path_old: Some(merkle_path_old),
            leaf_index_old: Some(leaf_index_old),
            poseidon_root_new: Some(poseidon_root_new),
            salt_new: Some(salt_new),
            depth,
            poseidon_config: poseidon_config::<F>(),
        }
    }

    /// Create an empty circuit (for setup/keygen — no witness values).
    pub fn empty(depth: usize) -> Self {
        Self {
            c_old: None,
            epoch_old: None,
            c_new: None,
            secret_key: None,
            poseidon_root_old: None,
            salt_old: None,
            merkle_path_old: None,
            leaf_index_old: None,
            poseidon_root_new: None,
            salt_new: None,
            depth,
            poseidon_config: poseidon_config::<F>(),
        }
    }
}

impl ConstraintSynthesizer<Fr> for UpdateCircuit<Fr> {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // ============================================================
        // Public inputs — pinned allocation order (c_old, epoch_old, c_new).
        // Allocation order IS the serialisation order; the Groth16 IC vector
        // indexes by allocation order. Changing this order without also
        // updating the prover, verifier, contract MSM, and wire format will
        // break every verification call site silently.
        // ============================================================
        let c_old_var = FpVar::new_input(cs.clone(), || {
            self.c_old.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let epoch_old_var = FpVar::new_input(cs.clone(), || {
            self.epoch_old
                .map(Fr::from)
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let c_new_var = FpVar::new_input(cs.clone(), || {
            self.c_new.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // ============================================================
        // Witness allocation — old-tree authentication path.
        // ============================================================
        let secret_key_var = FpVar::new_witness(cs.clone(), || {
            self.secret_key.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let poseidon_root_old_var = FpVar::new_witness(cs.clone(), || {
            self.poseidon_root_old
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let salt_old_var = FpVar::new_witness(cs.clone(), || {
            self.salt_old
                .map(|s| Fr::from_le_bytes_mod_order(&s))
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let path_old_vars: Vec<FpVar<Fr>> = (0..self.depth)
            .map(|i| {
                FpVar::new_witness(cs.clone(), || {
                    self.merkle_path_old
                        .as_ref()
                        .and_then(|p| p.get(i).copied())
                        .ok_or(SynthesisError::AssignmentMissing)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let index_bits_old: Vec<Boolean<Fr>> = (0..self.depth)
            .map(|i| {
                Boolean::new_witness(cs.clone(), || {
                    self.leaf_index_old
                        .map(|idx| (idx >> i) & 1 == 1)
                        .ok_or(SynthesisError::AssignmentMissing)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        // ============================================================
        // Witness allocation — new tree.
        // ============================================================
        let poseidon_root_new_var = FpVar::new_witness(cs.clone(), || {
            self.poseidon_root_new
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let salt_new_var = FpVar::new_witness(cs.clone(), || {
            self.salt_new
                .map(|s| Fr::from_le_bytes_mod_order(&s))
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        // ============================================================
        // Constraint (1): Key ownership + old-tree membership.
        //
        // leaf = Poseidon(secret_key); leaf folds along path_old/index_old to
        // poseidon_root_old. Same construction as MembershipCircuit.
        // ============================================================
        let leaf_var = poseidon_hash_one_gadget(
            cs.clone(),
            &self.poseidon_config,
            &secret_key_var,
        )?;

        let mut current = leaf_var;
        for i in 0..self.depth {
            let sibling = &path_old_vars[i];
            let is_right = &index_bits_old[i];

            let left = FpVar::conditionally_select(is_right, sibling, &current)?;
            let right = FpVar::conditionally_select(is_right, &current, sibling)?;

            current = poseidon_hash_two_gadget(
                cs.clone(),
                &self.poseidon_config,
                &left,
                &right,
            )?;
        }
        current.enforce_equal(&poseidon_root_old_var)?;

        // ============================================================
        // Constraint (2): Old-commitment binding.
        //
        // c_old = Poseidon(Poseidon(poseidon_root_old, epoch_old), salt_old)
        // ============================================================
        let old_root_epoch = poseidon_hash_two_gadget(
            cs.clone(),
            &self.poseidon_config,
            &poseidon_root_old_var,
            &epoch_old_var,
        )?;
        let c_old_derived = poseidon_hash_two_gadget(
            cs.clone(),
            &self.poseidon_config,
            &old_root_epoch,
            &salt_old_var,
        )?;
        c_old_derived.enforce_equal(&c_old_var)?;

        // ============================================================
        // Constraint (3): New-commitment binding — the core fix.
        //
        // c_new = Poseidon(Poseidon(poseidon_root_new, epoch_old + 1), salt_new)
        //
        // epoch_new is derived in-circuit as epoch_old_var + 1; it is never
        // a public input and never transmitted. The circuit enforces
        // monotonicity and commitment binding together.
        // ============================================================
        let epoch_new_var = &epoch_old_var + FpVar::Constant(Fr::from(1u64));
        let new_root_epoch = poseidon_hash_two_gadget(
            cs.clone(),
            &self.poseidon_config,
            &poseidon_root_new_var,
            &epoch_new_var,
        )?;
        let c_new_derived = poseidon_hash_two_gadget(
            cs,
            &self.poseidon_config,
            &new_root_epoch,
            &salt_new_var,
        )?;
        c_new_derived.enforce_equal(&c_new_var)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commitment::compute_poseidon_commitment;
    use crate::merkle::{compressed_public_key_bytes, PoseidonMerkleTree};
    use ark_bls12_381::Fr;
    use ark_relations::r1cs::ConstraintSystem;

    fn canonical_index(secret_keys: &[Fr], target: &Fr) -> usize {
        let target_pk = compressed_public_key_bytes(target);
        let mut ordered: Vec<[u8; 48]> =
            secret_keys.iter().map(compressed_public_key_bytes).collect();
        ordered.sort();
        ordered
            .iter()
            .position(|pk| pk == &target_pk)
            .expect("key must exist in roster")
    }

    struct Scenario {
        circuit: UpdateCircuit<Fr>,
    }

    fn build_scenario(
        old_keys: &[Fr],
        prover_idx_in_old: usize,
        new_keys: &[Fr],
        epoch_old: u64,
        salt_old: [u8; 32],
        salt_new: [u8; 32],
        depth: usize,
    ) -> Scenario {
        let config = poseidon_config::<Fr>();
        let sk = old_keys[prover_idx_in_old];

        let tree_old = PoseidonMerkleTree::build(&config, old_keys, depth).unwrap();
        let idx_old = canonical_index(old_keys, &sk);
        let root_old = tree_old.root();
        let path_old = tree_old.prove(idx_old);

        let tree_new = PoseidonMerkleTree::build(&config, new_keys, depth).unwrap();
        let root_new = tree_new.root();

        let c_old = compute_poseidon_commitment(&config, &root_old, epoch_old, &salt_old);
        let c_new = compute_poseidon_commitment(&config, &root_new, epoch_old + 1, &salt_new);

        let circuit = UpdateCircuit::<Fr>::new(
            c_old,
            epoch_old,
            c_new,
            sk,
            root_old,
            salt_old,
            path_old.path,
            path_old.leaf_index,
            root_new,
            salt_new,
            depth,
        );

        Scenario { circuit }
    }

    #[test]
    fn update_circuit_accepts_valid_transition() {
        let old_keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let new_keys = vec![Fr::from(100u64), Fr::from(200u64), Fr::from(300u64)];
        let s = build_scenario(&old_keys, 0, &new_keys, 0, [0xAA; 32], [0xBB; 32], 5);

        let cs = ConstraintSystem::<Fr>::new_ref();
        s.circuit.generate_constraints(cs.clone()).unwrap();
        assert!(cs.is_satisfied().unwrap(), "valid transition must satisfy");
        // 1 (constant) + 3 (public inputs) = 4 instance variables
        assert_eq!(cs.num_instance_variables(), 4);
    }

    #[test]
    fn update_circuit_rejects_wrong_c_new() {
        let old_keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let new_keys = vec![Fr::from(100u64), Fr::from(200u64), Fr::from(300u64)];
        let mut s = build_scenario(&old_keys, 0, &new_keys, 0, [0xAA; 32], [0xBB; 32], 5);

        // Swap the public c_new to an unrelated value while keeping the witness
        // (including the derived c_new from salt_new/root_new) intact.
        s.circuit.c_new = Some(Fr::from(0xDEAD_BEEFu64));

        let cs = ConstraintSystem::<Fr>::new_ref();
        s.circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap(), "rebinding must not satisfy");
    }

    #[test]
    fn update_circuit_rejects_non_monotonic_epoch() {
        let old_keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let new_keys = vec![Fr::from(100u64), Fr::from(200u64), Fr::from(300u64)];

        // Build a c_new that commits to epoch_old + 2 instead of epoch_old + 1.
        // The in-circuit derivation uses epoch_old + 1, so constraint (3) must
        // fail.
        let config = poseidon_config::<Fr>();
        let sk = old_keys[0];
        let epoch_old = 0u64;
        let salt_old = [0xAA; 32];
        let salt_new = [0xBB; 32];
        let depth = 5;

        let tree_old = PoseidonMerkleTree::build(&config, &old_keys, depth).unwrap();
        let idx_old = canonical_index(&old_keys, &sk);
        let root_old = tree_old.root();
        let path_old = tree_old.prove(idx_old);

        let tree_new = PoseidonMerkleTree::build(&config, &new_keys, depth).unwrap();
        let root_new = tree_new.root();

        let c_old = compute_poseidon_commitment(&config, &root_old, epoch_old, &salt_old);
        let c_new_wrong =
            compute_poseidon_commitment(&config, &root_new, epoch_old + 2, &salt_new);

        let circuit = UpdateCircuit::<Fr>::new(
            c_old,
            epoch_old,
            c_new_wrong,
            sk,
            root_old,
            salt_old,
            path_old.path,
            path_old.leaf_index,
            root_new,
            salt_new,
            depth,
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();
        assert!(
            !cs.is_satisfied().unwrap(),
            "non-monotonic epoch must not satisfy"
        );
    }

    #[test]
    fn update_circuit_rejects_wrong_auth_path() {
        let old_keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let new_keys = vec![Fr::from(100u64), Fr::from(200u64), Fr::from(300u64)];
        let mut s = build_scenario(&old_keys, 0, &new_keys, 0, [0xAA; 32], [0xBB; 32], 5);

        // Corrupt one sibling hash — leaf no longer opens to root_old.
        if let Some(ref mut path) = s.circuit.merkle_path_old {
            path[0] = path[0] + Fr::from(1u64);
        }

        let cs = ConstraintSystem::<Fr>::new_ref();
        s.circuit.generate_constraints(cs.clone()).unwrap();
        assert!(!cs.is_satisfied().unwrap(), "wrong path must not satisfy");
    }

    #[test]
    fn update_circuit_public_input_order_pinned() {
        let old_keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let new_keys = vec![Fr::from(100u64), Fr::from(200u64), Fr::from(300u64)];
        let s = build_scenario(&old_keys, 0, &new_keys, 7, [0xAA; 32], [0xBB; 32], 5);

        // Capture the three public-input values in allocation order.
        let c_old_expected = s.circuit.c_old.unwrap();
        let epoch_old_expected = s.circuit.epoch_old.unwrap();
        let c_new_expected = s.circuit.c_new.unwrap();

        let cs = ConstraintSystem::<Fr>::new_ref();
        s.circuit.generate_constraints(cs.clone()).unwrap();

        // 1 (ConstraintSystem constant) + 3 public inputs = 4.
        assert_eq!(cs.num_instance_variables(), 4);

        // Read back instance assignments. Index 0 is the constant 1; indices
        // 1-3 are the public inputs in allocation order.
        let cs_ref = cs.borrow().unwrap();
        let instance = &cs_ref.instance_assignment;
        assert_eq!(instance.len(), 4);
        assert_eq!(instance[0], Fr::from(1u64), "index 0 is the ConstraintSystem constant");
        assert_eq!(instance[1], c_old_expected, "index 1 must be c_old");
        assert_eq!(instance[2], Fr::from(epoch_old_expected), "index 2 must be epoch_old");
        assert_eq!(instance[3], c_new_expected, "index 3 must be c_new");
    }
}
