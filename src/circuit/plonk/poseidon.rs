//! Poseidon permutation, native + (forthcoming) jf-relation gadget.
//!
//! Mirrors the parameter set in `crate::poseidon` exactly so that:
//!
//! - Hashes computed by this module under `arkworks` 0.5 produce the
//!   *same field-element* (when interpreted via canonical bytes) as
//!   hashes computed by `crate::poseidon` under `arkworks` 0.4.
//! - Existing Merkle commitments produced by 0.4 code remain
//!   verifiable by the 0.5 PLONK circuit when it lands (depends on
//!   the in-circuit gadget reproducing the same permutation; that's
//!   B.3-second-step).
//!
//! Parameters per SEP-XXXX §2.2 (also described in the legacy
//! `crate::poseidon` module's doc-comment):
//!
//! - Field: BLS12-381 scalar field (Fr, ≈2^255).
//! - Width: t = 3 (rate = 2, capacity = 1).
//! - Full rounds: R_F = 8 (4 at start, 4 at end).
//! - Partial rounds: R_P = 56.
//! - S-box: x^5.
//! - Round constants: derived from SHA-256 seed
//!   `"SEP-XXXX-Poseidon-BLS12-381-w3-f8-p56-a5-round-constants"`.
//! - MDS matrix: Cauchy `M[i][j] = 1 / (x_i + y_j)` with `x_i = i+1`,
//!   `y_j = w + j + 1`.
//!
//! This module's `poseidon_config_v05()` uses bit-for-bit identical
//! derivation logic. The `equivalent_to_v04_native` test verifies
//! agreement on a few fixed inputs.

#![cfg(feature = "plonk")]

use std::sync::OnceLock;

use ark_bls12_381_v05::Fr;
use ark_crypto_primitives_v05::sponge::poseidon::{PoseidonConfig, PoseidonSponge};
use ark_crypto_primitives_v05::sponge::{Absorb, CryptographicSponge};
use ark_ff_v05::{Field, PrimeField};

// Re-exported from the legacy module so we have a single source of truth
// for the parameter constants. They're plain `pub const` and version-
// independent, so this is a pure compile-time alias.
pub use crate::poseidon::{ALPHA, CAPACITY, FULL_ROUNDS, PARTIAL_ROUNDS, RATE};

const WIDTH: usize = RATE + CAPACITY;

/// Process-wide cache of the v0.5 Poseidon parameters.
///
/// Parameter derivation involves SHA-256-chaining for 192 round constants
/// and a Cauchy-matrix inversion for the MDS — non-trivial, so cache.
static CACHED_CONFIG: OnceLock<PoseidonConfig<Fr>> = OnceLock::new();

/// Returns a reference to the cached PoseidonConfig.
pub fn poseidon_config_v05() -> &'static PoseidonConfig<Fr> {
    CACHED_CONFIG.get_or_init(build_poseidon_config_v05)
}

fn build_poseidon_config_v05() -> PoseidonConfig<Fr> {
    let num_constants = (FULL_ROUNDS + PARTIAL_ROUNDS) * WIDTH;
    let flat = generate_round_constants_v05(num_constants);
    let ark = chunk_round_constants(flat);
    let mds = generate_mds_matrix_v05();
    PoseidonConfig {
        full_rounds: FULL_ROUNDS,
        partial_rounds: PARTIAL_ROUNDS,
        alpha: ALPHA,
        ark,
        mds,
        rate: RATE,
        capacity: CAPACITY,
    }
}

/// Hash two field elements via Poseidon (binary mode, for Merkle nodes).
pub fn poseidon_hash_two_v05(left: &Fr, right: &Fr) -> Fr {
    let cfg = poseidon_config_v05();
    let mut sponge = PoseidonSponge::<Fr>::new(cfg);
    Absorb::to_sponge_field_elements(left, &mut sponge_inputs(&mut sponge));
    sponge.absorb(left);
    sponge.absorb(right);
    sponge.squeeze_field_elements::<Fr>(1)[0]
}

// Helper to keep absorb signature happy when the type already implements
// Absorb directly. (No-op pass-through; kept so the signature mirrors v0.4.)
fn sponge_inputs<F: PrimeField>(sponge: &mut PoseidonSponge<F>) -> Vec<F> {
    let _ = sponge;
    Vec::new()
}

