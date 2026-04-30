//! Soroban-portable port of `sep-xxxx-circuits::circuit::plonk::proof_format`.
//!
//! Byte-for-byte identical layout and parser logic to the Rust reference.
//! The reference's load-bearing oracle test (round-trip against
//! `jf_plonk::Proof::deserialize_uncompressed`) transfers to this port
//! because the parsing logic is copied verbatim — both crates produce
//! the same `ParsedProof` from the same `&[u8]`.
//!
//! The reference module's docstring carries the canonical byte-layout
//! table; we keep it here too so a contract reader can audit the
//! offsets without leaving the verifier crate.
//!
//! ## Layout (1601 bytes uncompressed)
//!
//!   Field                                      | offset | length
//!   ──────────────────────────────────────────────────────────────
//!   wires_poly_comms.len() = 5  (u64 LE)       |     0  |     8
//!   wires_poly_comms[0..5]      (G1Affine)     |     8  |   480 (5×96)
//!   prod_perm_poly_comm         (G1Affine)     |   488  |    96
//!   split_quot_poly_comms.len() = 5  (u64 LE)  |   584  |     8
//!   split_quot_poly_comms[0..5] (G1Affine)     |   592  |   480 (5×96)
//!   opening_proof               (G1Affine)     |  1072  |    96
//!   shifted_opening_proof       (G1Affine)     |  1168  |    96
//!   wires_evals.len() = 5       (u64 LE)       |  1264  |     8
//!   wires_evals[0..5]           (Fr)           |  1272  |   160 (5×32)
//!   wire_sigma_evals.len() = 4  (u64 LE)       |  1432  |     8
//!   wire_sigma_evals[0..4]      (Fr)           |  1440  |   128 (4×32)
//!   perm_next_eval              (Fr)           |  1568  |    32
//!   plookup_proof: Option<…>    (None = 0x00)  |  1600  |     1
//!
//!   Total:                                                  1601

/// Total byte length of the uncompressed proof stream.
pub const PROOF_LEN: usize = 1601;

/// Number of TurboPlonk wire-polynomial commitments.
pub const NUM_WIRE_TYPES: usize = 5;

/// Number of `wire_sigma` evaluations (one fewer than wire types — the
/// last sigma polynomial's evaluation is omitted as a verifier
/// optimisation in jf-plonk).
pub const NUM_WIRE_SIGMA_EVALS: usize = NUM_WIRE_TYPES - 1; // 4

/// Length of an arkworks-uncompressed BLS12-381 G1Affine point.
pub const G1_LEN: usize = 96;

/// Length of an arkworks-canonical-serialised Fr element.
pub const FR_LEN: usize = 32;

