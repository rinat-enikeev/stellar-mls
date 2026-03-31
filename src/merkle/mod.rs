//! Poseidon Merkle tree for member set commitment.
//!
//! Per SEP-XXXX Section 2.2:
//! - Binary tree of fixed depth `d` determined by circuit tier
//! - Leaves: `Poseidon(secret_key_scalar)` for real members, zero for empty slots
//! - Internal nodes: `Poseidon(left, right)`
//! - Root: `poseidon_root` used in the on-chain commitment
//!
//! Two construction methods:
//! - `build()`: takes raw secret key scalars, hashes each to produce leaves (testing)
//! - `build_from_leaf_hashes()`: takes pre-computed leaf hashes directly (production)

use ark_crypto_primitives::sponge::poseidon::PoseidonConfig;
use ark_crypto_primitives::sponge::Absorb;
use ark_ff::PrimeField;

use crate::poseidon::{poseidon_hash_one, poseidon_hash_two};

/// A binary Poseidon Merkle tree of fixed depth.
#[derive(Debug, Clone)]
pub struct PoseidonMerkleTree<F: PrimeField> {
    /// All nodes stored in a flat array. Index 1 = root.
    /// For a tree of depth d, indices [1..2^d-1] are internal,
    /// and [2^d..2^(d+1)-1] are leaves.
    nodes: Vec<F>,
    /// Tree depth.
    depth: usize,
}

/// A Merkle opening proof: the sibling hashes along the path from leaf to root.
#[derive(Debug, Clone)]
pub struct MerkleProof<F: PrimeField> {
    /// Sibling hashes from leaf level (index 0) to root level (index depth-1).
    pub path: Vec<F>,
    /// Leaf index in [0, 2^depth).
    pub leaf_index: usize,
    /// The leaf value (hashed member key).
    pub leaf: F,
}

impl<F: PrimeField + Absorb> PoseidonMerkleTree<F> {
    /// Build a Merkle tree from raw secret key scalars.
    ///
    /// Each scalar is hashed via `Poseidon(scalar)` to produce a leaf.
    /// Callers MUST sort the scalars in a canonical order before calling
    /// (SEP-XXXX Section 2.1: ascending lexicographic byte order of the
    /// corresponding compressed G1 public key representation).
    ///
    /// Empty slots are filled with `F::zero()`.
    pub fn build(
        config: &PoseidonConfig<F>,
        secret_keys: &[F],
        depth: usize,
    ) -> Self {
        let num_leaves = 1 << depth;
        assert!(
            secret_keys.len() <= num_leaves,
            "Too many members ({}) for depth {} (max {})",
            secret_keys.len(),
            depth,
            num_leaves
        );

        // Total nodes: 2^(depth+1). Index 0 unused, 1 = root.
        let total_nodes = 2 * num_leaves;
        let mut nodes = vec![F::zero(); total_nodes];

        // Fill leaf level: hash each secret key scalar, pad with zero
        let leaf_start = num_leaves; // index of first leaf
        for (i, scalar) in secret_keys.iter().enumerate() {
            nodes[leaf_start + i] = poseidon_hash_one(config, scalar);
        }
        // Remaining leaves stay zero (empty slots)

        // Build internal nodes bottom-up
        for i in (1..num_leaves).rev() {
            let left = nodes[2 * i];
            let right = nodes[2 * i + 1];
            nodes[i] = poseidon_hash_two(config, &left, &right);
        }

        Self { nodes, depth }
    }

    /// Build a Merkle tree from pre-computed leaf hashes.
    ///
    /// Use this when members share `Poseidon(sk_i)` values during
    /// registration. The hashes are placed directly at leaf positions
    /// without re-hashing.
    ///
    /// Callers MUST sort the leaf hashes in the same canonical order
    /// as the corresponding members (SEP-XXXX Section 2.1).
    ///
    /// Empty slots are filled with `F::zero()`.
    pub fn build_from_leaf_hashes(
        config: &PoseidonConfig<F>,
        leaf_hashes: &[F],
        depth: usize,
    ) -> Self {
        let num_leaves = 1 << depth;
        assert!(
            leaf_hashes.len() <= num_leaves,
            "Too many members ({}) for depth {} (max {})",
            leaf_hashes.len(),
            depth,
            num_leaves
        );

        let total_nodes = 2 * num_leaves;
        let mut nodes = vec![F::zero(); total_nodes];

        // Place leaf hashes directly (no re-hashing)
        let leaf_start = num_leaves;
        for (i, hash) in leaf_hashes.iter().enumerate() {
            nodes[leaf_start + i] = *hash;
        }
        // Remaining leaves stay zero (empty slots)

        // Build internal nodes bottom-up
        for i in (1..num_leaves).rev() {
            let left = nodes[2 * i];
            let right = nodes[2 * i + 1];
            nodes[i] = poseidon_hash_two(config, &left, &right);
        }

        Self { nodes, depth }
    }

