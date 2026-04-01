//! Commitment construction per SEP-XXXX Section 2.3.
//!
//! Two variants:
//!
//! **Variant A (SHA-256):**
//! `commitment = SHA-256(poseidon_root || epoch || salt)`
//! - poseidon_root: 32 bytes, big-endian field element
//! - epoch: u64, big-endian, 8 bytes
//! - salt: 32 bytes
//! - Total preimage: 72 bytes (fixed)
//!
//! **Variant B (Poseidon-only, used by the circuit):**
//! `commitment = Poseidon(Poseidon(poseidon_root, epoch), salt)`
//! - All values are BLS12-381 scalar field elements
//! - Salt is converted via `from_le_bytes_mod_order` (~255 bits effective entropy)

use ark_crypto_primitives::sponge::poseidon::PoseidonConfig;
use ark_crypto_primitives::sponge::Absorb;
use ark_ff::PrimeField;
use sha2::{Sha256, Digest};

use crate::poseidon::poseidon_hash_two;

/// Byte length of the commitment.
pub const COMMITMENT_LEN: usize = 32;

/// Byte length of the salt.
pub const SALT_LEN: usize = 32;

/// Byte length of the full commitment preimage.
pub const PREIMAGE_LEN: usize = 32 + 8 + 32; // poseidon_root + epoch + salt = 72

/// A 32-byte commitment value.
pub type Commitment = [u8; COMMITMENT_LEN];

/// A 32-byte salt.
pub type Salt = [u8; SALT_LEN];

/// Compute the on-chain commitment:
/// `SHA-256(poseidon_root_bytes || epoch_be || salt)`
pub fn compute_commitment<F: PrimeField>(
    poseidon_root: &F,
    epoch: u64,
    salt: &Salt,
) -> Commitment {
    let preimage = build_preimage(poseidon_root, epoch, salt);
    let hash = Sha256::digest(&preimage);
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash);
    result
}

/// Build the 72-byte commitment preimage.
pub fn build_preimage<F: PrimeField>(
    poseidon_root: &F,
    epoch: u64,
    salt: &Salt,
) -> [u8; PREIMAGE_LEN] {
    let mut preimage = [0u8; PREIMAGE_LEN];

    // poseidon_root: serialize to big-endian 32 bytes
    let root_bytes = field_to_bytes_be(poseidon_root);
    preimage[0..32].copy_from_slice(&root_bytes);

    // epoch: u64 big-endian
    preimage[32..40].copy_from_slice(&epoch.to_be_bytes());

    // salt: 32 bytes
    preimage[40..72].copy_from_slice(salt);

    preimage
}

/// Convert a field element to 32 big-endian bytes.
/// The field element is first serialized to little-endian bytes,
/// then reversed to big-endian.
pub fn field_to_bytes_be<F: PrimeField>(f: &F) -> [u8; 32] {
    let mut le_bytes = Vec::new();
    // ark_ff serializes to little-endian
    f.serialize_uncompressed(&mut le_bytes)
        .expect("Field element serialization should not fail");

    let mut be_bytes = [0u8; 32];
    let len = le_bytes.len().min(32);
    // Copy and reverse for big-endian
    for i in 0..len {
        be_bytes[31 - i] = le_bytes[i];
    }
    be_bytes
}

/// Convert 32 big-endian bytes back to a field element.
///
/// For trusted internal inputs only. For untrusted FFI inputs, use
/// [`bytes_be_to_field_checked`] which rejects non-canonical encodings.
pub fn bytes_be_to_field<F: PrimeField>(bytes: &[u8; 32]) -> F {
    let mut le_bytes = [0u8; 32];
    for i in 0..32 {
        le_bytes[i] = bytes[31 - i];
    }
    F::from_le_bytes_mod_order(&le_bytes)
}

/// Convert 32 big-endian bytes to a field element, rejecting non-canonical
/// encodings (values >= field modulus).
///
/// This prevents two different byte representations from silently mapping
/// to the same field element, which is a soundness concern at FFI boundaries.
pub fn bytes_be_to_field_checked<F: PrimeField>(bytes: &[u8; 32]) -> Result<F, String> {
    let result = bytes_be_to_field::<F>(bytes);
    let roundtrip = field_to_bytes_be(&result);
    if bytes != &roundtrip {
        return Err(format!(
            "non-canonical field element: input bytes were reduced modulo the field order"
        ));
    }
    Ok(result)
}

