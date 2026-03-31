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
mod ffi;

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