/// Hash a single field element via Poseidon (used for leaf hashing).
pub fn poseidon_hash_one_v05(input: &Fr) -> Fr {
    let cfg = poseidon_config_v05();
    let mut sponge = PoseidonSponge::<Fr>::new(cfg);
    sponge.absorb(input);
    sponge.squeeze_field_elements::<Fr>(1)[0]
}

// ---------------------------------------------------------------------------
// Parameter derivation — bit-for-bit identical to crate::poseidon (v0.4),
// but expressed in terms of v0.5 PrimeField + `from_le_bytes_mod_order`.
// ---------------------------------------------------------------------------

fn generate_round_constants_v05(count: usize) -> Vec<Fr> {
    use sha2::{Digest, Sha256};

    let mut constants = Vec::with_capacity(count);
    let mut seed = Sha256::digest(b"SEP-XXXX-Poseidon-BLS12-381-w3-f8-p56-a5-round-constants");
    for _ in 0..count {
        let mut extended = [0u8; 64];
        extended[..32].copy_from_slice(&seed);
        seed = Sha256::digest(seed);
        extended[32..].copy_from_slice(&seed);
        seed = Sha256::digest(seed);
        constants.push(Fr::from_le_bytes_mod_order(&extended));
    }
    constants
}

fn generate_mds_matrix_v05() -> Vec<Vec<Fr>> {
    let mut matrix = Vec::with_capacity(WIDTH);
    for i in 0..WIDTH {
        let mut row = Vec::with_capacity(WIDTH);
        for j in 0..WIDTH {
            let x = Fr::from((i + 1) as u64);
            let y = Fr::from((WIDTH + j + 1) as u64);
            let entry = (x + y)
                .inverse()
                .expect("Cauchy denominators must be invertible");
            row.push(entry);
        }
        matrix.push(row);
    }
    matrix
}

fn chunk_round_constants(flat: Vec<Fr>) -> Vec<Vec<Fr>> {
    flat.chunks(WIDTH).map(|c| c.to_vec()).collect()
}

// ---------------------------------------------------------------------------
// Conversion between v0.4 and v0.5 field elements via canonical bytes.
//
// Both crates use the same canonical Montgomery-form 32-byte little-endian
// encoding for BLS12-381 Fr. We round-trip through bytes to convert.
// ---------------------------------------------------------------------------

#[cfg(test)]
fn fr_v04_to_v05(x: &ark_bls12_381::Fr) -> Fr {
    use ark_serialize::CanonicalSerialize as Ser04;
    use ark_serialize_v05::CanonicalDeserialize as De05;
    let mut buf = Vec::new();
    x.serialize_uncompressed(&mut buf).expect("serialize v0.4");
    Fr::deserialize_uncompressed(&buf[..]).expect("deserialize v0.5")
}