/// Verify a commitment against known inputs.
pub fn verify_commitment<F: PrimeField>(
    commitment: &Commitment,
    poseidon_root: &F,
    epoch: u64,
    salt: &Salt,
) -> bool {
    let computed = compute_commitment(poseidon_root, epoch, salt);
    computed == *commitment
}

/// Compute the Poseidon-only commitment (Variant B):
/// `Poseidon(Poseidon(poseidon_root, epoch), salt)`
///
/// This is the binding used inside the ZK circuit. The salt is converted
/// to a field element via `from_le_bytes_mod_order`, which reduces modulo
/// the BLS12-381 scalar field order r (~2^255). This means ~1 bit of the
/// 256-bit salt entropy is lost for ~50% of random salts. The remaining
/// ~255 bits are cryptographically sufficient.
pub fn compute_poseidon_commitment<F: PrimeField + Absorb>(
    config: &PoseidonConfig<F>,
    poseidon_root: &F,
    epoch: u64,
    salt: &Salt,
) -> F {
    let epoch_fr = F::from(epoch);
    let salt_fr = F::from_le_bytes_mod_order(salt);
    let step = poseidon_hash_two(config, poseidon_root, &epoch_fr);
    poseidon_hash_two(config, &step, &salt_fr)
}

/// Verify a Poseidon-only commitment (Variant B) against known inputs.
pub fn verify_poseidon_commitment<F: PrimeField + Absorb>(
    commitment: &F,
    config: &PoseidonConfig<F>,
    poseidon_root: &F,
    epoch: u64,
    salt: &Salt,
) -> bool {
    let computed = compute_poseidon_commitment(config, poseidon_root, epoch, salt);
    computed == *commitment
}