// Pre-computed offsets — derived from the layout table above.
const OFF_WIRE_LEN: usize = 0;
const OFF_WIRE_FIRST: usize = 8;
const OFF_PROD_PERM: usize = OFF_WIRE_FIRST + NUM_WIRE_TYPES * G1_LEN; // 488
const OFF_QUOT_LEN: usize = OFF_PROD_PERM + G1_LEN; // 584
const OFF_QUOT_FIRST: usize = OFF_QUOT_LEN + 8; // 592
const OFF_OPENING: usize = OFF_QUOT_FIRST + NUM_WIRE_TYPES * G1_LEN; // 1072
const OFF_SHIFTED_OPENING: usize = OFF_OPENING + G1_LEN; // 1168
const OFF_WIRES_EVAL_LEN: usize = OFF_SHIFTED_OPENING + G1_LEN; // 1264
const OFF_WIRES_EVAL_FIRST: usize = OFF_WIRES_EVAL_LEN + 8; // 1272
const OFF_SIGMA_EVAL_LEN: usize = OFF_WIRES_EVAL_FIRST + NUM_WIRE_TYPES * FR_LEN; // 1432
const OFF_SIGMA_EVAL_FIRST: usize = OFF_SIGMA_EVAL_LEN + 8; // 1440
const OFF_PERM_NEXT_EVAL: usize = OFF_SIGMA_EVAL_FIRST + NUM_WIRE_SIGMA_EVALS * FR_LEN; // 1568
const OFF_PLOOKUP_OPT: usize = OFF_PERM_NEXT_EVAL + FR_LEN; // 1600

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parsed proof in byte form, suitable for direct consumption by the
/// Soroban verifier via `env.crypto().bls12_381().*`.
///
/// All G1 elements are arkworks-uncompressed format (x_be || y_be,
/// each 48 B). Field-element evaluations are arkworks-canonical
/// little-endian Fr (32 B).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedProof {
    pub wire_commitments: [[u8; G1_LEN]; NUM_WIRE_TYPES],
    pub prod_perm_commitment: [u8; G1_LEN],
    pub split_quot_commitments: [[u8; G1_LEN]; NUM_WIRE_TYPES],
    pub opening_proof: [u8; G1_LEN],
    pub shifted_opening_proof: [u8; G1_LEN],
    pub wires_evals: [[u8; FR_LEN]; NUM_WIRE_TYPES],
    pub wire_sigma_evals: [[u8; FR_LEN]; NUM_WIRE_SIGMA_EVALS],
    pub perm_next_eval: [u8; FR_LEN],
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    BadLength { expected: usize, actual: usize },
    BadWireLenPrefix(u64),
    BadQuotLenPrefix(u64),
    BadWiresEvalLenPrefix(u64),
    BadSigmaEvalLenPrefix(u64),
    UnexpectedPlookupProof,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadLength { expected, actual } => {
                write!(f, "expected {expected} bytes, got {actual}")
            }
            Self::BadWireLenPrefix(n) => write!(f, "wires_poly_comms.len() prefix = {n}, expected 5"),
            Self::BadQuotLenPrefix(n) => write!(f, "split_quot_poly_comms.len() prefix = {n}, expected 5"),
            Self::BadWiresEvalLenPrefix(n) => write!(f, "wires_evals.len() prefix = {n}, expected 5"),
            Self::BadSigmaEvalLenPrefix(n) => write!(f, "wire_sigma_evals.len() prefix = {n}, expected 4"),
            Self::UnexpectedPlookupProof => write!(f, "plookup_proof option byte = 0x01 (Some), expected 0x00 (None)"),
        }
    }
}

