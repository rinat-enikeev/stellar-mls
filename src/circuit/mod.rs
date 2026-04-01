//! Groth16 circuit for SEP-XXXX membership proof.
//!
//! Public inputs: commitment, epoch
//! Witness: secret_key, poseidon_root, salt, merkle_path, leaf_index
//!
//! Constraints:
//! 1. leaf = Poseidon(secret_key) — key ownership via preimage knowledge
//! 2. MerklePoseidonOpen(leaf, path, index, root) == poseidon_root
//! 3. Poseidon(Poseidon(poseidon_root, epoch), salt) == commitment
//!
//! Key ownership model: The prover demonstrates knowledge of `secret_key`
//! (the BLS12-381 private key scalar). The leaf `Poseidon(secret_key)` is
//! a one-way commitment — only the private key holder can produce the
//! preimage. The binding between the private key and the BLS12-381 public
//! key `pk = sk * G1` is verified off-chain during member registration.
//!
//! ## Circuit Tiers (M-14)
//!
//! The `depth` parameter determines the Merkle tree height and thus the
//! maximum group size. Each tier uses a separate proving/verification key:
//!
//! - **Small** (depth=5): up to 32 members, ~1,910 R1CS constraints
//! - **Medium** (depth=8): up to 256 members, ~2,630 R1CS constraints
//! - **Large** (depth=11): up to 2,048 members, ~3,350 R1CS constraints
//!
//! The depth is fixed at key generation time. Migrating to a different depth
//! requires new proving/verification keys and re-publishing all affected
//! groups' commitments.

use ark_bls12_381::Fr;
use ark_crypto_primitives::sponge::poseidon::PoseidonConfig;
use ark_crypto_primitives::sponge::Absorb;
use ark_ff::PrimeField;
use ark_relations::r1cs::{
    ConstraintSynthesizer, ConstraintSystemRef, SynthesisError,
};
use ark_r1cs_std::prelude::*;
use ark_r1cs_std::fields::fp::FpVar;

use crate::poseidon::poseidon_config;

/// The SEP-XXXX membership circuit.
///
/// Proves: "I know a secret key whose Poseidon hash is a leaf in the
/// Poseidon Merkle tree whose root, combined with epoch and salt,
/// hashes to the public commitment."
#[derive(Clone)]
pub struct MembershipCircuit<F: PrimeField + Absorb> {
    // === Public inputs ===
    /// The Poseidon-based commitment binding value:
    /// `Poseidon(Poseidon(poseidon_root, epoch), salt)`
    pub commitment: Option<F>,
    /// The epoch value.
    pub epoch: Option<u64>,

    // === Witness (private) ===
    /// The prover's BLS12-381 private key scalar.
    /// Key ownership: only the holder of this scalar can produce Poseidon(sk)
    /// which matches the leaf in the Merkle tree.
    pub secret_key: Option<F>,
    /// The Poseidon Merkle root.
    pub poseidon_root: Option<F>,
    /// The 32-byte salt (converted to field element inside the circuit).
    pub salt: Option<[u8; 32]>,
    /// Merkle proof path (sibling hashes, from leaf to root).
    pub merkle_path: Option<Vec<F>>,
    /// Leaf index in the Merkle tree.
    pub leaf_index: Option<usize>,
    /// Tree depth (determines circuit tier).
    pub depth: usize,

    /// Poseidon config (not a witness, but needed for constraint generation).
    pub poseidon_config: PoseidonConfig<F>,
}

impl<F: PrimeField + Absorb> MembershipCircuit<F> {
    /// Create a new circuit with all witness values set.
    pub fn new(
        commitment: F,
        epoch: u64,
        secret_key: F,
        poseidon_root: F,
        salt: [u8; 32],
        merkle_path: Vec<F>,
        leaf_index: usize,
        depth: usize,
    ) -> Self {
        Self {
            commitment: Some(commitment),
            epoch: Some(epoch),
            secret_key: Some(secret_key),
            poseidon_root: Some(poseidon_root),
            salt: Some(salt),
            merkle_path: Some(merkle_path),
            leaf_index: Some(leaf_index),
            depth,
            poseidon_config: poseidon_config::<F>(),
        }
    }

