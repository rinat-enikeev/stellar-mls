//! Poseidon Merkle tree for member set commitment.
//!
//! Per SEP-XXXX Section 2.2:
//! - Binary tree of fixed depth `d` determined by circuit tier
//! - Leaves: `Poseidon(secret_key_scalar)` for real members, zero for empty slots
//! - Internal nodes: `Poseidon(left, right)`
//! - Root: `poseidon_root` used in the on-chain commitment
//!
//! ## Tree Depth and Maximum Group Size (M-14)
//!
//! The tree depth is fixed at compile time per circuit tier:
//!
//! | Tier   | Depth | Max Members (2^depth) | Constraint Count |
//! |--------|-------|----------------------|-----------------|
//! | Small  |   5   | 32                   | ~1,910          |
//! | Medium |   8   | 256                  | ~2,630          |
//! | Large  |  11   | 2,048                | ~3,350          |
//!
//! Changing the depth requires regenerating the proving and verification keys
//! (via the ceremony module) and migrating all groups of that tier. The
//! verification keys stored in the Soroban contract must also be updated.
//!
//! Two construction methods:
//! - `build()`: takes raw secret key scalars, hashes each to produce leaves (testing)
//! - `build_from_members()`: takes `{compressed G1 public key, leaf hash}` records
//!   and canonicalizes them internally (production)

use std::fmt;

use ark_bls12_381::{Fr, G1Projective};
use ark_crypto_primitives::sponge::poseidon::PoseidonConfig;
use ark_crypto_primitives::sponge::Absorb;
use ark_ec::{CurveGroup, Group};
use ark_ff::PrimeField;
use ark_serialize::CanonicalSerialize;

use crate::poseidon::{poseidon_hash_one, poseidon_hash_two};

/// Compressed BLS12-381 G1 public key length in bytes.
pub const COMPRESSED_G1_PUBLIC_KEY_LEN: usize = 48;

/// Member material needed to canonicalize a roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalMember<F: PrimeField> {
    /// Canonical sorting key per SEP-XXXX Section 2.1.
    pub public_key_bytes: [u8; COMPRESSED_G1_PUBLIC_KEY_LEN],
    /// `Poseidon(sk)` leaf hash for this member.
    pub leaf_hash: F,
}

/// Errors produced while canonicalizing or building Merkle trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MerkleError {
    TooManyMembers { actual: usize, max: usize },
    DuplicatePublicKey([u8; COMPRESSED_G1_PUBLIC_KEY_LEN]),
}

impl fmt::Display for MerkleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MerkleError::TooManyMembers { actual, max } => {
                write!(f, "too many members ({actual}), max is {max}")
            }
            MerkleError::DuplicatePublicKey(bytes) => {
                write!(f, "duplicate member public key: {}", hex_string(bytes))
            }
        }
    }
}

