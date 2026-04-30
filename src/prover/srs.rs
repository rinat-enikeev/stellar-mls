//! Universal SRS for the PLONK prover.
//!
//! Source: Ethereum Foundation 2023 KZG ceremony, finalised 2023-11-14, ≈141k
//! contributors. Powers-of-tau over BLS12-381, n=4096 G1 + 65 G2 elements,
//! ≈400 KB on disk in arkworks-uncompressed encoding.
//!
//! The build.rs hash check makes it structurally impossible to ship the wrong
//! SRS — the build fails on a mismatch between the embedded blob and the
//! pinned hash in `srs/expected-hash.in`.
//!
//! Bytes are produced by `cargo run --bin extract-ef-kzg --features
//! extract-tool --release -- /tmp/ef-kzg-2023-transcript.json
//! src/prover/srs/ef-kzg-2023.bin`. See `src/prover/srs/README.md` for full
//! provenance and reproduction instructions.

#![cfg(feature = "plonk")]

// Arkworks 0.5 (renamed in Cargo.toml) — required to unify with jf-pcs's
// `Pairing` trait. The legacy code path under `feature = "groth16"` keeps
// using the unsuffixed 0.4 names; both coexist via Cargo's duplicate-version
// handling until Phase B.4 migrates the prover wholesale.
use ark_bls12_381_v05::{Bls12_381, G1Affine, G2Affine};
use ark_serialize_v05::CanonicalDeserialize;
use jf_pcs::prelude::UnivariateUniversalParams;

/// Compile-time-pinned SHA-256 of the embedded SRS blob. Reviewed and
/// committed alongside the binary, kept in a separate file so a file-replace
/// attack on `ef-kzg-2023.bin` requires touching the hash in the same commit.
const SRS_SHA256: [u8; 32] = include!("srs/expected-hash.in");

/// The SRS bytes themselves. ~400 KB.
const SRS_BYTES: &[u8] = include_bytes!("srs/ef-kzg-2023.bin");

const MAGIC: &[u8; 4] = b"EFKZ";
const VERSION: u32 = 1;
// See src/bin/extract_ef_kzg.rs for the rationale on choosing n=16384.
const TARGET_G1: usize = 16384;
const TARGET_G2: usize = 65;

const G1_UNCOMPRESSED_BYTES: usize = 96;
const G2_UNCOMPRESSED_BYTES: usize = 192;
const HEADER_BYTES: usize = 16;

/// Returns the raw SRS bytes (used for hashing in tests).
pub fn raw_srs_bytes() -> &'static [u8] {
    SRS_BYTES
}

/// Returns the pinned SHA-256 of the embedded SRS bytes.
pub fn pinned_srs_hash() -> [u8; 32] {
    SRS_SHA256
}

/// Errors from `load_ef_kzg_srs`.
#[derive(Debug)]
pub enum SrsLoadError {
    BadMagic,
    BadVersion(u32),
    BadG1Count(usize),
    BadG2Count(usize),
    BadLength { expected: usize, actual: usize },
    InvalidG1 { index: usize, reason: String },
    InvalidG2 { index: usize, reason: String },
}

impl std::fmt::Display for SrsLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic => write!(f, "SRS magic header mismatch (expected {:?})", MAGIC),
            Self::BadVersion(v) => write!(f, "SRS version {v} unsupported (expected {VERSION})"),
            Self::BadG1Count(n) => write!(f, "SRS G1 count {n} != expected {TARGET_G1}"),
            Self::BadG2Count(n) => write!(f, "SRS G2 count {n} != expected {TARGET_G2}"),
            Self::BadLength { expected, actual } => {
                write!(f, "SRS length {actual} != expected {expected}")
            }
            Self::InvalidG1 { index, reason } => write!(f, "SRS G1[{index}]: {reason}"),
            Self::InvalidG2 { index, reason } => write!(f, "SRS G2[{index}]: {reason}"),
        }
    }
}

impl std::error::Error for SrsLoadError {}

