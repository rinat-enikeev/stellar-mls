//! Soroban-portable port of `sep-xxxx-circuits::circuit::plonk::vk_format`.
//!
//! Byte-for-byte identical layout and parser logic to the Rust reference.
//! The reference module is oracle-tested against
//! `jf_plonk::VerifyingKey::deserialize_uncompressed`; the byte-equivalence
//! transfers to this port because the parsing logic is copied verbatim.
//!
//! ## Layout (3002 bytes, tier-independent)
//!
//!   Field                                          | offset | length
//!   ──────────────────────────────────────────────────────────────────
//!   domain_size              (u64 LE)              |     0  |     8
//!   num_inputs               (u64 LE)              |     8  |     8
//!   sigma_comms.len() = 5    (u64 LE)              |    16  |     8
//!   sigma_comms[0..5]        (G1Affine ×5)         |    24  |   480
//!   selector_comms.len() = 13 (u64 LE)             |   504  |     8
//!   selector_comms[0..13]    (G1Affine ×13)        |   512  |  1248
//!   k.len() = 5              (u64 LE)              |  1760  |     8
//!   k[0..5]                  (Fr ×5)               |  1768  |   160
//!   open_key.g               (G1Affine)            |  1928  |    96
//!   open_key.h               (G2Affine)            |  2024  |   192
//!   open_key.beta_h          (G2Affine)            |  2216  |   192
//!   open_key.powers_of_h.len() = 2 (u64 LE)        |  2408  |     8
//!   open_key.powers_of_h[0..2] (G2Affine ×2)       |  2416  |   384
//!   open_key.powers_of_g.len() = 2 (u64 LE)        |  2800  |     8
//!   open_key.powers_of_g[0..2] (G1Affine ×2)       |  2808  |   192
//!   is_merged                (bool, expect 0x00)   |  3000  |     1
//!   plookup_vk: Option<…>    (None, expect 0x00)   |  3001  |     1
//!
//!   Total:                                                    3002

/// Total byte length of an uncompressed canonical-shape VK.
pub const VK_LEN: usize = 3002;

/// Number of permutation-polynomial commitments.
pub const NUM_SIGMA_COMMS: usize = 5;

/// Number of selector-polynomial commitments (`N_TURBO_PLONK_SELECTORS`).
pub const NUM_SELECTOR_COMMS: usize = 13;

/// Number of `k` constants — same as `NUM_SIGMA_COMMS`.
pub const NUM_K_CONSTANTS: usize = NUM_SIGMA_COMMS;

/// `open_key.powers_of_h.len()` — fixed by the embedded EF KZG SRS.
pub const NUM_POWERS_OF_H: usize = 2;

/// `open_key.powers_of_g.len()` — fixed by the embedded EF KZG SRS.
pub const NUM_POWERS_OF_G: usize = 2;

/// Length of an arkworks-uncompressed BLS12-381 G1Affine point.
pub const G1_LEN: usize = 96;

/// Length of an arkworks-uncompressed BLS12-381 G2Affine point.
pub const G2_LEN: usize = 192;

/// Length of an arkworks-compressed BLS12-381 G2Affine point — used
/// by the transcript for the SRS τ in G2.
pub const G2_COMPRESSED_LEN: usize = G2_LEN / 2;

/// Length of an arkworks-canonical-serialised Fr element.
pub const FR_LEN: usize = 32;