/// Generate a random salt using a provided RNG.
pub fn generate_salt<R: rand::Rng>(rng: &mut R) -> Salt {
    let mut salt = [0u8; SALT_LEN];
    rng.fill_bytes(&mut salt);
    salt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poseidon::poseidon_config;
    use ark_bls12_381::Fr;
    use ark_ff::{One, Zero};
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    #[test]
    fn test_commitment_deterministic() {
        let root = Fr::from(12345u64);
        let epoch = 0u64;
        let salt = [0xAAu8; 32];

        let c1 = compute_commitment(&root, epoch, &salt);
        let c2 = compute_commitment(&root, epoch, &salt);
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_commitment_changes_with_root() {
        let salt = [0xAAu8; 32];
        let c1 = compute_commitment(&Fr::from(1u64), 0, &salt);
        let c2 = compute_commitment(&Fr::from(2u64), 0, &salt);
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_commitment_changes_with_epoch() {
        let root = Fr::from(1u64);
        let salt = [0xAAu8; 32];
        let c1 = compute_commitment(&root, 0, &salt);
        let c2 = compute_commitment(&root, 1, &salt);
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_commitment_changes_with_salt() {
        let root = Fr::from(1u64);
        let salt_a = [0xAAu8; 32];
        let salt_b = [0xBBu8; 32];
        let c1 = compute_commitment(&root, 0, &salt_a);
        let c2 = compute_commitment(&root, 0, &salt_b);
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_verify_commitment_valid() {
        let root = Fr::from(42u64);
        let epoch = 7u64;
        let salt = [0xCCu8; 32];
        let commitment = compute_commitment(&root, epoch, &salt);
        assert!(verify_commitment(&commitment, &root, epoch, &salt));
    }

    #[test]
    fn test_verify_commitment_rejects_wrong_epoch() {
        let root = Fr::from(42u64);
        let salt = [0xCCu8; 32];
        let commitment = compute_commitment(&root, 7, &salt);
        assert!(!verify_commitment(&commitment, &root, 8, &salt));
    }

    #[test]
    fn test_preimage_length() {
        let root = Fr::one();
        let salt = [0u8; 32];
        let preimage = build_preimage(&root, 0, &salt);
        assert_eq!(preimage.len(), PREIMAGE_LEN);
        assert_eq!(preimage.len(), 72);
    }

    #[test]
    fn test_field_bytes_roundtrip() {
        let original = Fr::from(9999999u64);
        let be_bytes = field_to_bytes_be(&original);
        let recovered: Fr = bytes_be_to_field(&be_bytes);
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_field_bytes_roundtrip_zero() {
        let be_bytes = field_to_bytes_be(&Fr::zero());
        let recovered: Fr = bytes_be_to_field(&be_bytes);
        assert_eq!(Fr::zero(), recovered);
    }

    #[test]
    fn test_field_bytes_roundtrip_one() {
        let be_bytes = field_to_bytes_be(&Fr::one());
        let recovered: Fr = bytes_be_to_field(&be_bytes);
        assert_eq!(Fr::one(), recovered);
    }

    #[test]
    fn test_generate_salt_random() {
        let mut rng = ChaCha20Rng::seed_from_u64(42);
        let salt1 = generate_salt(&mut rng);
        let salt2 = generate_salt(&mut rng);
        assert_ne!(salt1, salt2);
    }

    #[test]
    fn test_preimage_structure() {
        // Verify the byte layout: root(32) || epoch(8) || salt(32)
        let root = Fr::from(1u64);
        let epoch = 0x0102030405060708u64;
        let salt = [0xFFu8; 32];

        let preimage = build_preimage(&root, epoch, &salt);

        // Check epoch bytes are big-endian at offset 32
        assert_eq!(&preimage[32..40], &epoch.to_be_bytes());

        // Check salt at offset 40
        assert_eq!(&preimage[40..72], &salt);
    }

    #[test]
    fn test_commitment_is_32_bytes() {
        let commitment = compute_commitment(&Fr::one(), 0, &[0u8; 32]);
        assert_eq!(commitment.len(), 32);
    }

    #[test]
    fn test_zero_root_zero_epoch_nonzero_commitment() {
        // Even with zero root and epoch, commitment should be non-trivial
        // because SHA-256(zeros) != zeros
        let commitment = compute_commitment(&Fr::zero(), 0, &[0u8; 32]);
        assert_ne!(commitment, [0u8; 32]);
    }

    // === Poseidon commitment (Variant B) tests ===

    #[test]
    fn test_poseidon_commitment_deterministic() {
        let config = poseidon_config::<Fr>();
        let root = Fr::from(12345u64);
        let salt = [0xAAu8; 32];
        let c1 = compute_poseidon_commitment(&config, &root, 0, &salt);
        let c2 = compute_poseidon_commitment(&config, &root, 0, &salt);
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_poseidon_commitment_changes_with_root() {
        let config = poseidon_config::<Fr>();
        let salt = [0xAAu8; 32];
        let c1 = compute_poseidon_commitment(&config, &Fr::from(1u64), 0, &salt);
        let c2 = compute_poseidon_commitment(&config, &Fr::from(2u64), 0, &salt);
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_poseidon_commitment_changes_with_epoch() {
        let config = poseidon_config::<Fr>();
        let root = Fr::from(1u64);
        let salt = [0xAAu8; 32];
        let c1 = compute_poseidon_commitment(&config, &root, 0, &salt);
        let c2 = compute_poseidon_commitment(&config, &root, 1, &salt);
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_poseidon_commitment_changes_with_salt() {
        let config = poseidon_config::<Fr>();
        let root = Fr::from(1u64);
        let c1 = compute_poseidon_commitment(&config, &root, 0, &[0xAAu8; 32]);
        let c2 = compute_poseidon_commitment(&config, &root, 0, &[0xBBu8; 32]);
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_verify_poseidon_commitment() {
        let config = poseidon_config::<Fr>();
        let root = Fr::from(42u64);
        let salt = [0xCCu8; 32];
        let commitment = compute_poseidon_commitment(&config, &root, 7, &salt);
        assert!(verify_poseidon_commitment(&commitment, &config, &root, 7, &salt));
        assert!(!verify_poseidon_commitment(&commitment, &config, &root, 8, &salt));
    }
}
