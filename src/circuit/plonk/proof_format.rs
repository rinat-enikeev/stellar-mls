//! Byte-level parser for `jf_plonk::Proof<Bls12_381>` — Soroban-portable.
//!
//! The Soroban verifier (Phase C.2) cannot import jf-plonk; it has to
//! parse the proof byte stream directly via the host's
//! `bls12_381::g1_*` primitives. This module is the prover-side
//! reference for that parser:
//!
//! - **No jf-plonk types** in the `parse_proof_bytes` body — only
//!   slices and fixed-size byte arrays.
//! - **Round-trip-validated** against `jf_plonk::Proof::deserialize_
//!   uncompressed` in the test below: every offset and field-by-field
//!   mapping is asserted to agree byte-for-byte.
//!
//! The Soroban port (Phase C.2) replaces the `[u8; 96]` G1 outputs
//! with `BytesN<96>` and feeds them straight into
//! `env.crypto().bls12_381().g1_*`. Field-element evaluations stay
//! `[u8; 32]` for `Fr::from_le_bytes_mod_order` on the contract side.

#![cfg(feature = "plonk")]

// ---------------------------------------------------------------------------
// Wire-format constants. The total proof length and all per-field offsets
// are pinned here; if they drift, the round-trip test below fails.
//
//   Total uncompressed proof length: 1601 bytes
//
//   Field                                      | offset | length
//   ──────────────────────────────────────────────────────────────
//   wires_poly_comms.len() = 5  (u64 LE)       |     0  |     8
//   wires_poly_comms[0..5]      (G1Affine)     |     8  |   480 (5×96)
//   prod_perm_poly_comm         (G1Affine)     |   488  |    96
//   split_quot_poly_comms.len() = 5  (u64 LE)  |   584  |     8
//   split_quot_poly_comms[0..5] (G1Affine)     |   592  |   480 (5×96)
//   opening_proof               (G1Affine)     |  1072  |    96
//   shifted_opening_proof       (G1Affine)     |  1168  |    96
//   wires_evals.len() = 5       (u64 LE)       |  1264  |     8
//   wires_evals[0..5]           (Fr)           |  1272  |   160 (5×32)
//   wire_sigma_evals.len() = 4  (u64 LE)       |  1432  |     8
//   wire_sigma_evals[0..4]      (Fr)           |  1440  |   128 (4×32)
//   perm_next_eval              (Fr)           |  1568  |    32
//   plookup_proof: Option<…>    (None = 0x00)  |  1600  |     1
//
//   Total:                                                  1601
// ---------------------------------------------------------------------------

/// Total byte length of the uncompressed proof stream. Matches
/// `test_vectors::PROOF_UNCOMPRESSED_LEN`.
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

// Pre-computed offsets, derived from the layout table above.
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