// Pre-computed offsets, derived from the layout table above.
const OFF_DOMAIN_SIZE: usize = 0;
const OFF_NUM_INPUTS: usize = OFF_DOMAIN_SIZE + 8; // 8
const OFF_SIGMA_LEN: usize = OFF_NUM_INPUTS + 8; // 16
const OFF_SIGMA_FIRST: usize = OFF_SIGMA_LEN + 8; // 24
const OFF_SELECTOR_LEN: usize = OFF_SIGMA_FIRST + NUM_SIGMA_COMMS * G1_LEN; // 504
const OFF_SELECTOR_FIRST: usize = OFF_SELECTOR_LEN + 8; // 512
const OFF_K_LEN: usize = OFF_SELECTOR_FIRST + NUM_SELECTOR_COMMS * G1_LEN; // 1760
const OFF_K_FIRST: usize = OFF_K_LEN + 8; // 1768
const OFF_OPEN_G: usize = OFF_K_FIRST + NUM_K_CONSTANTS * FR_LEN; // 1928
const OFF_OPEN_H: usize = OFF_OPEN_G + G1_LEN; // 2024
const OFF_OPEN_BETA_H: usize = OFF_OPEN_H + G2_LEN; // 2216
const OFF_POWERS_OF_H_LEN: usize = OFF_OPEN_BETA_H + G2_LEN; // 2408
const OFF_POWERS_OF_H_FIRST: usize = OFF_POWERS_OF_H_LEN + 8; // 2416
const OFF_POWERS_OF_G_LEN: usize = OFF_POWERS_OF_H_FIRST + NUM_POWERS_OF_H * G2_LEN; // 2800
const OFF_POWERS_OF_G_FIRST: usize = OFF_POWERS_OF_G_LEN + 8; // 2808
const OFF_IS_MERGED: usize = OFF_POWERS_OF_G_FIRST + NUM_POWERS_OF_G * G1_LEN; // 3000
const OFF_PLOOKUP_OPT: usize = OFF_IS_MERGED + 1; // 3001

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parsed VK in byte form, suitable for direct consumption by the
/// Soroban verifier via `env.crypto().bls12_381().*`.
///
/// Structural parsing only — see `parse_vk_bytes` for the validation
/// contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedVerifyingKey {
    pub domain_size: u64,
    pub num_inputs: u64,
    pub sigma_commitments: [[u8; G1_LEN]; NUM_SIGMA_COMMS],
    pub selector_commitments: [[u8; G1_LEN]; NUM_SELECTOR_COMMS],
    pub k_constants: [[u8; FR_LEN]; NUM_K_CONSTANTS],
    pub open_key_g: [u8; G1_LEN],
    pub open_key_h: [u8; G2_LEN],
    pub open_key_beta_h: [u8; G2_LEN],
    pub open_key_powers_of_h: [[u8; G2_LEN]; NUM_POWERS_OF_H],
    pub open_key_powers_of_g: [[u8; G1_LEN]; NUM_POWERS_OF_G],
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    BadLength { expected: usize, actual: usize },
    BadSigmaLenPrefix(u64),
    BadSelectorLenPrefix(u64),
    BadKLenPrefix(u64),
    BadPowersOfHLenPrefix(u64),
    BadPowersOfGLenPrefix(u64),
    UnexpectedMergedKey,
    UnexpectedPlookupVk,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadLength { expected, actual } => {
                write!(f, "expected {expected} bytes, got {actual}")
            }
            Self::BadSigmaLenPrefix(n) => write!(
                f,
                "sigma_comms.len() prefix = {n}, expected {NUM_SIGMA_COMMS}"
            ),
            Self::BadSelectorLenPrefix(n) => write!(
                f,
                "selector_comms.len() prefix = {n}, expected {NUM_SELECTOR_COMMS}"
            ),
            Self::BadKLenPrefix(n) => {
                write!(f, "k.len() prefix = {n}, expected {NUM_K_CONSTANTS}")
            }
            Self::BadPowersOfHLenPrefix(n) => write!(
                f,
                "open_key.powers_of_h.len() prefix = {n}, expected {NUM_POWERS_OF_H}"
            ),
            Self::BadPowersOfGLenPrefix(n) => write!(
                f,
                "open_key.powers_of_g.len() prefix = {n}, expected {NUM_POWERS_OF_G}"
            ),
            Self::UnexpectedMergedKey => write!(
                f,
                "is_merged byte = 0x01, expected 0x00 (TurboPlonk merged keys not supported)"
            ),
            Self::UnexpectedPlookupVk => write!(
                f,
                "plookup_vk option byte = 0x01 (Some), expected 0x00 (None)"
            ),
        }
    }
}