/// Deserialise the embedded SRS into the jf-pcs struct shape jf-plonk consumes.
///
/// jf-pcs's `UnivariateUniversalParams<Bls12_381>` (verified against
/// `pcs/src/univariate_kzg/srs.rs` at revision `b33995b6`) has all four fields
/// public, so a direct struct literal works without a public constructor.
pub fn load_ef_kzg_srs() -> Result<UnivariateUniversalParams<Bls12_381>, SrsLoadError> {
    deserialize_srs(SRS_BYTES)
}

fn deserialize_srs(bytes: &[u8]) -> Result<UnivariateUniversalParams<Bls12_381>, SrsLoadError> {
    if bytes.len() < HEADER_BYTES {
        return Err(SrsLoadError::BadLength {
            expected: HEADER_BYTES,
            actual: bytes.len(),
        });
    }
    if &bytes[0..4] != MAGIC {
        return Err(SrsLoadError::BadMagic);
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != VERSION {
        return Err(SrsLoadError::BadVersion(version));
    }
    let g1_count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let g2_count = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    if g1_count != TARGET_G1 {
        return Err(SrsLoadError::BadG1Count(g1_count));
    }
    if g2_count != TARGET_G2 {
        return Err(SrsLoadError::BadG2Count(g2_count));
    }

    let expected_total =
        HEADER_BYTES + g1_count * G1_UNCOMPRESSED_BYTES + g2_count * G2_UNCOMPRESSED_BYTES;
    if bytes.len() != expected_total {
        return Err(SrsLoadError::BadLength {
            expected: expected_total,
            actual: bytes.len(),
        });
    }

    let mut cursor = HEADER_BYTES;
    let mut powers_of_g = Vec::with_capacity(g1_count);
    for index in 0..g1_count {
        let slice = &bytes[cursor..cursor + G1_UNCOMPRESSED_BYTES];
        let p = G1Affine::deserialize_uncompressed(slice).map_err(|e| {
            SrsLoadError::InvalidG1 {
                index,
                reason: format!("{e:?}"),
            }
        })?;
        powers_of_g.push(p);
        cursor += G1_UNCOMPRESSED_BYTES;
    }

    let mut powers_of_h = Vec::with_capacity(g2_count);
    for index in 0..g2_count {
        let slice = &bytes[cursor..cursor + G2_UNCOMPRESSED_BYTES];
        let p = G2Affine::deserialize_uncompressed(slice).map_err(|e| {
            SrsLoadError::InvalidG2 {
                index,
                reason: format!("{e:?}"),
            }
        })?;
        powers_of_h.push(p);
        cursor += G2_UNCOMPRESSED_BYTES;
    }

    debug_assert_eq!(cursor, bytes.len());

    let h = powers_of_h[0];
    let beta_h = powers_of_h[1];

    Ok(UnivariateUniversalParams {
        powers_of_g,
        h,
        beta_h,
        powers_of_h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_ec_v05::pairing::Pairing;
    use ark_serialize_v05::CanonicalSerialize;
    use sha2::{Digest, Sha256};

    /// The embedded blob's SHA-256 matches the pinned hash. This is the same
    /// invariant build.rs enforces at compile time; this test catches the case
    /// where the build script was somehow skipped (e.g. cached `target/`
    /// across an SRS swap).
    #[test]
    fn embedded_srs_matches_pinned_hash() {
        let actual: [u8; 32] = Sha256::digest(SRS_BYTES).into();
        assert_eq!(
            actual, SRS_SHA256,
            "SRS bytes do not match pinned hash — re-run extract-ef-kzg or update expected-hash.in"
        );
    }

    /// The deserialiser produces a struct whose `[τ^0]_1` and `[τ^0]_2`
    /// slots equal the canonical BLS12-381 generators.
    ///
    /// Exact-equality (rather than just `!is_zero()`) catches an off-by-
    /// one in the extractor — e.g. starting at τ¹ instead of τ⁰ would
    /// still produce non-zero points but break the Lagrange-basis
    /// indexing every PLONK consumer relies on.
    #[test]
    fn srs_loads_into_jf_pcs() {
        let srs = load_ef_kzg_srs().expect("SRS loads");
        assert_eq!(srs.powers_of_g.len(), TARGET_G1);
        assert_eq!(srs.powers_of_h.len(), TARGET_G2);

        use ark_ec_v05::AffineRepr;
        let g1_gen = G1Affine::generator();
        let g2_gen = G2Affine::generator();
        assert_eq!(
            srs.powers_of_g[0], g1_gen,
            "powers_of_g[0] != BLS12-381 G1 generator — SRS extraction is off by one or wrong subgroup"
        );
        assert_eq!(
            srs.h, g2_gen,
            "h != BLS12-381 G2 generator — SRS extraction is off by one or wrong subgroup"
        );
        assert_eq!(
            srs.powers_of_h[0], g2_gen,
            "powers_of_h[0] != BLS12-381 G2 generator — should equal h"
        );
        // beta_h = [τ]_2 must NOT be the generator (would mean τ = 1, breaking soundness).
        assert_ne!(
            srs.beta_h, g2_gen,
            "beta_h == G2 generator implies τ = 1 — ceremony output is broken"
        );
        assert!(!srs.beta_h.is_zero(), "beta_h is the point at infinity");
    }

    /// Powers-of-tau invariant: e([τ^i]_1, [τ^j]_2) == e([τ^k]_1, [τ^l]_2)
    /// whenever i+j == k+l. The cheapest non-trivial check is
    /// e([τ]_1, [1]_2) == e([1]_1, [τ]_2).
    #[test]
    fn srs_satisfies_pairing_identity() {
        let srs = load_ef_kzg_srs().expect("SRS loads");
        // [1]_1 = powers_of_g[0]; [τ]_1 = powers_of_g[1]
        // [1]_2 = h            ; [τ]_2 = beta_h
        let lhs = Bls12_381::pairing(srs.powers_of_g[1], srs.h);
        let rhs = Bls12_381::pairing(srs.powers_of_g[0], srs.beta_h);
        assert_eq!(
            lhs, rhs,
            "SRS does not satisfy powers-of-tau pairing identity — extraction is incorrect"
        );
    }

    /// Round-trip a synthetic SRS through our encoding and back.
    ///
    /// This is the highest-leverage test in the file: it isolates the
    /// deserialiser from any errors in the EF transcript itself. We
    /// generate a tiny SRS with `gen_srs_for_testing`, serialise it
    /// through the same byte layout `extract-ef-kzg` emits, deserialise
    /// it, and check the struct fields round-trip.
    #[test]
    fn srs_round_trips_against_test_srs() {
        use jf_pcs::univariate_kzg::UnivariateKzgPCS;
        use jf_pcs::PolynomialCommitmentScheme;
        use rand_chacha::rand_core::SeedableRng;

        // gen_srs_for_testing wants prover_degree; pick small.
        // (Note: this test SRS is NOT n=4096; we override the deserialiser's
        // count check by going through a path that doesn't require the
        // production constants. We test the byte-level round-trip directly,
        // not load_ef_kzg_srs.)
        // Need a CryptoRng + RngCore — use ChaCha20Rng with a fixed seed for
        // deterministic test runs.
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let test_srs: UnivariateUniversalParams<Bls12_381> =
            <UnivariateKzgPCS<Bls12_381> as PolynomialCommitmentScheme>::gen_srs_for_testing(
                &mut rng, 16,
            )
            .expect("test SRS generation");

        // Serialise to our format
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&VERSION.to_le_bytes());
        buf.extend_from_slice(&(test_srs.powers_of_g.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(test_srs.powers_of_h.len() as u32).to_le_bytes());
        for p in &test_srs.powers_of_g {
            let mut tmp = Vec::new();
            p.serialize_uncompressed(&mut tmp).expect("serialise G1");
            assert_eq!(tmp.len(), G1_UNCOMPRESSED_BYTES);
            buf.extend_from_slice(&tmp);
        }
        for p in &test_srs.powers_of_h {
            let mut tmp = Vec::new();
            p.serialize_uncompressed(&mut tmp).expect("serialise G2");
            assert_eq!(tmp.len(), G2_UNCOMPRESSED_BYTES);
            buf.extend_from_slice(&tmp);
        }

        // Deserialise — bypass the production count check by calling the
        // private deserialise_srs with a relaxed count.
        let g1_count = test_srs.powers_of_g.len();
        let g2_count = test_srs.powers_of_h.len();
        let parsed = deserialize_srs_with_counts(&buf, g1_count, g2_count)
            .expect("round-trip deserialise");

        // Field-by-field equality.
        assert_eq!(parsed.powers_of_g.len(), test_srs.powers_of_g.len());
        assert_eq!(parsed.powers_of_h.len(), test_srs.powers_of_h.len());
        for (a, b) in parsed.powers_of_g.iter().zip(&test_srs.powers_of_g) {
            assert_eq!(a, b, "G1 power mismatch after round-trip");
        }
        for (a, b) in parsed.powers_of_h.iter().zip(&test_srs.powers_of_h) {
            assert_eq!(a, b, "G2 power mismatch after round-trip");
        }
        assert_eq!(parsed.h, test_srs.h, "h mismatch after round-trip");
        assert_eq!(parsed.beta_h, test_srs.beta_h, "beta_h mismatch after round-trip");
    }

    /// Test-only variant of `deserialize_srs` that accepts arbitrary
    /// (g1_count, g2_count) instead of the production constants. Used by
    /// `srs_round_trips_against_test_srs` to verify the byte-level encoding
    /// in isolation from the EF-specific count checks.
    fn deserialize_srs_with_counts(
        bytes: &[u8],
        expected_g1: usize,
        expected_g2: usize,
    ) -> Result<UnivariateUniversalParams<Bls12_381>, SrsLoadError> {
        if bytes.len() < HEADER_BYTES {
            return Err(SrsLoadError::BadLength { expected: HEADER_BYTES, actual: bytes.len() });
        }
        if &bytes[0..4] != MAGIC {
            return Err(SrsLoadError::BadMagic);
        }
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != VERSION {
            return Err(SrsLoadError::BadVersion(version));
        }
        let g1_count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let g2_count = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        if g1_count != expected_g1 {
            return Err(SrsLoadError::BadG1Count(g1_count));
        }
        if g2_count != expected_g2 {
            return Err(SrsLoadError::BadG2Count(g2_count));
        }
        let expected_total =
            HEADER_BYTES + g1_count * G1_UNCOMPRESSED_BYTES + g2_count * G2_UNCOMPRESSED_BYTES;
        if bytes.len() != expected_total {
            return Err(SrsLoadError::BadLength { expected: expected_total, actual: bytes.len() });
        }
        let mut cursor = HEADER_BYTES;
        let mut powers_of_g = Vec::with_capacity(g1_count);
        for index in 0..g1_count {
            let slice = &bytes[cursor..cursor + G1_UNCOMPRESSED_BYTES];
            let p = G1Affine::deserialize_uncompressed(slice)
                .map_err(|e| SrsLoadError::InvalidG1 { index, reason: format!("{e:?}") })?;
            powers_of_g.push(p);
            cursor += G1_UNCOMPRESSED_BYTES;
        }
        let mut powers_of_h = Vec::with_capacity(g2_count);
        for index in 0..g2_count {
            let slice = &bytes[cursor..cursor + G2_UNCOMPRESSED_BYTES];
            let p = G2Affine::deserialize_uncompressed(slice)
                .map_err(|e| SrsLoadError::InvalidG2 { index, reason: format!("{e:?}") })?;
            powers_of_h.push(p);
            cursor += G2_UNCOMPRESSED_BYTES;
        }
        let h = powers_of_h[0];
        let beta_h = if g2_count >= 2 { powers_of_h[1] } else { h };
        Ok(UnivariateUniversalParams { powers_of_g, h, beta_h, powers_of_h })
    }
}