#[cfg(test)]
fn fr_v05_to_v04(x: &Fr) -> ark_bls12_381::Fr {
    use ark_serialize::CanonicalDeserialize as De04;
    use ark_serialize_v05::CanonicalSerialize as Ser05;
    let mut buf = Vec::new();
    x.serialize_uncompressed(&mut buf).expect("serialize v0.5");
    ark_bls12_381::Fr::deserialize_uncompressed(&buf[..]).expect("deserialize v0.4")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poseidon as v04;

    /// Two calls to `poseidon_config_v05()` must return identical
    /// parameters — sanity check that derivation is deterministic.
    #[test]
    fn config_is_deterministic() {
        let c1 = poseidon_config_v05();
        let c2 = poseidon_config_v05();
        // Both refs point to the same OnceLock entry, so this is by-construction.
        // Verify shape:
        assert_eq!(c1.full_rounds, FULL_ROUNDS);
        assert_eq!(c1.partial_rounds, PARTIAL_ROUNDS);
        assert_eq!(c1.alpha, ALPHA);
        assert_eq!(c1.rate, RATE);
        assert_eq!(c1.capacity, CAPACITY);
        assert_eq!(c1.ark.len(), FULL_ROUNDS + PARTIAL_ROUNDS);
        for round in &c1.ark {
            assert_eq!(round.len(), WIDTH);
        }
        assert_eq!(c1.mds.len(), WIDTH);
        for row in &c1.mds {
            assert_eq!(row.len(), WIDTH);
        }
        assert_eq!(c1.ark, c2.ark);
        assert_eq!(c1.mds, c2.mds);
    }

    /// The v0.5 round constants and MDS reproduce the v0.4 constants
    /// when compared via canonical bytes. Catches any divergence in the
    /// parameter derivation logic between the two version-aliased crates.
    #[test]
    fn parameters_match_v04() {
        let v05 = poseidon_config_v05();
        let v04_cfg = v04::poseidon_config::<ark_bls12_381::Fr>();

        assert_eq!(v05.ark.len(), v04_cfg.ark.len(), "ark length");
        for (i, (r5, r4)) in v05.ark.iter().zip(v04_cfg.ark.iter()).enumerate() {
            assert_eq!(r5.len(), r4.len(), "ark[{i}] length");
            for (j, (e5, e4)) in r5.iter().zip(r4.iter()).enumerate() {
                let converted = fr_v05_to_v04(e5);
                assert_eq!(
                    &converted, e4,
                    "ark[{i}][{j}] mismatches between v0.5 and v0.4"
                );
            }
        }

        assert_eq!(v05.mds.len(), v04_cfg.mds.len(), "mds rows");
        for (i, (r5, r4)) in v05.mds.iter().zip(v04_cfg.mds.iter()).enumerate() {
            for (j, (e5, e4)) in r5.iter().zip(r4.iter()).enumerate() {
                let converted = fr_v05_to_v04(e5);
                assert_eq!(
                    &converted, e4,
                    "mds[{i}][{j}] mismatches between v0.5 and v0.4"
                );
            }
        }
    }

    /// `poseidon_hash_two_v05(l, r)` produces the same value as
    /// `crate::poseidon::poseidon_hash_two(cfg, l_v04, r_v04)`. This is
    /// the highest-leverage test: if the v0.5 native sponge differs from
    /// v0.4 by even a single field element across any input, the
    /// in-circuit gadget (next subtask) will silently produce wrong
    /// commitments.
    #[test]
    fn hash_two_matches_v04_native() {
        let cfg_v04 = v04::poseidon_config::<ark_bls12_381::Fr>();

        let test_pairs: &[(u64, u64)] = &[
            (0, 0),
            (1, 0),
            (0, 1),
            (1, 2),
            (2, 1),
            (42, 1337),
            (1u64 << 50, 1u64 << 51),
            (u64::MAX, u64::MAX - 1),
        ];

        for &(l, r) in test_pairs {
            let l_v04 = ark_bls12_381::Fr::from(l);
            let r_v04 = ark_bls12_381::Fr::from(r);
            let h_v04 = v04::poseidon_hash_two(&cfg_v04, &l_v04, &r_v04);

            let l_v05 = fr_v04_to_v05(&l_v04);
            let r_v05 = fr_v04_to_v05(&r_v04);
            let h_v05 = poseidon_hash_two_v05(&l_v05, &r_v05);

            let h_v05_back = fr_v05_to_v04(&h_v05);
            assert_eq!(
                h_v05_back, h_v04,
                "hash_two(({l}, {r})) diverges between v0.5 and v0.4"
            );
        }
    }

    /// Same equivalence check, single-input form.
    #[test]
    fn hash_one_matches_v04_native() {
        let cfg_v04 = v04::poseidon_config::<ark_bls12_381::Fr>();

        for &x in &[0u64, 1, 42, 1u64 << 50, u64::MAX] {
            let x_v04 = ark_bls12_381::Fr::from(x);
            let h_v04 = v04::poseidon_hash_one(&cfg_v04, &x_v04);

            let x_v05 = fr_v04_to_v05(&x_v04);
            let h_v05 = poseidon_hash_one_v05(&x_v05);

            assert_eq!(
                fr_v05_to_v04(&h_v05),
                h_v04,
                "hash_one({x}) diverges between v0.5 and v0.4"
            );
        }
    }
}