/// Parse a 3002-byte VK stream. No allocations; copies into fixed-
/// size arrays.
///
/// **Structural parsing only.** Validates the byte layout (total
/// length, length prefixes, merged-key absence, plookup absence) but
/// does **not** check that G1/G2 points lie on the curve / in the
/// correct subgroup, and does not check that Fr constants are
/// canonical (`< r`). Curve / subgroup validation happens at the
/// host-fn boundary in the Soroban verifier.
pub fn parse_vk_bytes(bytes: &[u8]) -> Result<ParsedVerifyingKey, ParseError> {
    if bytes.len() != VK_LEN {
        return Err(ParseError::BadLength {
            expected: VK_LEN,
            actual: bytes.len(),
        });
    }

    let domain_size = read_u64_le(bytes, OFF_DOMAIN_SIZE);
    let num_inputs = read_u64_le(bytes, OFF_NUM_INPUTS);

    let sigma_len = read_u64_le(bytes, OFF_SIGMA_LEN);
    if sigma_len != NUM_SIGMA_COMMS as u64 {
        return Err(ParseError::BadSigmaLenPrefix(sigma_len));
    }
    let mut sigma_commitments = [[0u8; G1_LEN]; NUM_SIGMA_COMMS];
    for i in 0..NUM_SIGMA_COMMS {
        let off = OFF_SIGMA_FIRST + i * G1_LEN;
        sigma_commitments[i].copy_from_slice(&bytes[off..off + G1_LEN]);
    }

    let selector_len = read_u64_le(bytes, OFF_SELECTOR_LEN);
    if selector_len != NUM_SELECTOR_COMMS as u64 {
        return Err(ParseError::BadSelectorLenPrefix(selector_len));
    }
    let mut selector_commitments = [[0u8; G1_LEN]; NUM_SELECTOR_COMMS];
    for i in 0..NUM_SELECTOR_COMMS {
        let off = OFF_SELECTOR_FIRST + i * G1_LEN;
        selector_commitments[i].copy_from_slice(&bytes[off..off + G1_LEN]);
    }

    let k_len = read_u64_le(bytes, OFF_K_LEN);
    if k_len != NUM_K_CONSTANTS as u64 {
        return Err(ParseError::BadKLenPrefix(k_len));
    }
    let mut k_constants = [[0u8; FR_LEN]; NUM_K_CONSTANTS];
    for i in 0..NUM_K_CONSTANTS {
        let off = OFF_K_FIRST + i * FR_LEN;
        k_constants[i].copy_from_slice(&bytes[off..off + FR_LEN]);
    }

    let mut open_key_g = [0u8; G1_LEN];
    open_key_g.copy_from_slice(&bytes[OFF_OPEN_G..OFF_OPEN_G + G1_LEN]);
    let mut open_key_h = [0u8; G2_LEN];
    open_key_h.copy_from_slice(&bytes[OFF_OPEN_H..OFF_OPEN_H + G2_LEN]);
    let mut open_key_beta_h = [0u8; G2_LEN];
    open_key_beta_h.copy_from_slice(&bytes[OFF_OPEN_BETA_H..OFF_OPEN_BETA_H + G2_LEN]);

    let powers_of_h_len = read_u64_le(bytes, OFF_POWERS_OF_H_LEN);
    if powers_of_h_len != NUM_POWERS_OF_H as u64 {
        return Err(ParseError::BadPowersOfHLenPrefix(powers_of_h_len));
    }
    let mut open_key_powers_of_h = [[0u8; G2_LEN]; NUM_POWERS_OF_H];
    for i in 0..NUM_POWERS_OF_H {
        let off = OFF_POWERS_OF_H_FIRST + i * G2_LEN;
        open_key_powers_of_h[i].copy_from_slice(&bytes[off..off + G2_LEN]);
    }

    let powers_of_g_len = read_u64_le(bytes, OFF_POWERS_OF_G_LEN);
    if powers_of_g_len != NUM_POWERS_OF_G as u64 {
        return Err(ParseError::BadPowersOfGLenPrefix(powers_of_g_len));
    }
    let mut open_key_powers_of_g = [[0u8; G1_LEN]; NUM_POWERS_OF_G];
    for i in 0..NUM_POWERS_OF_G {
        let off = OFF_POWERS_OF_G_FIRST + i * G1_LEN;
        open_key_powers_of_g[i].copy_from_slice(&bytes[off..off + G1_LEN]);
    }

    if bytes[OFF_IS_MERGED] != 0x00 {
        return Err(ParseError::UnexpectedMergedKey);
    }
    if bytes[OFF_PLOOKUP_OPT] != 0x00 {
        return Err(ParseError::UnexpectedPlookupVk);
    }

    Ok(ParsedVerifyingKey {
        domain_size,
        num_inputs,
        sigma_commitments,
        selector_commitments,
        k_constants,
        open_key_g,
        open_key_h,
        open_key_beta_h,
        open_key_powers_of_h,
        open_key_powers_of_g,
    })
}