/// Parsed proof in byte form, suitable for direct consumption by a
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
/// **Structural parsing only.** This function validates the proof's
/// byte layout (total length, length prefixes, plookup-absence), but
/// it does **not** check that the G1 points lie on the curve or in
/// the correct subgroup, and it does not check that the Fr evaluations
/// are canonical (`< r`). Trusting `ParsedProof` fields is therefore
/// only safe after a downstream validating step. In the Soroban
/// verifier (Phase C.2) that step is the host primitive
/// `env.crypto().bls12_381().g1_*`, which rejects off-curve points.
/// Callers in any other context must add an equivalent check.
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

    // We do not currently support Plookup proofs; reject any byte stream that
    // claims to carry one. Phase B circuits all set `plookup_proof = None`.
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
    use ark_bls12_381_v05::{Bls12_381, Fr};
    use ark_ff_v05::PrimeField;
    use ark_serialize_v05::{CanonicalDeserialize, CanonicalSerialize};
    use jf_plonk::proof_system::structs::Proof;
    use jf_relation::{Circuit, PlonkCircuit};
    use rand_chacha::rand_core::SeedableRng;

    use crate::circuit::plonk::membership::{synthesize_membership, MembershipWitness};
    use crate::circuit::plonk::poseidon::{poseidon_hash_one_v05, poseidon_hash_two_v05};
    use crate::prover::plonk;

    /// Generate a deterministic canonical proof for testing.
    fn canonical_proof_bytes(depth: usize) -> Vec<u8> {
        // Mirrors test_vectors::build_canonical_witness with the same seed.
        let secret_keys: Vec<Fr> = (1u64..=8).map(Fr::from).collect();
        let prover_index = 3usize;
        let epoch: u64 = 1234;
        let salt: [u8; 32] = [0xEE; 32];

        let leaves: Vec<Fr> = secret_keys.iter().map(poseidon_hash_one_v05).collect();
        let num_leaves = 1usize << depth;
        let mut nodes = vec![Fr::from(0u64); 2 * num_leaves];
        for (i, leaf) in leaves.iter().enumerate() {
            nodes[num_leaves + i] = *leaf;
        }
        for i in (1..num_leaves).rev() {
            nodes[i] = poseidon_hash_two_v05(&nodes[2 * i], &nodes[2 * i + 1]);
        }
        let root = nodes[1];

        let mut path = Vec::with_capacity(depth);
        let mut cur = num_leaves + prover_index;
        for _ in 0..depth {
            let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
            path.push(nodes[sib]);
            cur /= 2;
        }
        let salt_fr = Fr::from_le_bytes_mod_order(&salt);
        let inner = poseidon_hash_two_v05(&root, &Fr::from(epoch));
        let commitment = poseidon_hash_two_v05(&inner, &salt_fr);

        let witness = MembershipWitness {
            commitment,
            epoch,
            secret_key: secret_keys[prover_index],
            poseidon_root: root,
            salt,
            merkle_path: path,
            leaf_index: prover_index,
            depth,
        };

        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_membership(&mut circuit, &witness).unwrap();
        circuit.finalize_for_arithmetization().unwrap();
        let keys = plonk::preprocess(&circuit).unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let proof = plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();
        let mut bytes = Vec::new();
        proof.serialize_uncompressed(&mut bytes).unwrap();
        bytes
    }

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

    /// Parse a real proof and confirm it round-trips byte-for-byte
    /// against `Proof::deserialize_uncompressed → serialize_uncompressed`.
    #[test]
    fn parse_round_trips_against_jf_plonk_oracle() {
        for &depth in &[5usize, 8, 11] {
            let bytes = canonical_proof_bytes(depth);
            assert_eq!(bytes.len(), PROOF_LEN, "depth={depth} length");

            let parsed = parse_proof_bytes(&bytes).unwrap_or_else(|e| {
                panic!("parse failed at depth={depth}: {e}")
            });

            // Cross-check field-by-field against jf-plonk's own deserialiser.
            let oracle = Proof::<Bls12_381>::deserialize_uncompressed(&bytes[..])
                .expect("oracle deserialise");

            // Wires
            assert_eq!(oracle.wires_poly_comms.len(), NUM_WIRE_TYPES);
            for (i, comm) in oracle.wires_poly_comms.iter().enumerate() {
                let mut expected = Vec::new();
                comm.0.serialize_uncompressed(&mut expected).unwrap();
                assert_eq!(parsed.wire_commitments[i].as_slice(), expected.as_slice(),
                    "depth={depth} wire_commitments[{i}]");
            }

            // Prod perm
            let mut expected = Vec::new();
            oracle.prod_perm_poly_comm.0.serialize_uncompressed(&mut expected).unwrap();
            assert_eq!(parsed.prod_perm_commitment.as_slice(), expected.as_slice(),
                "depth={depth} prod_perm_commitment");

            // Split quot
            for (i, comm) in oracle.split_quot_poly_comms.iter().enumerate() {
                let mut expected = Vec::new();
                comm.0.serialize_uncompressed(&mut expected).unwrap();
                assert_eq!(parsed.split_quot_commitments[i].as_slice(), expected.as_slice(),
                    "depth={depth} split_quot_commitments[{i}]");
            }

            // Opening proofs
            let mut expected = Vec::new();
            oracle.opening_proof.0.serialize_uncompressed(&mut expected).unwrap();
            assert_eq!(parsed.opening_proof.as_slice(), expected.as_slice(),
                "depth={depth} opening_proof");
            let mut expected = Vec::new();
            oracle.shifted_opening_proof.0.serialize_uncompressed(&mut expected).unwrap();
            assert_eq!(parsed.shifted_opening_proof.as_slice(), expected.as_slice(),
                "depth={depth} shifted_opening_proof");

            // Wires evals
            assert_eq!(oracle.poly_evals.wires_evals.len(), NUM_WIRE_TYPES);
            for (i, eval) in oracle.poly_evals.wires_evals.iter().enumerate() {
                let mut expected = Vec::new();
                eval.serialize_uncompressed(&mut expected).unwrap();
                assert_eq!(parsed.wires_evals[i].as_slice(), expected.as_slice(),
                    "depth={depth} wires_evals[{i}]");
            }

            // Sigma evals
            assert_eq!(oracle.poly_evals.wire_sigma_evals.len(), NUM_WIRE_SIGMA_EVALS);
            for (i, eval) in oracle.poly_evals.wire_sigma_evals.iter().enumerate() {
                let mut expected = Vec::new();
                eval.serialize_uncompressed(&mut expected).unwrap();
                assert_eq!(parsed.wire_sigma_evals[i].as_slice(), expected.as_slice(),
                    "depth={depth} wire_sigma_evals[{i}]");
            }

            // Perm-next eval
            let mut expected = Vec::new();
            oracle.poly_evals.perm_next_eval.serialize_uncompressed(&mut expected).unwrap();
            assert_eq!(parsed.perm_next_eval.as_slice(), expected.as_slice(),
                "depth={depth} perm_next_eval");

            // Plookup absent
            assert!(oracle.plookup_proof.is_none(),
                "depth={depth} oracle has Plookup proof — parser would reject");
        }
    }

    /// Every reject path on the parser's structural-checks side is
    /// exercised: each of the four length-prefix variants and the
    /// plookup-discriminant variant. Important because the parser is
    /// the security boundary between Soroban host bytes and the
    /// verifier — any reachable-but-untested error path is a hole
    /// the host can drive.
    #[test]
    fn parse_rejects_bad_length_prefix() {
        let canonical = canonical_proof_bytes(5);

        // The first byte of each length-prefix offset is overwritten
        // with the (invalid) value; arkworks' u64 LE means this maps
        // to the low byte. Plookup byte is overwritten to 0x01 (Some).
        let cases: &[(usize, u8, fn(&ParseError) -> bool, &'static str)] = &[
            (OFF_WIRE_LEN, 6, |e| matches!(e, ParseError::BadWireLenPrefix(6)), "BadWireLenPrefix"),
            (OFF_QUOT_LEN, 6, |e| matches!(e, ParseError::BadQuotLenPrefix(6)), "BadQuotLenPrefix"),
            (OFF_WIRES_EVAL_LEN, 6, |e| matches!(e, ParseError::BadWiresEvalLenPrefix(6)), "BadWiresEvalLenPrefix"),
            (OFF_SIGMA_EVAL_LEN, 5, |e| matches!(e, ParseError::BadSigmaEvalLenPrefix(5)), "BadSigmaEvalLenPrefix"),
            (OFF_PLOOKUP_OPT, 0x01, |e| matches!(e, ParseError::UnexpectedPlookupProof), "UnexpectedPlookupProof"),
        ];

        for &(offset, byte, ref matcher, name) in cases {
            let mut bytes = canonical.clone();
            bytes[offset] = byte;
            match parse_proof_bytes(&bytes) {
                Err(ref e) if matcher(e) => {}
                other => panic!("at offset={offset}, expected {name}, got {other:?}"),
            }
        }
    }

    /// Wrong proof length is rejected.
    #[test]
    fn parse_rejects_wrong_total_length() {
        let bytes = vec![0u8; 100];
        match parse_proof_bytes(&bytes) {
            Err(ParseError::BadLength { expected: 1601, actual: 100 }) => {}
            other => panic!("expected BadLength, got {other:?}"),
        }
    }

}
