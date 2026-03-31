//! Poseidon hash function over the BLS12-381 scalar field.
//!
//! Parameters per SEP-XXXX Section 2.2:
//! - Field: BLS12-381 scalar field (Fr, ~2^255)
//! - Arity: 2 (binary Merkle tree)
//! - Full rounds: 8
//! - Partial rounds: 56
//! - S-box: x^5
//!
//! We use arkworks' built-in Poseidon sponge with these parameters.

use ark_crypto_primitives::sponge::poseidon::{PoseidonConfig, PoseidonSponge};
use ark_crypto_primitives::sponge::{Absorb, CryptographicSponge};
use ark_ff::PrimeField;

/// Number of full rounds (applied at start and end).
pub const FULL_ROUNDS: usize = 8;

/// Number of partial rounds (applied in the middle).
pub const PARTIAL_ROUNDS: usize = 56;

/// Sponge rate (arity for 2-to-1 hashing).
pub const RATE: usize = 2;

/// Sponge capacity.
pub const CAPACITY: usize = 1;

/// Alpha for the S-box: x^ALPHA.
pub const ALPHA: u64 = 5;

/// Generate deterministic Poseidon parameters for a given field.
///
/// In production, these would come from the Poseidon paper's reference
/// generation script for BLS12-381. For now we use a deterministic
/// construction that produces secure round constants and MDS matrix.
pub fn poseidon_config<F: PrimeField + Absorb>() -> PoseidonConfig<F> {
    // Width = rate + capacity
    let width = RATE + CAPACITY;
    let full_rounds = FULL_ROUNDS;
    let partial_rounds = PARTIAL_ROUNDS;

    // Generate round constants deterministically.
    // In production: use the Poseidon reference generation script.
    // Here we derive them from a seed using the field's modulus.
    let num_constants = (full_rounds + partial_rounds) * width;
    let round_constants = generate_round_constants::<F>(num_constants);

    // Generate MDS matrix deterministically.
    let mds_matrix = generate_mds_matrix::<F>(width);

    PoseidonConfig {
        full_rounds,
        partial_rounds,
        alpha: ALPHA,
        ark: chunk_round_constants(round_constants, width),
        mds: mds_matrix,
        rate: RATE,
        capacity: CAPACITY,
    }
}

/// Hash two field elements (binary Poseidon, for Merkle tree nodes).
pub fn poseidon_hash_two<F: PrimeField + Absorb>(
    config: &PoseidonConfig<F>,
    left: &F,
    right: &F,
) -> F {
    let mut sponge = PoseidonSponge::new(config);
    sponge.absorb(left);
    sponge.absorb(right);
    sponge.squeeze_field_elements::<F>(1)[0]
}

/// Hash a single field element (for leaf hashing).
pub fn poseidon_hash_one<F: PrimeField + Absorb>(
    config: &PoseidonConfig<F>,
    input: &F,
) -> F {
    let mut sponge = PoseidonSponge::new(config);
    sponge.absorb(input);
    sponge.squeeze_field_elements::<F>(1)[0]
}

/// Generate deterministic round constants from a seed derived from
/// the string "SEP-XXXX-Poseidon-BLS12-381" using repeated hashing.
fn generate_round_constants<F: PrimeField>(count: usize) -> Vec<F> {
    use sha2::{Sha256, Digest};

    let mut constants = Vec::with_capacity(count);
    // Domain-separated seed includes all Poseidon parameters for reproducibility.
    // Production deployments SHOULD use canonical constants from the Poseidon
    // reference generation script (generate_params_poseidon.sage).
    let mut seed = Sha256::digest(b"SEP-XXXX-Poseidon-BLS12-381-w3-f8-p56-a5-round-constants");

    for _ in 0..count {
        // Extend seed to 64 bytes for uniform field element sampling
        let mut extended = [0u8; 64];
        extended[..32].copy_from_slice(&seed);
        seed = Sha256::digest(&seed);
        extended[32..].copy_from_slice(&seed);
        seed = Sha256::digest(&seed);

        let constant = F::from_le_bytes_mod_order(&extended);
        constants.push(constant);
    }

    constants
}