    /// The Merkle root.
    pub fn root(&self) -> F {
        self.nodes[1]
    }

    /// Number of leaves (= 2^depth, including empty slots).
    pub fn num_leaves(&self) -> usize {
        1 << self.depth
    }

    /// Generate an opening proof for the leaf at `index`.
    pub fn prove(&self, index: usize) -> MerkleProof<F> {
        assert!(index < self.num_leaves(), "Leaf index out of range");

        let mut path = Vec::with_capacity(self.depth);
        let mut current = self.num_leaves() + index; // absolute node index

        for _ in 0..self.depth {
            // Sibling is the other child of our parent
            let sibling = if current % 2 == 0 {
                current + 1
            } else {
                current - 1
            };
            path.push(self.nodes[sibling]);
            current /= 2; // move to parent
        }

        MerkleProof {
            path,
            leaf_index: index,
            leaf: self.nodes[self.num_leaves() + index],
        }
    }

    /// Verify an opening proof against the root.
    pub fn verify(
        config: &PoseidonConfig<F>,
        root: &F,
        proof: &MerkleProof<F>,
        depth: usize,
    ) -> bool {
        assert_eq!(proof.path.len(), depth, "Proof path length mismatch");

        let mut current = proof.leaf;
        let mut index = proof.leaf_index;

        for sibling in &proof.path {
            current = if index % 2 == 0 {
                // Current is left child
                poseidon_hash_two(config, &current, sibling)
            } else {
                // Current is right child
                poseidon_hash_two(config, sibling, &current)
            };
            index /= 2;
        }

        current == *root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poseidon::{poseidon_config, poseidon_hash_one};
    use ark_bls12_381::Fr;
    use ark_ff::{One, Zero};

    fn make_config() -> PoseidonConfig<Fr> {
        poseidon_config::<Fr>()
    }

    #[test]
    fn test_build_empty_tree() {
        let config = make_config();
        let tree = PoseidonMerkleTree::build(&config, &[], 5);
        assert_eq!(tree.num_leaves(), 32);
        // Root should be deterministic for an all-zero tree
        let root = tree.root();
        let tree2 = PoseidonMerkleTree::build(&config, &[], 5);
        assert_eq!(root, tree2.root());
    }

    #[test]
    fn test_build_single_member() {
        let config = make_config();
        let sk = Fr::from(42u64);
        let tree = PoseidonMerkleTree::build(&config, &[sk], 5);
        assert_ne!(tree.root(), Fr::zero());
    }

    #[test]
    fn test_root_changes_with_different_members() {
        let config = make_config();
        let tree_a = PoseidonMerkleTree::build(&config, &[Fr::from(1u64)], 5);
        let tree_b = PoseidonMerkleTree::build(&config, &[Fr::from(2u64)], 5);
        assert_ne!(tree_a.root(), tree_b.root());
    }

    #[test]
    fn test_root_changes_with_member_order() {
        let config = make_config();
        let keys_ab = vec![Fr::from(1u64), Fr::from(2u64)];
        let keys_ba = vec![Fr::from(2u64), Fr::from(1u64)];
        let tree_ab = PoseidonMerkleTree::build(&config, &keys_ab, 5);
        let tree_ba = PoseidonMerkleTree::build(&config, &keys_ba, 5);
        assert_ne!(
            tree_ab.root(),
            tree_ba.root(),
            "Different member order must produce different roots (sort enforcement)"
        );
    }

    #[test]
    fn test_prove_and_verify_first_leaf() {
        let config = make_config();
        let keys = vec![Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];
        let tree = PoseidonMerkleTree::build(&config, &keys, 5);
        let root = tree.root();

        let proof = tree.prove(0);
        assert!(PoseidonMerkleTree::verify(&config, &root, &proof, 5));
    }

    #[test]
    fn test_prove_and_verify_all_members() {
        let config = make_config();
        let keys: Vec<Fr> = (1..=5).map(|i| Fr::from(i as u64)).collect();
        let tree = PoseidonMerkleTree::build(&config, &keys, 5);
        let root = tree.root();

        for i in 0..keys.len() {
            let proof = tree.prove(i);
            assert!(
                PoseidonMerkleTree::verify(&config, &root, &proof, 5),
                "Proof for member {} failed",
                i
            );
        }
    }

    #[test]
    fn test_verify_rejects_wrong_root() {
        let config = make_config();
        let keys = vec![Fr::from(10u64)];
        let tree = PoseidonMerkleTree::build(&config, &keys, 5);

        let proof = tree.prove(0);
        let wrong_root = Fr::from(9999u64);
        assert!(!PoseidonMerkleTree::verify(
            &config,
            &wrong_root,
            &proof,
            5
        ));
    }

    #[test]
    fn test_verify_rejects_tampered_path() {
        let config = make_config();
        let keys = vec![Fr::from(10u64), Fr::from(20u64)];
        let tree = PoseidonMerkleTree::build(&config, &keys, 5);
        let root = tree.root();

        let mut proof = tree.prove(0);
        proof.path[0] = Fr::one(); // tamper with first sibling
        assert!(!PoseidonMerkleTree::verify(&config, &root, &proof, 5));
    }

    #[test]
    fn test_verify_rejects_wrong_leaf() {
        let config = make_config();
        let keys = vec![Fr::from(10u64)];
        let tree = PoseidonMerkleTree::build(&config, &keys, 5);
        let root = tree.root();

        let mut proof = tree.prove(0);
        proof.leaf = Fr::from(999u64); // wrong leaf value
        assert!(!PoseidonMerkleTree::verify(&config, &root, &proof, 5));
    }

    #[test]
    fn test_empty_slot_proof_verifies() {
        let config = make_config();
        let keys = vec![Fr::from(1u64)]; // only slot 0 filled
        let tree = PoseidonMerkleTree::build(&config, &keys, 5);
        let root = tree.root();

        // Slot 1 is empty (zero leaf)
        let proof = tree.prove(1);
        assert!(PoseidonMerkleTree::verify(&config, &root, &proof, 5));
    }

    #[test]
    fn test_different_depths() {
        let config = make_config();
        let keys = vec![Fr::from(1u64), Fr::from(2u64)];

        for depth in [5, 8, 11] {
            let tree = PoseidonMerkleTree::build(&config, &keys, depth);
            let root = tree.root();
            assert_eq!(tree.num_leaves(), 1 << depth);

            for i in 0..keys.len() {
                let proof = tree.prove(i);
                assert_eq!(proof.path.len(), depth);
                assert!(PoseidonMerkleTree::verify(&config, &root, &proof, depth));
            }
        }
    }

    #[test]
    fn test_full_tree() {
        let config = make_config();
        // Fill all 32 leaves
        let keys: Vec<Fr> = (1..=32).map(|i| Fr::from(i as u64)).collect();
        let tree = PoseidonMerkleTree::build(&config, &keys, 5);
        let root = tree.root();

        for i in 0..32 {
            let proof = tree.prove(i);
            assert!(PoseidonMerkleTree::verify(&config, &root, &proof, 5));
        }
    }

    #[test]
    #[should_panic(expected = "Too many members")]
    fn test_too_many_members_panics() {
        let config = make_config();
        let keys: Vec<Fr> = (1..=33).map(|i| Fr::from(i as u64)).collect();
        PoseidonMerkleTree::build(&config, &keys, 5); // max 32
    }

    #[test]
    #[should_panic(expected = "Leaf index out of range")]
    fn test_prove_out_of_range_panics() {
        let config = make_config();
        let tree = PoseidonMerkleTree::build(&config, &[Fr::one()], 5);
        tree.prove(32); // max index is 31
    }

    // === Tests for build_from_leaf_hashes ===

    #[test]
    fn test_build_from_leaf_hashes_matches_build() {
        let config = make_config();
        let keys = vec![Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];

        // build() hashes each key to make a leaf
        let tree_a = PoseidonMerkleTree::build(&config, &keys, 5);

        // Manually compute leaf hashes and use build_from_leaf_hashes()
        let leaf_hashes: Vec<Fr> = keys.iter().map(|k| poseidon_hash_one(&config, k)).collect();
        let tree_b = PoseidonMerkleTree::build_from_leaf_hashes(&config, &leaf_hashes, 5);

        assert_eq!(
            tree_a.root(),
            tree_b.root(),
            "build() and build_from_leaf_hashes() must produce the same tree"
        );
    }

    #[test]
    fn test_build_from_leaf_hashes_proof_verifies() {
        let config = make_config();
        let keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let leaf_hashes: Vec<Fr> = keys.iter().map(|k| poseidon_hash_one(&config, k)).collect();

        let tree = PoseidonMerkleTree::build_from_leaf_hashes(&config, &leaf_hashes, 5);
        let root = tree.root();

        for i in 0..leaf_hashes.len() {
            let proof = tree.prove(i);
            assert!(PoseidonMerkleTree::verify(&config, &root, &proof, 5));
        }
    }

    #[test]
    fn test_build_from_leaf_hashes_empty() {
        let config = make_config();
        let tree = PoseidonMerkleTree::build_from_leaf_hashes(&config, &[], 5);
        // All-zero tree from build_from_leaf_hashes should match all-zero build()
        // Note: build() with empty input also produces all-zero leaves
        let tree_empty = PoseidonMerkleTree::build(&config, &[], 5);
        assert_eq!(tree.root(), tree_empty.root());
    }

    #[test]
    #[should_panic(expected = "Too many members")]
    fn test_build_from_leaf_hashes_too_many() {
        let config = make_config();
        let hashes: Vec<Fr> = (1..=33).map(|i| Fr::from(i as u64)).collect();
        PoseidonMerkleTree::build_from_leaf_hashes(&config, &hashes, 5);
    }
}