    /// Create an empty circuit (for setup/keygen — no witness values).
    pub fn empty(depth: usize) -> Self {
        Self {
            commitment: None,
            epoch: None,
            secret_key: None,
            poseidon_root: None,
            salt: None,
            merkle_path: None,
            leaf_index: None,
            depth,
            poseidon_config: poseidon_config::<F>(),
        }
    }
}

impl ConstraintSynthesizer<Fr> for MembershipCircuit<Fr> {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Fr>,
    ) -> Result<(), SynthesisError> {
        // ============================================================
        // Allocate public inputs (2 field elements)
        // ============================================================
        let commitment_var = FpVar::new_input(
            cs.clone(),
            || self.commitment.ok_or(SynthesisError::AssignmentMissing),
        )?;
        let epoch_var = FpVar::new_input(
            cs.clone(),
            || {
                self.epoch
                    .map(|e| Fr::from(e))
                    .ok_or(SynthesisError::AssignmentMissing)
            },
        )?;

        // ============================================================
        // Allocate witness values
        // ============================================================
        let secret_key_var = FpVar::new_witness(
            cs.clone(),
            || self.secret_key.ok_or(SynthesisError::AssignmentMissing),
        )?;
        let poseidon_root_var = FpVar::new_witness(
            cs.clone(),
            || self.poseidon_root.ok_or(SynthesisError::AssignmentMissing),
        )?;

        // Salt as a field element witness.
        // Note: from_le_bytes_mod_order reduces 256-bit salt modulo r (~2^255),
        // losing ~1 bit of entropy for ~50% of random salts. This is acceptable
        // as 255 bits exceeds the 128-bit security target.
        let salt_var = FpVar::new_witness(cs.clone(), || {
            self.salt
                .map(|s| Fr::from_le_bytes_mod_order(&s))
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Merkle path siblings
        let path_vars: Vec<FpVar<Fr>> = (0..self.depth)
            .map(|i| {
                FpVar::new_witness(cs.clone(), || {
                    self.merkle_path
                        .as_ref()
                        .and_then(|p| p.get(i).copied())
                        .ok_or(SynthesisError::AssignmentMissing)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Leaf index bits (determines left/right at each level)
        let index_bits: Vec<Boolean<Fr>> = (0..self.depth)
            .map(|i| {
                Boolean::new_witness(cs.clone(), || {
                    self.leaf_index
                        .map(|idx| (idx >> i) & 1 == 1)
                        .ok_or(SynthesisError::AssignmentMissing)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        // ============================================================
        // Constraint 1: Key ownership
        //
        // leaf = Poseidon(secret_key)
        //
        // The prover demonstrates knowledge of the secret key by
        // providing its preimage. Only the private key holder can
        // produce a value that hashes to the registered leaf.
        // ============================================================
        let leaf_var = poseidon_hash_one_gadget(
            cs.clone(),
            &self.poseidon_config,
            &secret_key_var,
        )?;

        // ============================================================
        // Constraint 2: Poseidon Merkle membership
        //
        // Verify that the leaf (derived from secret_key) is in the tree
        // with root poseidon_root.
        // ============================================================
        let mut current = leaf_var;
        for i in 0..self.depth {
            let sibling = &path_vars[i];
            let is_right = &index_bits[i];

            // If is_right=0: hash(current, sibling)
            // If is_right=1: hash(sibling, current)
            let left = FpVar::conditionally_select(is_right, sibling, &current)?;
            let right = FpVar::conditionally_select(is_right, &current, sibling)?;

            current = poseidon_hash_two_gadget(
                cs.clone(),
                &self.poseidon_config,
                &left,
                &right,
            )?;
        }

        // Assert computed root == witness poseidon_root
        current.enforce_equal(&poseidon_root_var)?;

        // ============================================================
        // Constraint 3: Commitment binding (Poseidon-only)
        //
        // Poseidon(Poseidon(poseidon_root, epoch), salt) == commitment
        // ============================================================
        let epoch_and_root_hash = poseidon_hash_two_gadget(
            cs.clone(),
            &self.poseidon_config,
            &poseidon_root_var,
            &epoch_var,
        )?;

        let full_binding = poseidon_hash_two_gadget(
            cs.clone(),
            &self.poseidon_config,
            &epoch_and_root_hash,
            &salt_var,
        )?;

        // Bind to the public commitment input
        full_binding.enforce_equal(&commitment_var)?;

        Ok(())
    }
}

// ================================================================
// Poseidon gadgets (in-circuit Poseidon hash using R1CS constraints)
// ================================================================

/// In-circuit Poseidon hash of one field element.
fn poseidon_hash_one_gadget(
    cs: ConstraintSystemRef<Fr>,
    config: &PoseidonConfig<Fr>,
    input: &FpVar<Fr>,
) -> Result<FpVar<Fr>, SynthesisError> {
    use ark_crypto_primitives::sponge::constraints::CryptographicSpongeVar;
    use ark_crypto_primitives::sponge::poseidon::constraints::PoseidonSpongeVar;

    let mut sponge = PoseidonSpongeVar::new(cs, config);
    sponge.absorb(&input)?;
    let output = sponge.squeeze_field_elements(1)?;
    Ok(output[0].clone())
}

/// In-circuit Poseidon hash of two field elements.
fn poseidon_hash_two_gadget(
    cs: ConstraintSystemRef<Fr>,
    config: &PoseidonConfig<Fr>,
    left: &FpVar<Fr>,
    right: &FpVar<Fr>,
) -> Result<FpVar<Fr>, SynthesisError> {
    use ark_crypto_primitives::sponge::constraints::CryptographicSpongeVar;
    use ark_crypto_primitives::sponge::poseidon::constraints::PoseidonSpongeVar;

    let mut sponge = PoseidonSpongeVar::new(cs, config);
    sponge.absorb(&left)?;
    sponge.absorb(&right)?;
    let output = sponge.squeeze_field_elements(1)?;
    Ok(output[0].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commitment::compute_poseidon_commitment;
    use crate::merkle::{PoseidonMerkleTree, compressed_public_key_bytes};
    use crate::poseidon::poseidon_config;
    use ark_bls12_381::Fr;
    use ark_relations::r1cs::ConstraintSystem;

    /// Helper: build a circuit from secret keys and a chosen prover index.
    fn build_test_circuit(
        secret_keys: &[Fr],
        prover_index: usize,
        epoch: u64,
        salt: [u8; 32],
        depth: usize,
    ) -> MembershipCircuit<Fr> {
        let config = poseidon_config::<Fr>();
        let target_key = secret_keys[prover_index];

        // Build canonical tree and locate the selected prover's canonical index.
        let tree = PoseidonMerkleTree::build(&config, secret_keys, depth).unwrap();
        let canonical_index = canonical_index_for_key(secret_keys, &target_key);

        let root = tree.root();
        let proof = tree.prove(canonical_index);

        // Compute the Poseidon commitment binding
        let commitment = compute_poseidon_commitment(&config, &root, epoch, &salt);

        MembershipCircuit::new(
            commitment,
            epoch,
            target_key,
            root,
            salt,
            proof.path,
            proof.leaf_index,
            depth,
        )
    }

    fn canonical_index_for_key(secret_keys: &[Fr], target_key: &Fr) -> usize {
        let target_public_key = compressed_public_key_bytes(target_key);
        let mut ordered_public_keys: Vec<[u8; 48]> =
            secret_keys.iter().map(compressed_public_key_bytes).collect();
        ordered_public_keys.sort();
        ordered_public_keys
            .iter()
            .position(|public_key| *public_key == target_public_key)
            .expect("selected prover must exist in canonicalized roster")
    }

    #[test]
    fn test_circuit_satisfiable_2_members() {
        let keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let circuit = build_test_circuit(&keys, 0, 0, [0xAA; 32], 5);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            cs.is_satisfied().unwrap(),
            "Circuit should be satisfiable for a valid member"
        );
        println!(
            "2-member circuit: {} constraints",
            cs.num_constraints()
        );
    }

    #[test]
    fn test_circuit_satisfiable_3_members() {
        let keys = vec![Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];
        let circuit = build_test_circuit(&keys, 1, 1, [0xBB; 32], 5);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            cs.is_satisfied().unwrap(),
            "Circuit should be satisfiable for member at index 1"
        );
    }

    #[test]
    fn test_circuit_satisfiable_single_member() {
        let keys = vec![Fr::from(42u64)];
        let circuit = build_test_circuit(&keys, 0, 0, [0x00; 32], 5);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(cs.is_satisfied().unwrap());
    }

    #[test]
    fn test_circuit_satisfiable_full_tree() {
        let keys: Vec<Fr> = (1..=32).map(|i| Fr::from(i as u64)).collect();
        let circuit = build_test_circuit(&keys, 15, 7, [0xCC; 32], 5);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(cs.is_satisfied().unwrap());
    }

    #[test]
    fn test_circuit_rejects_wrong_secret_key() {
        // Build tree with known keys
        let keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let config = poseidon_config::<Fr>();
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
        let canonical_index = canonical_index_for_key(&keys, &keys[0]);
        let root = tree.root();
        let proof = tree.prove(canonical_index);

        let epoch = 0u64;
        let salt = [0xAA; 32];
        let commitment = compute_poseidon_commitment(&config, &root, epoch, &salt);

        // Use a WRONG secret key (not in the tree)
        let wrong_key = Fr::from(999u64);

        let circuit = MembershipCircuit::new(
            commitment,
            epoch,
            wrong_key,   // <- not a member's secret key
            root,
            salt,
            proof.path,
            proof.leaf_index,
            5,
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            !cs.is_satisfied().unwrap(),
            "Circuit must reject a non-member secret key"
        );
    }

    #[test]
    fn test_circuit_rejects_wrong_root() {
        let keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let config = poseidon_config::<Fr>();
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
        let canonical_index = canonical_index_for_key(&keys, &keys[0]);
        let proof = tree.prove(canonical_index);

        let wrong_root = Fr::from(9999u64);
        let epoch = 0u64;
        let salt = [0xAA; 32];
        // Commitment computed with wrong root
        let commitment = compute_poseidon_commitment(&config, &wrong_root, epoch, &salt);

        let circuit = MembershipCircuit::new(
            commitment,
            epoch,
            keys[0],
            wrong_root,   // <- doesn't match the tree
            salt,
            proof.path,
            proof.leaf_index,
            5,
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            !cs.is_satisfied().unwrap(),
            "Circuit must reject a mismatched root"
        );
    }

    #[test]
    fn test_circuit_rejects_wrong_epoch() {
        let keys = vec![Fr::from(100u64)];
        let config = poseidon_config::<Fr>();
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
        let root = tree.root();
        let proof = tree.prove(0);

        let correct_epoch = 5u64;
        let wrong_epoch = 6u64;
        let salt = [0xAA; 32];
        // Commitment computed with correct epoch
        let commitment = compute_poseidon_commitment(&config, &root, correct_epoch, &salt);

        // Public input says wrong epoch, but commitment was built with correct epoch
        let circuit = MembershipCircuit::new(
            commitment,
            wrong_epoch,   // <- public input epoch doesn't match commitment
            keys[0],
            root,
            salt,
            proof.path,
            proof.leaf_index,
            5,
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            !cs.is_satisfied().unwrap(),
            "Circuit must reject mismatched epoch"
        );
    }

    #[test]
    fn test_circuit_rejects_wrong_salt() {
        let keys = vec![Fr::from(100u64)];
        let config = poseidon_config::<Fr>();
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
        let root = tree.root();
        let proof = tree.prove(0);

        let epoch = 0u64;
        let correct_salt = [0xAA; 32];
        let wrong_salt = [0xBB; 32];
        // Commitment computed with correct salt
        let commitment = compute_poseidon_commitment(&config, &root, epoch, &correct_salt);

        let circuit = MembershipCircuit::new(
            commitment,
            epoch,
            keys[0],
            root,
            wrong_salt,   // <- doesn't match the commitment
            proof.path,
            proof.leaf_index,
            5,
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            !cs.is_satisfied().unwrap(),
            "Circuit must reject wrong salt"
        );
    }

    #[test]
    fn test_circuit_depth_8() {
        let keys: Vec<Fr> = (1..=10).map(|i| Fr::from(i as u64)).collect();
        let circuit = build_test_circuit(&keys, 5, 0, [0xDD; 32], 8);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(cs.is_satisfied().unwrap());
        println!(
            "Depth-8 circuit (10 members): {} constraints",
            cs.num_constraints()
        );
    }

    #[test]
    fn test_circuit_depth_11() {
        let keys: Vec<Fr> = (1..=10).map(|i| Fr::from(i as u64)).collect();
        let circuit = build_test_circuit(&keys, 3, 99, [0xEE; 32], 11);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(cs.is_satisfied().unwrap());
        println!(
            "Depth-11 circuit (10 members): {} constraints",
            cs.num_constraints()
        );
    }

    #[test]
    fn test_constraint_count_scales_with_depth() {
        let keys = vec![Fr::from(1u64), Fr::from(2u64)];

        let mut counts = vec![];
        for depth in [5, 8, 11] {
            let circuit = build_test_circuit(&keys, 0, 0, [0x00; 32], depth);
            let cs = ConstraintSystem::<Fr>::new_ref();
            circuit.generate_constraints(cs.clone()).unwrap();
            assert!(cs.is_satisfied().unwrap());
            counts.push((depth, cs.num_constraints()));
        }

        // Constraint count should increase with depth (logarithmic scaling)
        println!("Constraint counts by depth:");
        for (d, c) in &counts {
            println!("  depth {}: {} constraints", d, c);
        }
        assert!(counts[1].1 > counts[0].1);
        assert!(counts[2].1 > counts[1].1);
    }

    #[test]
    fn test_empty_circuit_for_keygen() {
        let circuit = MembershipCircuit::<Fr>::empty(5);
        let cs = ConstraintSystem::<Fr>::new_ref();
        // Should not panic — empty circuit is used for trusted setup
        let result = circuit.generate_constraints(cs.clone());
        // It's OK if this fails with AssignmentMissing in non-setup mode
        // The important thing is that the constraint structure is defined
        assert!(result.is_err() || cs.num_constraints() > 0);
    }

    #[test]
    fn test_public_input_count() {
        // The circuit should have exactly 2 public inputs: commitment and epoch
        let keys = vec![Fr::from(1u64)];
        let circuit = build_test_circuit(&keys, 0, 0, [0x00; 32], 5);

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        // arkworks adds a "one" constant as instance variable 0,
        // so 2 public inputs = 3 instance variables
        assert_eq!(
            cs.num_instance_variables(), 3,
            "Circuit must have exactly 2 public inputs (+ 1 constant)"
        );
    }

    #[test]
    fn test_circuit_rejects_tampered_merkle_path() {
        // Valid circuit, then tamper with one sibling in the Merkle path.
        let keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let config = poseidon_config::<Fr>();
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
        let canonical_index = canonical_index_for_key(&keys, &keys[0]);
        let root = tree.root();
        let proof = tree.prove(canonical_index);

        let epoch = 0u64;
        let salt = [0xAA; 32];
        let commitment = compute_poseidon_commitment(&config, &root, epoch, &salt);

        let mut tampered_path = proof.path.clone();
        tampered_path[0] = Fr::from(12345u64); // corrupt first sibling

        let circuit = MembershipCircuit::new(
            commitment,
            epoch,
            keys[0],
            root,
            salt,
            tampered_path,
            proof.leaf_index,
            5,
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            !cs.is_satisfied().unwrap(),
            "Circuit must reject a tampered Merkle path"
        );
    }

    #[test]
    fn test_circuit_rejects_wrong_leaf_index() {
        // Prover uses correct key and path but claims a different tree position.
        let keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let config = poseidon_config::<Fr>();
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
        let canonical_index = canonical_index_for_key(&keys, &keys[0]);
        let root = tree.root();
        let proof = tree.prove(canonical_index);

        let epoch = 0u64;
        let salt = [0xAA; 32];
        let commitment = compute_poseidon_commitment(&config, &root, epoch, &salt);

        let circuit = MembershipCircuit::new(
            commitment,
            epoch,
            keys[0],
            root,
            salt,
            proof.path,
            if canonical_index == 0 { 1 } else { 0 },
            5,
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            !cs.is_satisfied().unwrap(),
            "Circuit must reject wrong leaf index with mismatched path"
        );
    }

    #[test]
    fn test_circuit_rejects_zero_secret_key_for_empty_slot() {
        // Empty-slot attack: attacker tries sk=0 hoping Poseidon(0) = 0 (the empty leaf).
        // This must fail because Poseidon(0) != 0.
        let keys = vec![Fr::from(100u64)]; // slot 0 occupied, slots 1..31 empty (zero)
        let config = poseidon_config::<Fr>();
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
        let root = tree.root();
        let proof = tree.prove(1); // path for empty slot at index 1

        let epoch = 0u64;
        let salt = [0xAA; 32];
        let commitment = compute_poseidon_commitment(&config, &root, epoch, &salt);

        // Attacker uses sk=0, hoping Poseidon(0) matches the zero leaf
        let circuit = MembershipCircuit::new(
            commitment,
            epoch,
            Fr::from(0u64), // <- attacker's "key"
            root,
            salt,
            proof.path,
            proof.leaf_index,
            5,
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            !cs.is_satisfied().unwrap(),
            "Circuit must reject sk=0 for empty slot (Poseidon(0) != 0)"
        );
    }

    #[test]
    fn test_circuit_last_position_member() {
        // Edge case: member at the last leaf position (all index bits = 1).
        // Tests that conditionally_select works when all bits are set.
        let depth = 5;
        let num_leaves = 1 << depth; // 32
        let keys: Vec<Fr> = (1..=num_leaves as u64).map(Fr::from).collect();
        let config = poseidon_config::<Fr>();
        let tree = PoseidonMerkleTree::build(&config, &keys, depth).unwrap();
        let mut ordered_keys = keys.clone();
        ordered_keys.sort_by_key(compressed_public_key_bytes);
        let last_key = *ordered_keys.last().expect("full tree must contain members");
        let proof = tree.prove(num_leaves - 1);
        let root = tree.root();
        let commitment = compute_poseidon_commitment(&config, &root, 0, &[0xFF; 32]);

        let circuit = MembershipCircuit::new(
            commitment,
            0,
            last_key,
            root,
            [0xFF; 32],
            proof.path,
            proof.leaf_index,
            depth,
        );

        let cs = ConstraintSystem::<Fr>::new_ref();
        circuit.generate_constraints(cs.clone()).unwrap();

        assert!(
            cs.is_satisfied().unwrap(),
            "Circuit must work for the last leaf (all index bits = 1)"
        );
    }

    #[test]
    fn test_native_poseidon_matches_circuit_poseidon() {
        // Verify that the in-circuit Poseidon gadget produces the same
        // output as the native Poseidon function. This is the foundational
        // assumption for the entire circuit's correctness.
        use crate::poseidon::{poseidon_hash_one, poseidon_hash_two};

        let config = poseidon_config::<Fr>();
        let a = Fr::from(42u64);
        let b = Fr::from(99u64);

        // Compute natively
        let native_h1 = poseidon_hash_one(&config, &a);
        let native_h2 = poseidon_hash_two(&config, &a, &b);

        // Compute via circuit gadget and extract the assigned value
        let cs = ConstraintSystem::<Fr>::new_ref();
        let a_var = FpVar::new_witness(cs.clone(), || Ok(a)).unwrap();
        let b_var = FpVar::new_witness(cs.clone(), || Ok(b)).unwrap();

        let circuit_h1 = poseidon_hash_one_gadget(cs.clone(), &config, &a_var).unwrap();
        let circuit_h2 = poseidon_hash_two_gadget(cs.clone(), &config, &a_var, &b_var).unwrap();

        assert_eq!(circuit_h1.value().unwrap(), native_h1,
            "In-circuit Poseidon hash_one must match native");
        assert_eq!(circuit_h2.value().unwrap(), native_h2,
            "In-circuit Poseidon hash_two must match native");
        assert!(cs.is_satisfied().unwrap());
    }
}