#[inline]
fn read_u64_le(bytes: &[u8], offset: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{build_synthetic_vk_bytes, fill_u64_le};

    #[test]
    fn pinned_offsets_match_layout_table() {
        assert_eq!(OFF_NUM_INPUTS, 8);
        assert_eq!(OFF_SIGMA_LEN, 16);
        assert_eq!(OFF_SIGMA_FIRST, 24);
        assert_eq!(OFF_SELECTOR_LEN, 504);
        assert_eq!(OFF_SELECTOR_FIRST, 512);
        assert_eq!(OFF_K_LEN, 1760);
        assert_eq!(OFF_K_FIRST, 1768);
        assert_eq!(OFF_OPEN_G, 1928);
        assert_eq!(OFF_OPEN_H, 2024);
        assert_eq!(OFF_OPEN_BETA_H, 2216);
        assert_eq!(OFF_POWERS_OF_H_LEN, 2408);
        assert_eq!(OFF_POWERS_OF_H_FIRST, 2416);
        assert_eq!(OFF_POWERS_OF_G_LEN, 2800);
        assert_eq!(OFF_POWERS_OF_G_FIRST, 2808);
        assert_eq!(OFF_IS_MERGED, 3000);
        assert_eq!(OFF_PLOOKUP_OPT, 3001);
        assert_eq!(VK_LEN, 3002);
    }

    /// A synthetic 3002-byte stream with valid length prefixes parses
    /// and produces a `ParsedVerifyingKey` whose fields point at the
    /// right offsets.
    #[test]
    fn parses_synthetic_canonical_stream() {
        let bytes = build_synthetic_vk_bytes(8192, 2);
        let parsed = parse_vk_bytes(&bytes).expect("synthetic stream parses");

        assert_eq!(parsed.domain_size, 8192);
        assert_eq!(parsed.num_inputs, 2);

        for i in 0..NUM_SIGMA_COMMS {
            assert_eq!(parsed.sigma_commitments[i][0], 0x10 + i as u8);
        }
        for i in 0..NUM_SELECTOR_COMMS {
            assert_eq!(parsed.selector_commitments[i][0], 0x20 + i as u8);
        }
        for i in 0..NUM_K_CONSTANTS {
            assert_eq!(parsed.k_constants[i][0], 0x30 + i as u8);
        }
        assert_eq!(parsed.open_key_g[0], 0x40);
        assert_eq!(parsed.open_key_h[0], 0x41);
        assert_eq!(parsed.open_key_beta_h[0], 0x42);
        for i in 0..NUM_POWERS_OF_H {
            assert_eq!(parsed.open_key_powers_of_h[i][0], 0x50 + i as u8);
        }
        for i in 0..NUM_POWERS_OF_G {
            assert_eq!(parsed.open_key_powers_of_g[i][0], 0x60 + i as u8);
        }
    }

    #[test]
    fn parse_rejects_wrong_total_length() {
        let bytes = [0u8; 100];
        match parse_vk_bytes(&bytes) {
            Err(ParseError::BadLength { expected: VK_LEN, actual: 100 }) => {}
            other => panic!("expected BadLength, got {other:?}"),
        }
    }

    /// Every reachable structural reject path is exercised.
    #[test]
    fn parse_rejects_bad_structural_fields() {
        let canonical = build_synthetic_vk_bytes(8192, 2);

        let cases: &[(usize, fn(&mut [u8], usize), fn(&ParseError) -> bool, &'static str)] = &[
            (
                OFF_SIGMA_LEN,
                |b, off| fill_u64_le(b, off, 6),
                |e| matches!(e, ParseError::BadSigmaLenPrefix(6)),
                "BadSigmaLenPrefix",
            ),
            (
                OFF_SELECTOR_LEN,
                |b, off| fill_u64_le(b, off, 14),
                |e| matches!(e, ParseError::BadSelectorLenPrefix(14)),
                "BadSelectorLenPrefix",
            ),
            (
                OFF_K_LEN,
                |b, off| fill_u64_le(b, off, 6),
                |e| matches!(e, ParseError::BadKLenPrefix(6)),
                "BadKLenPrefix",
            ),
            (
                OFF_POWERS_OF_H_LEN,
                |b, off| fill_u64_le(b, off, 3),
                |e| matches!(e, ParseError::BadPowersOfHLenPrefix(3)),
                "BadPowersOfHLenPrefix",
            ),
            (
                OFF_POWERS_OF_G_LEN,
                |b, off| fill_u64_le(b, off, 3),
                |e| matches!(e, ParseError::BadPowersOfGLenPrefix(3)),
                "BadPowersOfGLenPrefix",
            ),
            (
                OFF_IS_MERGED,
                |b, off| b[off] = 0x01,
                |e| matches!(e, ParseError::UnexpectedMergedKey),
                "UnexpectedMergedKey",
            ),
            (
                OFF_PLOOKUP_OPT,
                |b, off| b[off] = 0x01,
                |e| matches!(e, ParseError::UnexpectedPlookupVk),
                "UnexpectedPlookupVk",
            ),
        ];

        for &(offset, ref mutator, ref matcher, name) in cases {
            let mut bytes = canonical.clone();
            mutator(&mut bytes, offset);
            match parse_vk_bytes(&bytes) {
                Err(ref e) if matcher(e) => {}
                other => panic!("at offset={offset}, expected {name}, got {other:?}"),
            }
        }
    }
}