/// Parse a 1601-byte proof stream. No allocations; copies into fixed-
/// size arrays.
///
/// **Structural parsing only.** Validates the proof's byte layout
/// (total length, length prefixes, plookup-absence), but does **not**
/// check that the G1 points lie on the curve / in the correct
/// subgroup, and does not check that the Fr evaluations are canonical
/// (`< r`). Trusting `ParsedProof` fields is therefore only safe
/// after a downstream validating step. The Soroban verifier reaches
/// curve validation by feeding G1 bytes into
/// `env.crypto().bls12_381().g1_*` host primitives, which reject
/// off-curve points.
pub fn parse_proof_bytes(bytes: &[u8]) -> Result<ParsedProof, ParseError> {
    if bytes.len() != PROOF_LEN {
        return Err(ParseError::BadLength {
            expected: PROOF_LEN,
            actual: bytes.len(),
        });
    }

    let wire_len = read_u64_le(bytes, OFF_WIRE_LEN);
    if wire_len != NUM_WIRE_TYPES as u64 {
        return Err(ParseError::BadWireLenPrefix(wire_len));
    }
    let mut wire_commitments = [[0u8; G1_LEN]; NUM_WIRE_TYPES];
    for i in 0..NUM_WIRE_TYPES {
        let off = OFF_WIRE_FIRST + i * G1_LEN;
        wire_commitments[i].copy_from_slice(&bytes[off..off + G1_LEN]);
    }

    let mut prod_perm_commitment = [0u8; G1_LEN];
    prod_perm_commitment.copy_from_slice(&bytes[OFF_PROD_PERM..OFF_PROD_PERM + G1_LEN]);

    let quot_len = read_u64_le(bytes, OFF_QUOT_LEN);
    if quot_len != NUM_WIRE_TYPES as u64 {
        return Err(ParseError::BadQuotLenPrefix(quot_len));
    }
    let mut split_quot_commitments = [[0u8; G1_LEN]; NUM_WIRE_TYPES];
    for i in 0..NUM_WIRE_TYPES {
        let off = OFF_QUOT_FIRST + i * G1_LEN;
        split_quot_commitments[i].copy_from_slice(&bytes[off..off + G1_LEN]);
    }

    let mut opening_proof = [0u8; G1_LEN];
    opening_proof.copy_from_slice(&bytes[OFF_OPENING..OFF_OPENING + G1_LEN]);
    let mut shifted_opening_proof = [0u8; G1_LEN];
    shifted_opening_proof
        .copy_from_slice(&bytes[OFF_SHIFTED_OPENING..OFF_SHIFTED_OPENING + G1_LEN]);

    let wires_eval_len = read_u64_le(bytes, OFF_WIRES_EVAL_LEN);
    if wires_eval_len != NUM_WIRE_TYPES as u64 {
        return Err(ParseError::BadWiresEvalLenPrefix(wires_eval_len));
    }
    let mut wires_evals = [[0u8; FR_LEN]; NUM_WIRE_TYPES];
    for i in 0..NUM_WIRE_TYPES {
        let off = OFF_WIRES_EVAL_FIRST + i * FR_LEN;
        wires_evals[i].copy_from_slice(&bytes[off..off + FR_LEN]);
    }

    let sigma_eval_len = read_u64_le(bytes, OFF_SIGMA_EVAL_LEN);
    if sigma_eval_len != NUM_WIRE_SIGMA_EVALS as u64 {
        return Err(ParseError::BadSigmaEvalLenPrefix(sigma_eval_len));
    }
    let mut wire_sigma_evals = [[0u8; FR_LEN]; NUM_WIRE_SIGMA_EVALS];
    for i in 0..NUM_WIRE_SIGMA_EVALS {
        let off = OFF_SIGMA_EVAL_FIRST + i * FR_LEN;
        wire_sigma_evals[i].copy_from_slice(&bytes[off..off + FR_LEN]);
    }

    let mut perm_next_eval = [0u8; FR_LEN];
    perm_next_eval.copy_from_slice(&bytes[OFF_PERM_NEXT_EVAL..OFF_PERM_NEXT_EVAL + FR_LEN]);

    if bytes[OFF_PLOOKUP_OPT] != 0x00 {
        return Err(ParseError::UnexpectedPlookupProof);
    }

    Ok(ParsedProof {
        wire_commitments,
        prod_perm_commitment,
        split_quot_commitments,
        opening_proof,
        shifted_opening_proof,
        wires_evals,
        wire_sigma_evals,
        perm_next_eval,
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
    use crate::test_fixtures::{build_synthetic_proof_bytes, fill_u64_le};

    /// Pinned offsets agree with the constants. Catches typos.
    #[test]
    fn pinned_offsets_match_layout_table() {
        assert_eq!(OFF_WIRE_FIRST, 8);
        assert_eq!(OFF_PROD_PERM, 488);
        assert_eq!(OFF_QUOT_LEN, 584);
        assert_eq!(OFF_QUOT_FIRST, 592);
        assert_eq!(OFF_OPENING, 1072);
        assert_eq!(OFF_SHIFTED_OPENING, 1168);
        assert_eq!(OFF_WIRES_EVAL_LEN, 1264);
        assert_eq!(OFF_WIRES_EVAL_FIRST, 1272);
        assert_eq!(OFF_SIGMA_EVAL_LEN, 1432);
        assert_eq!(OFF_SIGMA_EVAL_FIRST, 1440);
        assert_eq!(OFF_PERM_NEXT_EVAL, 1568);
        assert_eq!(OFF_PLOOKUP_OPT, 1600);
        assert_eq!(PROOF_LEN, 1601);
    }

    /// A synthetic 1601-byte stream with valid length prefixes parses
    /// and produces a `ParsedProof` whose fields point at the right
    /// offsets. The payload is a deterministic byte pattern so we can
    /// assert each split landed correctly.
    #[test]
    fn parses_synthetic_canonical_stream() {
        let bytes = build_synthetic_proof_bytes();
        let parsed = parse_proof_bytes(&bytes).expect("synthetic stream parses");

        // wires: each commitment starts with its byte index — we
        // wrote `wire_i_byte_0 = 0x10 + i` at the start of each
        // commitment in the synthetic helper.
        for i in 0..NUM_WIRE_TYPES {
            assert_eq!(parsed.wire_commitments[i][0], 0x10 + i as u8);
        }

        // prod_perm
        assert_eq!(parsed.prod_perm_commitment[0], 0x20);

        // split_quot
        for i in 0..NUM_WIRE_TYPES {
            assert_eq!(parsed.split_quot_commitments[i][0], 0x30 + i as u8);
        }

        // opening proofs
        assert_eq!(parsed.opening_proof[0], 0x40);
        assert_eq!(parsed.shifted_opening_proof[0], 0x41);

        // wires_evals
        for i in 0..NUM_WIRE_TYPES {
            assert_eq!(parsed.wires_evals[i][0], 0x50 + i as u8);
        }

        // wire_sigma_evals
        for i in 0..NUM_WIRE_SIGMA_EVALS {
            assert_eq!(parsed.wire_sigma_evals[i][0], 0x60 + i as u8);
        }

        // perm_next_eval
        assert_eq!(parsed.perm_next_eval[0], 0x70);
    }

    /// Wrong total length is rejected.
    #[test]
    fn parse_rejects_wrong_total_length() {
        let bytes = [0u8; 100];
        match parse_proof_bytes(&bytes) {
            Err(ParseError::BadLength { expected: 1601, actual: 100 }) => {}
            other => panic!("expected BadLength, got {other:?}"),
        }
    }

    /// Every reachable structural reject path is exercised. Critical
    /// because the parser is the security boundary between Soroban
    /// host bytes and the verifier — any reachable-but-untested error
    /// path is a hole the host can drive.
    #[test]
    fn parse_rejects_bad_structural_fields() {
        let canonical = build_synthetic_proof_bytes();

        // (offset, byte-write-fn, predicate, name)
        let cases: &[(usize, fn(&mut [u8], usize), fn(&ParseError) -> bool, &'static str)] = &[
            (
                OFF_WIRE_LEN,
                |b, off| fill_u64_le(b, off, 6),
                |e| matches!(e, ParseError::BadWireLenPrefix(6)),
                "BadWireLenPrefix",
            ),
            (
                OFF_QUOT_LEN,
                |b, off| fill_u64_le(b, off, 6),
                |e| matches!(e, ParseError::BadQuotLenPrefix(6)),
                "BadQuotLenPrefix",
            ),
            (
                OFF_WIRES_EVAL_LEN,
                |b, off| fill_u64_le(b, off, 6),
                |e| matches!(e, ParseError::BadWiresEvalLenPrefix(6)),
                "BadWiresEvalLenPrefix",
            ),
            (
                OFF_SIGMA_EVAL_LEN,
                |b, off| fill_u64_le(b, off, 5),
                |e| matches!(e, ParseError::BadSigmaEvalLenPrefix(5)),
                "BadSigmaEvalLenPrefix",
            ),
            (
                OFF_PLOOKUP_OPT,
                |b, off| b[off] = 0x01,
                |e| matches!(e, ParseError::UnexpectedPlookupProof),
                "UnexpectedPlookupProof",
            ),
        ];

        for &(offset, ref mutator, ref matcher, name) in cases {
            let mut bytes = canonical.clone();
            mutator(&mut bytes, offset);
            match parse_proof_bytes(&bytes) {
                Err(ref e) if matcher(e) => {}
                other => panic!("at offset={offset}, expected {name}, got {other:?}"),
            }
        }
    }
}