/// Generate a deterministic MDS (Maximum Distance Separable) matrix.
/// Uses the Cauchy matrix construction: M[i][j] = 1 / (x_i + y_j)
/// where x and y are distinct field elements.
fn generate_mds_matrix<F: PrimeField>(width: usize) -> Vec<Vec<F>> {
    let mut matrix = Vec::with_capacity(width);

    for i in 0..width {
        let mut row = Vec::with_capacity(width);
        for j in 0..width {
            let x = F::from((i + 1) as u64);
            let y = F::from((width + j + 1) as u64);
            // M[i][j] = 1 / (x_i + y_j)
            let sum = x + y;
            let entry = sum.inverse().expect("Cauchy matrix elements must be invertible");
            row.push(entry);
        }
        matrix.push(row);
    }

    matrix
}

/// Chunk a flat list of round constants into per-round vectors.
fn chunk_round_constants<F: Clone>(constants: Vec<F>, width: usize) -> Vec<Vec<F>> {
    constants.chunks(width).map(|c| c.to_vec()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bls12_381::Fr;
    use ark_ff::One;

    #[test]
    fn test_poseidon_config_creation() {
        let config = poseidon_config::<Fr>();
        assert_eq!(config.full_rounds, FULL_ROUNDS);
        assert_eq!(config.partial_rounds, PARTIAL_ROUNDS);
        assert_eq!(config.rate, RATE);
        assert_eq!(config.capacity, CAPACITY);
        assert_eq!(config.alpha, ALPHA);
    }

    #[test]
    fn test_poseidon_hash_two_deterministic() {
        let config = poseidon_config::<Fr>();
        let a = Fr::from(1u64);
        let b = Fr::from(2u64);

        let h1 = poseidon_hash_two(&config, &a, &b);
        let h2 = poseidon_hash_two(&config, &a, &b);
        assert_eq!(h1, h2, "Poseidon must be deterministic");
    }

    #[test]
    fn test_poseidon_hash_two_different_inputs() {
        let config = poseidon_config::<Fr>();
        let a = Fr::from(1u64);
        let b = Fr::from(2u64);
        let c = Fr::from(3u64);

        let h1 = poseidon_hash_two(&config, &a, &b);
        let h2 = poseidon_hash_two(&config, &a, &c);
        assert_ne!(h1, h2, "Different inputs must produce different hashes");
    }

    #[test]
    fn test_poseidon_hash_two_not_commutative() {
        let config = poseidon_config::<Fr>();
        let a = Fr::from(1u64);
        let b = Fr::from(2u64);

        let h1 = poseidon_hash_two(&config, &a, &b);
        let h2 = poseidon_hash_two(&config, &b, &a);
        assert_ne!(h1, h2, "Poseidon(a,b) != Poseidon(b,a) — order matters for Merkle");
    }

    #[test]
    fn test_poseidon_hash_one_deterministic() {
        let config = poseidon_config::<Fr>();
        let a = Fr::one();

        let h1 = poseidon_hash_one(&config, &a);
        let h2 = poseidon_hash_one(&config, &a);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_poseidon_hash_one_preimage_distinct() {
        let config = poseidon_config::<Fr>();
        let a = Fr::from(100u64);
        let b = Fr::from(200u64);

        let h1 = poseidon_hash_one(&config, &a);
        let h2 = poseidon_hash_one(&config, &b);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_round_constants_count() {
        let config = poseidon_config::<Fr>();
        let expected_rounds = FULL_ROUNDS + PARTIAL_ROUNDS;
        assert_eq!(config.ark.len(), expected_rounds);
        for round in &config.ark {
            assert_eq!(round.len(), RATE + CAPACITY);
        }
    }

    #[test]
    fn test_mds_matrix_dimensions() {
        let config = poseidon_config::<Fr>();
        let width = RATE + CAPACITY;
        assert_eq!(config.mds.len(), width);
        for row in &config.mds {
            assert_eq!(row.len(), width);
        }
    }
}
