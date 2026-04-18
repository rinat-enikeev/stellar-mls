//! # SEP-XXXX Circuits
//!
//! Groth16 zero-knowledge proof circuits for the Private Group Membership
//! Registry standard on Stellar/Soroban.
//!
//! ## Architecture
//!
//! The circuit proves three things without revealing any witness data:
//!
//! 1. **Key ownership**: `leaf = Poseidon(sk)` — prover knows the secret key preimage
//! 2. **Merkle membership**: `leaf` is in a Poseidon Merkle tree with root `poseidon_root`
//! 3. **Commitment binding**: `Poseidon(Poseidon(poseidon_root, epoch), salt) == commitment`
//!
//! ## Circuit Tiers
//!
//! | Tier   | MAX_MEMBERS | Tree depth |
//! |--------|-------------|------------|
//! | Small  | 32          | 5          |
//! | Medium | 256         | 8          |
//! | Large  | 2048        | 11         |

pub mod poseidon;
pub mod merkle;
pub mod circuit;
pub mod commitment;
pub mod prover;
pub mod ceremony;
#[cfg(feature = "ffi")]
mod ffi;
#[cfg(feature = "jni")]
mod jni_ffi;

/// Circuit tier definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Up to 32 members, Merkle depth 5
    Small,
    /// Up to 256 members, Merkle depth 8
    Medium,
    /// Up to 2048 members, Merkle depth 11
    Large,
}

impl Tier {
    /// Maximum number of members for this tier.
    pub const fn max_members(&self) -> usize {
        match self {
            Tier::Small => 32,
            Tier::Medium => 256,
            Tier::Large => 2048,
        }
    }

    /// Merkle tree depth (binary tree, so 2^depth = max_members).
    pub const fn depth(&self) -> usize {
        match self {
            Tier::Small => 5,
            Tier::Medium => 8,
            Tier::Large => 11,
        }
    }

    /// Tier identifier stored on-chain.
    pub const fn id(&self) -> u32 {
        match self {
            Tier::Small => 0,
            Tier::Medium => 1,
            Tier::Large => 2,
        }
    }
}

#[cfg(test)]
mod cross_platform_vectors {
    use ark_bls12_381::Fr;
    use ark_ff::PrimeField;
    use crate::commitment::{field_to_bytes_be, compute_commitment, compute_poseidon_commitment};
    use crate::merkle::{compressed_public_key_bytes, CanonicalMember, PoseidonMerkleTree};
    use crate::poseidon::poseidon_config;
    use crate::prover::compute_leaf_hash;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }

    fn from_hex(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn generate_cross_platform_vectors() {
        let config = poseidon_config::<Fr>();

        // === BLS key derivation vectors ===
        let sk1_bytes = [0x42u8; 32];
        let sk1 = Fr::from_be_bytes_mod_order(&sk1_bytes);
        let pk1 = compressed_public_key_bytes(&sk1);
        let leaf1 = compute_leaf_hash(&sk1);
        let leaf1_bytes = field_to_bytes_be(&leaf1);

        let sk2_bytes = [0x01u8; 32];
        let sk2 = Fr::from_be_bytes_mod_order(&sk2_bytes);
        let pk2 = compressed_public_key_bytes(&sk2);
        let leaf2 = compute_leaf_hash(&sk2);
        let leaf2_bytes = field_to_bytes_be(&leaf2);

        // Print all values for JSON update
        eprintln!("BLS Vector 1 (sk=0x42..42):");
        eprintln!("  pk:   {}", hex(&pk1));
        eprintln!("  leaf: {}", hex(&leaf1_bytes));
        eprintln!("BLS Vector 2 (sk=0x01..01):");
        eprintln!("  pk:   {}", hex(&pk2));
        eprintln!("  leaf: {}", hex(&leaf2_bytes));

        // === Merkle root vectors ===
        let members1 = vec![CanonicalMember { public_key_bytes: pk1, leaf_hash: leaf1 }];
        let tree1 = PoseidonMerkleTree::build_from_members(&config, &members1, 5).unwrap();
        let root1_bytes = field_to_bytes_be(&tree1.root());

        let members2 = vec![
            CanonicalMember { public_key_bytes: pk1, leaf_hash: leaf1 },
            CanonicalMember { public_key_bytes: pk2, leaf_hash: leaf2 },
        ];
        let tree2 = PoseidonMerkleTree::build_from_members(&config, &members2, 5).unwrap();
        let root2_bytes = field_to_bytes_be(&tree2.root());

        eprintln!("Merkle root (1 member, depth 5): {}", hex(&root1_bytes));
        eprintln!("Merkle root (2 members, depth 5): {}", hex(&root2_bytes));

        // === Commitment vectors ===
        let salt_zero = [0u8; 32];
        let salt_aa = [0xAA; 32];

        let pos1 = compute_poseidon_commitment(&config, &tree1.root(), 0, &salt_zero);
        let sha1 = compute_commitment(&tree1.root(), 0, &salt_zero);
        let pos2 = compute_poseidon_commitment(&config, &tree2.root(), 5, &salt_aa);
        let sha2 = compute_commitment(&tree2.root(), 5, &salt_aa);

        eprintln!("Poseidon commitment (root1, e=0, s=00): {}", hex(&field_to_bytes_be(&pos1)));
        eprintln!("SHA-256  commitment (root1, e=0, s=00): {}", hex(&sha1));
        eprintln!("Poseidon commitment (root2, e=5, s=AA): {}", hex(&field_to_bytes_be(&pos2)));
        eprintln!("SHA-256  commitment (root2, e=5, s=AA): {}", hex(&sha2));

        // Verify determinism
        assert_eq!(pk1.len(), 48);
        assert_eq!(pk2.len(), 48);
        assert_eq!(leaf1_bytes.len(), 32);
        assert_eq!(leaf2_bytes.len(), 32);
        assert_eq!(root1_bytes.len(), 32);
        assert_eq!(root2_bytes.len(), 32);
    }

    #[test]
    fn verify_cross_platform_vectors() {
        // Verify topic derivation vectors from the JSON file
        use sha2::{Sha256, Digest};

        // topic: SHA-256("sep-topic-v1" || groupSecret)[0..8] as hex
        let check_topic = |secret_hex: &str, expected: &str| {
            let secret = from_hex(secret_hex);
            let mut hasher = Sha256::new();
            hasher.update(b"sep-topic-v1");
            hasher.update(&secret);
            let hash = hasher.finalize();
            let topic = hex(&hash[..8]);
            assert_eq!(topic, expected, "topic mismatch for secret {}", secret_hex);
        };
        check_topic("0000000000000000000000000000000000000000000000000000000000000000", "0c6ce887ec3881f3");
        check_topic("4242424242424242424242424242424242424242424242424242424242424242", "aa263d222a1f2e1f");
        check_topic("ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00", "7df84225883879ff");

        // inbox: SHA-256("sep-inbox-v1" || pubkey)[0..8] as hex
        let check_inbox = |pubkey_hex: &str, expected: &str| {
            let pubkey = from_hex(pubkey_hex);
            let mut hasher = Sha256::new();
            hasher.update(b"sep-inbox-v1");
            hasher.update(&pubkey);
            let hash = hasher.finalize();
            let tag = hex(&hash[..8]);
            assert_eq!(tag, expected, "inbox mismatch for pubkey {}", pubkey_hex);
        };
        check_inbox("0000000000000000000000000000000000000000000000000000000000000000", "f434d35e24e86124");
        check_inbox("4242424242424242424242424242424242424242424242424242424242424242", "c9bcafedc73f4289");
    }
}