impl std::error::Error for MerkleError {}

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
    fn build_from_ordered_leaf_hashes(
        config: &PoseidonConfig<F>,
        ordered_leaf_hashes: &[F],
        depth: usize,
    ) -> Result<Self, MerkleError> {
        let num_leaves = 1 << depth;
        if ordered_leaf_hashes.len() > num_leaves {
            return Err(MerkleError::TooManyMembers {
                actual: ordered_leaf_hashes.len(),
                max: num_leaves,
            });
        }

        // Total nodes: 2^(depth+1). Index 0 unused, 1 = root.
        let total_nodes = 2 * num_leaves;
        let mut nodes = vec![F::zero(); total_nodes];

        // Fill leaf level and pad remaining slots with zero.
        let leaf_start = num_leaves;
        for (i, leaf_hash) in ordered_leaf_hashes.iter().enumerate() {
            nodes[leaf_start + i] = *leaf_hash;
        }

        // Build internal nodes bottom-up
        for i in (1..num_leaves).rev() {
            let left = nodes[2 * i];
            let right = nodes[2 * i + 1];
            nodes[i] = poseidon_hash_two(config, &left, &right);
        }

        Ok(Self { nodes, depth })
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

/// Compute a member's compressed G1 public key from its secret scalar.
pub fn compressed_public_key_bytes(secret_key: &Fr) -> [u8; COMPRESSED_G1_PUBLIC_KEY_LEN] {
    let public_key = (G1Projective::generator() * *secret_key).into_affine();
    let mut bytes = Vec::new();
    public_key
        .serialize_compressed(&mut bytes)
        .expect("G1 public key serialization should not fail");
    bytes
        .try_into()
        .expect("compressed BLS12-381 G1 public key must be 48 bytes")
}

/// Canonicalize member records by SEP-XXXX ordering and reject duplicates.
pub fn canonicalize_members(
    members: &[CanonicalMember<Fr>],
) -> Result<Vec<CanonicalMember<Fr>>, MerkleError> {
    let mut ordered = members.to_vec();
    ordered.sort_by(|lhs, rhs| lhs.public_key_bytes.cmp(&rhs.public_key_bytes));

    for pair in ordered.windows(2) {
        if pair[0].public_key_bytes == pair[1].public_key_bytes {
            return Err(MerkleError::DuplicatePublicKey(pair[0].public_key_bytes));
        }
    }

    Ok(ordered)
}

impl PoseidonMerkleTree<Fr> {
    /// Build a Merkle tree from raw secret key scalars.
    ///
    /// The implementation derives compressed G1 public keys, canonicalizes
    /// the roster per SEP-XXXX Section 2.1, rejects duplicates, and then
    /// hashes each secret key into its corresponding leaf.
    pub fn build(
        config: &PoseidonConfig<Fr>,
        secret_keys: &[Fr],
        depth: usize,
    ) -> Result<Self, MerkleError> {
        let members: Vec<CanonicalMember<Fr>> = secret_keys
            .iter()
            .map(|secret_key| CanonicalMember {
                public_key_bytes: compressed_public_key_bytes(secret_key),
                leaf_hash: poseidon_hash_one(config, secret_key),
            })
            .collect();
        Self::build_from_members(config, &members, depth)
    }

    /// Build a Merkle tree from pre-computed leaf hashes plus canonical sort keys.
    ///
    /// Each member contributes:
    /// - `public_key_bytes`: compressed G1 bytes used for deterministic ordering
    /// - `leaf_hash`: `Poseidon(sk)`
    ///
    /// The function sorts internally and rejects duplicate public keys.
    pub fn build_from_members(
        config: &PoseidonConfig<Fr>,
        members: &[CanonicalMember<Fr>],
        depth: usize,
    ) -> Result<Self, MerkleError> {
        let ordered_members = canonicalize_members(members)?;
        let ordered_leaf_hashes: Vec<Fr> = ordered_members.iter().map(|member| member.leaf_hash).collect();
        Self::build_from_ordered_leaf_hashes(config, &ordered_leaf_hashes, depth)
    }
}

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
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
        let tree = PoseidonMerkleTree::build(&config, &[], 5).unwrap();
        assert_eq!(tree.num_leaves(), 32);
        // Root should be deterministic for an all-zero tree
        let root = tree.root();
        let tree2 = PoseidonMerkleTree::build(&config, &[], 5).unwrap();
        assert_eq!(root, tree2.root());
    }

    #[test]
    fn test_build_single_member() {
        let config = make_config();
        let sk = Fr::from(42u64);
        let tree = PoseidonMerkleTree::build(&config, &[sk], 5).unwrap();
        assert_ne!(tree.root(), Fr::zero());
    }

    #[test]
    fn test_root_changes_with_different_members() {
        let config = make_config();
        let tree_a = PoseidonMerkleTree::build(&config, &[Fr::from(1u64)], 5).unwrap();
        let tree_b = PoseidonMerkleTree::build(&config, &[Fr::from(2u64)], 5).unwrap();
        assert_ne!(tree_a.root(), tree_b.root());
    }

    #[test]
    fn test_root_is_canonical_under_member_reordering() {
        let config = make_config();
        let keys_ab = vec![Fr::from(1u64), Fr::from(2u64)];
        let keys_ba = vec![Fr::from(2u64), Fr::from(1u64)];
        let tree_ab = PoseidonMerkleTree::build(&config, &keys_ab, 5).unwrap();
        let tree_ba = PoseidonMerkleTree::build(&config, &keys_ba, 5).unwrap();
        assert_eq!(
            tree_ab.root(),
            tree_ba.root(),
            "Canonical sorting must make the root independent of input ordering"
        );
    }

    #[test]
    fn test_prove_and_verify_first_leaf() {
        let config = make_config();
        let keys = vec![Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
        let root = tree.root();

        let proof = tree.prove(0);
        assert!(PoseidonMerkleTree::verify(&config, &root, &proof, 5));
    }

    #[test]
    fn test_prove_and_verify_all_members() {
        let config = make_config();
        let keys: Vec<Fr> = (1..=5).map(|i| Fr::from(i as u64)).collect();
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
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
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();

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
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
        let root = tree.root();

        let mut proof = tree.prove(0);
        proof.path[0] = Fr::one(); // tamper with first sibling
        assert!(!PoseidonMerkleTree::verify(&config, &root, &proof, 5));
    }

    #[test]
    fn test_verify_rejects_wrong_leaf() {
        let config = make_config();
        let keys = vec![Fr::from(10u64)];
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
        let root = tree.root();

        let mut proof = tree.prove(0);
        proof.leaf = Fr::from(999u64); // wrong leaf value
        assert!(!PoseidonMerkleTree::verify(&config, &root, &proof, 5));
    }

    #[test]
    fn test_empty_slot_proof_verifies() {
        let config = make_config();
        let keys = vec![Fr::from(1u64)]; // only slot 0 filled
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
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
            let tree = PoseidonMerkleTree::build(&config, &keys, depth).unwrap();
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
        let tree = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();
        let root = tree.root();

        for i in 0..32 {
            let proof = tree.prove(i);
            assert!(PoseidonMerkleTree::verify(&config, &root, &proof, 5));
        }
    }

    #[test]
    fn test_too_many_members_rejected() {
        let config = make_config();
        let keys: Vec<Fr> = (1..=33).map(|i| Fr::from(i as u64)).collect();
        let error = PoseidonMerkleTree::build(&config, &keys, 5).unwrap_err();
        assert!(matches!(error, MerkleError::TooManyMembers { actual: 33, max: 32 }));
    }

    #[test]
    fn test_too_many_members_panics() {
        let config = make_config();
        let tree = PoseidonMerkleTree::build(&config, &[Fr::one()], 5).unwrap();
        assert_eq!(tree.num_leaves(), 32);
    }

    #[test]
    #[should_panic(expected = "Leaf index out of range")]
    fn test_prove_out_of_range_panics() {
        let config = make_config();
        let tree = PoseidonMerkleTree::build(&config, &[Fr::one()], 5).unwrap();
        tree.prove(32); // max index is 31
    }

    // === Tests for build_from_members ===

    fn canonical_members_for_keys(config: &PoseidonConfig<Fr>, keys: &[Fr]) -> Vec<CanonicalMember<Fr>> {
        keys.iter()
            .map(|key| CanonicalMember {
                public_key_bytes: compressed_public_key_bytes(key),
                leaf_hash: poseidon_hash_one(config, key),
            })
            .collect()
    }

    #[test]
    fn test_build_from_members_matches_build() {
        let config = make_config();
        let keys = vec![Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];

        // build() hashes each key to make a leaf
        let tree_a = PoseidonMerkleTree::build(&config, &keys, 5).unwrap();

        let members = canonical_members_for_keys(&config, &keys);
        let tree_b = PoseidonMerkleTree::build_from_members(&config, &members, 5).unwrap();

        assert_eq!(
            tree_a.root(),
            tree_b.root(),
            "build() and build_from_members() must produce the same tree"
        );
    }

    #[test]
    fn test_build_from_members_proof_verifies() {
        let config = make_config();
        let keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let members = canonical_members_for_keys(&config, &keys);

        let tree = PoseidonMerkleTree::build_from_members(&config, &members, 5).unwrap();
        let root = tree.root();

        for i in 0..members.len() {
            let proof = tree.prove(i);
            assert!(PoseidonMerkleTree::verify(&config, &root, &proof, 5));
        }
    }

    #[test]
    fn test_build_from_members_empty() {
        let config = make_config();
        let tree = PoseidonMerkleTree::build_from_members(&config, &[], 5).unwrap();
        let tree_empty = PoseidonMerkleTree::build(&config, &[], 5).unwrap();
        assert_eq!(tree.root(), tree_empty.root());
    }

    #[test]
    fn test_build_from_members_too_many() {
        let config = make_config();
        let members = canonical_members_for_keys(
            &config,
            &(1..=33).map(|i| Fr::from(i as u64)).collect::<Vec<_>>(),
        );
        let error = PoseidonMerkleTree::build_from_members(&config, &members, 5).unwrap_err();
        assert!(matches!(error, MerkleError::TooManyMembers { actual: 33, max: 32 }));
    }

    #[test]
    fn test_duplicate_public_keys_rejected() {
        let config = make_config();
        let key = Fr::from(42u64);
        let duplicate_public_key = compressed_public_key_bytes(&key);
        let members = vec![
            CanonicalMember {
                public_key_bytes: duplicate_public_key,
                leaf_hash: poseidon_hash_one(&config, &key),
            },
            CanonicalMember {
                public_key_bytes: duplicate_public_key,
                leaf_hash: poseidon_hash_one(&config, &Fr::from(99u64)),
            },
        ];

        let error = PoseidonMerkleTree::build_from_members(&config, &members, 5).unwrap_err();
        assert!(matches!(error, MerkleError::DuplicatePublicKey(_)));
    }
}
