//! Soroban-side port of the Fiat-Shamir transcript from
//! `sep-xxxx-circuits::circuit::plonk::transcript` (PR #178).
//!
//! State machine identical to the prover-side reference; only the
//! Keccak-256 backend differs:
//!
//! - **Reference (off-chain):** `sha3::Keccak256` over a `Vec<u8>`.
//! - **This crate (on-chain):** `env.crypto().keccak256(&Bytes)` over
//!   a Soroban `Bytes` accumulator.
//!
//! The state-update equation is unchanged:
//!
//! ```text
//!   state ← Keccak256(state || transcript)
//!   transcript ← empty
//! ```
//!
//! Field-element and G1-commitment appends use **big-endian** form
//! (Solidity convention). The conversion helpers
//! [`arkworks_fr_le_to_be`] and [`arkworks_g1_uncompressed_to_be_xy`]
//! handle the two arkworks-bls12-381 quirks the prover-side ref
//! documents:
//!
//! 1. **Asymmetric endianness.** `Fr` (scalar field) is serialised LE
//!    by arkworks; `Fp` (base field, G1/G2 coords) is serialised BE
//!    (IETF / EIP-2537 canonical encoding via arkworks-bls12-381's
//!    custom serialiser).
//! 2. **Flag bits in `bytes[0]`.** arkworks-bls12-381 packs
//!    compression / infinity / sort flags in the top 3 bits of the
//!    very first buffer byte (the high byte of x in BE), not in y.
//!
//! See `circuit::plonk::transcript` doc comments for the full
//! provenance — those quirks were established there with arkworks
//! source citations and oracle-tested against jf-plonk's
//! `SolidityTranscript`. This crate is byte-equivalent by
//! construction.

use soroban_sdk::{Bytes, Env};

use crate::vk_format::{
    ParsedVerifyingKey, FR_LEN, G1_LEN, G2_COMPRESSED_LEN, NUM_K_CONSTANTS, NUM_SELECTOR_COMMS,
    NUM_SIGMA_COMMS,
};

/// BLS12-381 scalar-field modulus bit size (255 bits). Fed into the
/// transcript header during `append_vk_and_public_inputs`.
pub const FR_MODULUS_BITS: u32 = 255;

/// Half of the uncompressed G1 byte length — one field-element x or y.
pub const G1_HALF: usize = G1_LEN / 2; // 48

/// Soroban-portable Fiat-Shamir transcript.
///
/// Holds a 32-byte rolling state and a `Bytes` accumulator of
/// not-yet-squeezed input. Each `squeeze` call hashes
/// `state || transcript` via `env.crypto().keccak256`.
pub struct SolidityTranscript<'a> {
    env: &'a Env,
    state: [u8; 32],
    transcript: Bytes,
}

impl<'a> SolidityTranscript<'a> {
    /// Create a fresh transcript with zero state.
    pub fn new(env: &'a Env) -> Self {
        Self {
            env,
            state: [0u8; 32],
            transcript: Bytes::new(env),
        }
    }

    /// Append raw bytes to the unsqueezed buffer.
    pub fn append_message(&mut self, msg: &[u8]) {
        self.transcript.extend_from_slice(msg);
    }

    /// Append a G1 commitment in Solidity-BE form (`x_be(48) || y_be(48)`).
    pub fn append_g1_commitment_be(&mut self, x_be: &[u8; G1_HALF], y_be: &[u8; G1_HALF]) {
        self.transcript.extend_from_array(x_be);
        self.transcript.extend_from_array(y_be);
    }

    /// Append a 32-byte BE field element.
    pub fn append_field_elem_be(&mut self, fe_be: &[u8; FR_LEN]) {
        self.transcript.extend_from_array(fe_be);
    }

    /// Squeeze a 32-byte challenge: `state := keccak256(state || transcript);
    /// transcript.clear(); return state`.
    pub fn squeeze(&mut self) -> [u8; 32] {
        let mut buf = Bytes::from_array(self.env, &self.state);
        buf.append(&self.transcript);
        let hash = self.env.crypto().keccak256(&buf);
        // `Hash<32> -> [u8; 32]` via `Into<[u8; 32]>` blanket impl.
        let new_state: [u8; 32] = hash.into();
        self.state = new_state;
        self.transcript = Bytes::new(self.env);
        new_state
    }

    /// Test-only inspection of the unsqueezed buffer. Used by the
    /// (forthcoming) `append_vk_and_public_inputs_step_by_step`
    /// diagnostic test once we wire up byte-stream comparison against
    /// the Rust reference's output. Currently unused in this PR but
    /// kept so the test in the next PR doesn't need to re-add it.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn buffered_bytes(&self) -> Bytes {
        self.transcript.clone()
    }

    /// Mirror of jf-plonk's `append_vk_and_pub_input` for the
    /// `SolidityTranscript`. Drives the transcript's initial state
    /// before the verifier consumes the proof.
    ///
    /// `srs_g2_compressed` is `to_bytes!(&vk.open_key.powers_of_h[1])`
    /// (arkworks-compressed BE, 96 bytes with sign + infinity flags
    /// in `bytes[0]`). Public inputs must already be in BE form
    /// (`Fr::into_bigint().to_bytes_be()`).
    ///
    /// The 12-byte zero pad after the three header fields is the
    /// EVM-word-alignment quirk from `SolidityTranscript`: 4 (field
    /// modulus bits) + 8 (domain size) + 8 (input size) = 20 bytes;
    /// pad to the 32-byte EVM word boundary.
    pub fn append_vk_and_public_inputs(
        &mut self,
        vk: &ParsedVerifyingKey,
        srs_g2_compressed: &[u8; G2_COMPRESSED_LEN],
        public_inputs_be: &[[u8; FR_LEN]],
    ) {
        // Validate VK shape *before* writing anything, so a malformed
        // `ParsedVerifyingKey` fails fast rather than poisoning the
        // transcript buffer with partial state.
        assert_eq!(
            vk.k_constants.len(),
            NUM_K_CONSTANTS,
            "ParsedVerifyingKey::k_constants has wrong length"
        );
        assert_eq!(
            vk.selector_commitments.len(),
            NUM_SELECTOR_COMMS,
            "ParsedVerifyingKey::selector_commitments has wrong length"
        );
        assert_eq!(
            vk.sigma_commitments.len(),
            NUM_SIGMA_COMMS,
            "ParsedVerifyingKey::sigma_commitments has wrong length"
        );

        // 1. field size in bits — 4 bytes BE u32
        self.append_message(&FR_MODULUS_BITS.to_be_bytes());
        // 2. domain size — 8 bytes BE u64
        self.append_message(&vk.domain_size.to_be_bytes());
        // 3. input size — 8 bytes BE u64
        self.append_message(&vk.num_inputs.to_be_bytes());
        // 4. EVM-word-alignment pad
        self.append_message(&[0u8; 12]);
        // 5. SRS G2 element — 96 bytes compressed BE
        self.append_message(srs_g2_compressed);
        // 6. wire-subset separators (k constants) — 5 × Fr BE
        for k_le in &vk.k_constants {
            let k_be = arkworks_fr_le_to_be(k_le);
            self.append_field_elem_be(&k_be);
        }
        // 7. selector commitments — 13 × G1 BE
        for sel_le in &vk.selector_commitments {
            let (x_be, y_be) = arkworks_g1_uncompressed_to_be_xy(sel_le);
            self.append_g1_commitment_be(&x_be, &y_be);
        }
        // 8. sigma commitments — 5 × G1 BE
        for sig_le in &vk.sigma_commitments {
            let (x_be, y_be) = arkworks_g1_uncompressed_to_be_xy(sig_le);
            self.append_g1_commitment_be(&x_be, &y_be);
        }
        // 9. public inputs — N × Fr BE
        for pi in public_inputs_be {
            self.append_field_elem_be(pi);
        }
    }
}

// ---------------------------------------------------------------------------
// Byte-format conversions: arkworks-uncompressed-LE ↔ Solidity-BE.
//
// Pure byte ops — no host calls, no Env. Verbatim from the prover-side
// reference (`circuit::plonk::transcript`).
// ---------------------------------------------------------------------------

/// Reverse a 32-byte LE Fr representation into 32 BE bytes.
pub fn arkworks_fr_le_to_be(le: &[u8; FR_LEN]) -> [u8; FR_LEN] {
    let mut out = [0u8; FR_LEN];
    for (o, &b) in out.iter_mut().zip(le.iter().rev()) {
        *o = b;
    }
    out
}

/// Split a 96-byte arkworks-uncompressed G1 into `(x_be, y_be)`,
/// stripping arkworks-bls12-381 0.5's flag bits from the first byte.
///
/// **Not validation.** The mask is a defensive scrub of flag bits
/// (which the upstream parsers don't strip); on-curve / subgroup /
/// canonicity checks happen later when these bytes are fed to
/// `env.crypto().bls12_381().g1_*` host primitives.
pub fn arkworks_g1_uncompressed_to_be_xy(
    bytes: &[u8; G1_LEN],
) -> ([u8; G1_HALF], [u8; G1_HALF]) {
    let mut x_be = [0u8; G1_HALF];
    let mut y_be = [0u8; G1_HALF];
    x_be.copy_from_slice(&bytes[0..G1_HALF]);
    y_be.copy_from_slice(&bytes[G1_HALF..G1_LEN]);
    // y has no flag bits in this format; bits 5-7 of `y_be[0]` must
    // be zero for any valid y < p (since BLS12-381's `p` is 381 bits).
    debug_assert_eq!(
        y_be[0] & 0xE0,
        0,
        "y high byte has bits 5-7 set; upstream parser corrupted or fed non-canonical bytes"
    );
    x_be[0] &= 0x1F;
    y_be[0] &= 0x1F;
    (x_be, y_be)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha3::{Digest, Keccak256};
    use soroban_sdk::Env;

    /// G1 conversion on a non-infinity, no-flags input — both halves
    /// pass through unchanged. BLS12-381 G1 generator x starts with
    /// `0x17F1D3A7…`; arkworks writes BE so the first 48 bytes of the
    /// uncompressed buffer are exactly this BE form.
    #[test]
    fn g1_le_to_be_passes_through_no_flag_input() {
        let mut bytes = [0u8; G1_LEN];
        bytes[0] = 0x17; // x[0] — bits 5-7 already zero, no strip needed
        bytes[1] = 0xF1;
        bytes[G1_HALF] = 0x08; // y[0]
        bytes[G1_HALF + 1] = 0xB3;
        let (x, y) = arkworks_g1_uncompressed_to_be_xy(&bytes);
        assert_eq!(x[0], 0x17);
        assert_eq!(x[1], 0xF1);
        assert_eq!(y[0], 0x08);
        assert_eq!(y[1], 0xB3);
    }

    /// Actually exercise the `bytes[0] &= 0x1F` mask: feed inputs
    /// where bits 5-7 of `bytes[0]` are set (compression, infinity,
    /// sort flags, in arkworks-bls12-381 0.5's encoding) and assert
    /// the high bits are cleared while the low 5 bits + the rest of
    /// the field element survive untouched.
    ///
    /// The previous `g1_le_to_be_passes_through_no_flag_input` would
    /// have passed even if the mask were removed; this case fails
    /// without it.
    #[test]
    fn g1_le_to_be_strips_high_flag_bits_from_x() {
        // bytes[0] = 0xBA = 0b1011_1010 — top 3 bits (compression +
        // sort) set, plus a non-zero low-5-bit field-element value
        // (0b1_1010 = 0x1A) that must survive the strip.
        let mut bytes = [0u8; G1_LEN];
        bytes[0] = 0xBA;
        bytes[1] = 0xCD;
        bytes[G1_HALF] = 0x05; // y high byte: bit 7 unset, bits 0-4 = 0x05
        bytes[G1_HALF + 1] = 0xEF;
        let (x, y) = arkworks_g1_uncompressed_to_be_xy(&bytes);
        // x[0] should have bits 5-7 cleared, low 5 bits (0x1A) preserved.
        assert_eq!(x[0], 0x1A, "x[0] high bits not stripped: got 0x{:02x}", x[0]);
        // x[1] is untouched.
        assert_eq!(x[1], 0xCD);
        // y[0] unchanged (no high bits set).
        assert_eq!(y[0], 0x05);
        assert_eq!(y[1], 0xEF);
    }

    /// Edge case: a hypothetical "all-flags-set" `bytes[0] = 0xE0`
    /// (bits 5-7 all set, bits 0-4 zero) collapses to `x[0] = 0x00`.
    /// The point's body bytes (zero in this case) survive intact.
    #[test]
    fn g1_le_to_be_strips_all_high_flag_bits() {
        let mut bytes = [0u8; G1_LEN];
        bytes[0] = 0xE0; // 0b1110_0000 — top 3 flag bits set, low bits zero
        let (x, y) = arkworks_g1_uncompressed_to_be_xy(&bytes);
        assert_eq!(x[0], 0x00);
        assert_eq!(y[0], 0x00);
    }

    /// Infinity point: arkworks-bls12-381 sets bit 6 of `bytes[0]`
    /// (the infinity flag) on serialise, then the remaining bytes are
    /// zero. After our mask both halves come out as 48 zero bytes —
    /// matching jf-plonk's `(0, 0)` substitution for `comm.is_zero()`
    /// points.
    #[test]
    fn g1_le_to_be_returns_zero_for_infinity_point() {
        let mut bytes = [0u8; G1_LEN];
        bytes[0] = 0x40; // infinity flag
        let (x, y) = arkworks_g1_uncompressed_to_be_xy(&bytes);
        assert_eq!(x, [0u8; G1_HALF]);
        assert_eq!(y, [0u8; G1_HALF]);
    }

    /// Fr conversion is a simple byte reverse.
    #[test]
    fn fr_le_to_be_reverses() {
        let mut le = [0u8; FR_LEN];
        for (i, b) in le.iter_mut().enumerate() {
            *b = i as u8;
        }
        let be = arkworks_fr_le_to_be(&le);
        for i in 0..FR_LEN {
            assert_eq!(be[i], le[FR_LEN - 1 - i]);
        }
    }

    /// Squeezing a fresh transcript matches `Keccak256(0^32)`. This
    /// pins both the state-init logic and the keccak host primitive
    /// against the reference Keccak implementation.
    #[test]
    fn squeeze_empty_matches_keccak_of_zeros() {
        let env = Env::default();
        let mut t = SolidityTranscript::new(&env);
        let challenge = t.squeeze();

        let mut hasher = Keccak256::new();
        hasher.update([0u8; 32]);
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(challenge, expected);
    }

    /// State machine: `state ← keccak256(state || transcript)`. Append
    /// a known sequence of bytes, squeeze, and confirm the result
    /// matches `Keccak256(0^32 || appended_bytes)` computed by `sha3`.
    /// Mirrors the load-bearing oracle test in the reference module.
    #[test]
    fn squeeze_after_append_matches_manual_keccak() {
        let env = Env::default();
        let mut t = SolidityTranscript::new(&env);

        // First squeeze: append [1, 2, 3, ..., 64], squeeze, expect
        // keccak256(0^32 || bytes).
        let mut payload = [0u8; 64];
        for (i, b) in payload.iter_mut().enumerate() {
            *b = (i + 1) as u8;
        }
        t.append_message(&payload);
        let chal_1 = t.squeeze();

        let mut hasher = Keccak256::new();
        hasher.update([0u8; 32]);
        hasher.update(payload);
        let expected_state_1: [u8; 32] = hasher.finalize().into();
        assert_eq!(chal_1, expected_state_1);

        // Second squeeze: append [10; 16], squeeze, expect
        // keccak256(state_1 || [10; 16]) — proves state rolled.
        let payload2 = [10u8; 16];
        t.append_message(&payload2);
        let chal_2 = t.squeeze();

        let mut hasher = Keccak256::new();
        hasher.update(expected_state_1);
        hasher.update(payload2);
        let expected_state_2: [u8; 32] = hasher.finalize().into();
        assert_eq!(chal_2, expected_state_2);

        // Sanity: clearing transcript on squeeze means a third squeeze
        // with no appends should hash state_2 alone.
        let chal_3 = t.squeeze();
        let mut hasher = Keccak256::new();
        hasher.update(expected_state_2);
        let expected_state_3: [u8; 32] = hasher.finalize().into();
        assert_eq!(chal_3, expected_state_3);
    }

    /// `append_g1_commitment_be` and `append_field_elem_be` produce
    /// the same buffered bytes as `append_message` with a manually-
    /// concatenated payload. Cleanest way to confirm the typed appends
    /// don't lose / reorder bytes.
    #[test]
    fn typed_appends_match_raw_message_appends() {
        let env = Env::default();
        let mut t_typed = SolidityTranscript::new(&env);
        let mut t_raw = SolidityTranscript::new(&env);

        let x_be = [0xAA; G1_HALF];
        let y_be = [0xBB; G1_HALF];
        let fe_be = [0xCC; FR_LEN];

        t_typed.append_g1_commitment_be(&x_be, &y_be);
        t_typed.append_field_elem_be(&fe_be);

        // Manually concatenate.
        let mut combined = [0u8; G1_HALF * 2 + FR_LEN];
        combined[0..G1_HALF].copy_from_slice(&x_be);
        combined[G1_HALF..G1_HALF * 2].copy_from_slice(&y_be);
        combined[G1_HALF * 2..].copy_from_slice(&fe_be);
        t_raw.append_message(&combined);

        // Squeeze both and compare.
        let chal_typed = t_typed.squeeze();
        let chal_raw = t_raw.squeeze();
        assert_eq!(chal_typed, chal_raw);
    }

    /// Walk `append_vk_and_public_inputs` step by step and assert the
    /// buffered bytes match a manually-concatenated reference stream.
    /// Mirrors the off-chain reference's
    /// `append_vk_step_by_step_matches_manual_byte_stream` test
    /// (`src/circuit/plonk/transcript.rs`).
    ///
    /// This pins the most byte-sequence-sensitive function in the
    /// module: header layout, 12-byte EVM pad, k → BE conversion,
    /// selector / sigma ordering, infinity-substitution behavior.
    /// A regression in append ordering surfaces here, not at the
    /// next phase.
    ///
    /// The synthetic VK from `test_fixtures::build_synthetic_vk_bytes`
    /// has commitment slot first-bytes in the low 5 bits (0x10–0x60
    /// ranges), so `arkworks_g1_uncompressed_to_be_xy`'s `& 0x1F`
    /// mask is a no-op on this input — the manual reference stream
    /// can predict each commitment's byte exactly.
    #[test]
    fn append_vk_step_by_step_matches_manual_byte_stream() {
        use crate::test_fixtures::build_synthetic_vk_bytes;
        use crate::vk_format::{
            parse_vk_bytes, NUM_K_CONSTANTS, NUM_SELECTOR_COMMS, NUM_SIGMA_COMMS,
        };

        let domain_size = 8192u64;
        let num_inputs = 2u64;
        let vk_bytes = build_synthetic_vk_bytes(domain_size, num_inputs);
        let parsed = parse_vk_bytes(&vk_bytes).expect("parse vk");

        // Synthetic SRS G2 element + public inputs.
        let srs_g2_compressed: [u8; G2_COMPRESSED_LEN] = {
            let mut a = [0u8; G2_COMPRESSED_LEN];
            a[0] = 0xDE;
            a[1] = 0xAD;
            a
        };
        let public_inputs_be: [[u8; FR_LEN]; 2] = {
            let mut p = [[0u8; FR_LEN]; 2];
            p[0][0] = 0x70;
            p[1][0] = 0x71;
            p
        };

        // What our port buffers.
        let env = Env::default();
        let mut t = SolidityTranscript::new(&env);
        t.append_vk_and_public_inputs(&parsed, &srs_g2_compressed, &public_inputs_be);
        let actual = t.buffered_bytes();

        // What jf-plonk's logic would produce, recreated manually.
        let env2 = Env::default();
        let mut expected = Bytes::new(&env2);
        // 1. field size in bits (4 BE)
        expected.extend_from_array(&FR_MODULUS_BITS.to_be_bytes());
        // 2. domain size (8 BE u64)
        expected.extend_from_array(&domain_size.to_be_bytes());
        // 3. num_inputs (8 BE u64)
        expected.extend_from_array(&num_inputs.to_be_bytes());
        // 4. 12-byte EVM word-alignment pad
        expected.extend_from_array(&[0u8; 12]);
        // 5. SRS G2 element (96 compressed bytes)
        expected.extend_from_array(&srs_g2_compressed);
        // 6. k constants — 5 × Fr BE.
        // The synthetic VK's k_constants[i][0] = 0x30 + i (LE), all other bytes 0.
        // Reverse → BE high byte = 0, …, BE last byte = 0x30 + i.
        for i in 0..NUM_K_CONSTANTS {
            let mut k_be = [0u8; FR_LEN];
            k_be[FR_LEN - 1] = 0x30 + i as u8; // last BE byte = first LE byte
            expected.extend_from_array(&k_be);
        }
        // 7. selector commitments — 13 × G1 BE (x||y, 96 B each).
        // Synthetic selector_commitments[i][0] = 0x20 + i (LE high byte
        // of x, in BE form bytes[0]). Mask `& 0x1F` is a no-op on
        // 0x20..0x2C since bits 5-7 are zero (0x20 = 0010_0000…, only
        // bit 5 set — wait, that DOES get cleared).
        //
        // Actually 0x20 = 0010_0000, bit 5 = 1. The mask `& 0x1F` =
        // `& 0001_1111` clears bits 5-7, so 0x20 → 0x00. Need to
        // reflect that in the manual reference stream.
        for i in 0..NUM_SELECTOR_COMMS {
            let raw_first = 0x20u8 + i as u8;
            let mut x_be = [0u8; G1_HALF];
            x_be[0] = raw_first & 0x1F;
            let y_be = [0u8; G1_HALF];
            expected.extend_from_array(&x_be);
            expected.extend_from_array(&y_be);
        }
        // 8. sigma commitments — 5 × G1 BE.
        // sigma_commitments[i][0] = 0x10 + i (LE), masked → 0x10 + i
        // since 0x10 < 0x20 (bits 5-7 all zero for 0x10..0x14).
        for i in 0..NUM_SIGMA_COMMS {
            let raw_first = 0x10u8 + i as u8;
            let mut x_be = [0u8; G1_HALF];
            x_be[0] = raw_first & 0x1F;
            let y_be = [0u8; G1_HALF];
            expected.extend_from_array(&x_be);
            expected.extend_from_array(&y_be);
        }
        // 9. public inputs.
        for pi in &public_inputs_be {
            expected.extend_from_array(pi);
        }

        assert_eq!(actual.len(), expected.len(), "buffered length mismatch");
        // Bytes::PartialEq compares contents in the same Env.  Both
        // builders use independent envs so we copy actual → expected's
        // env via to_alloc_vec / iter to compare.  Use byte-by-byte
        // iteration since Bytes doesn't directly support cross-env
        // equality.
        let actual_bytes: alloc::vec::Vec<u8> =
            (0..actual.len()).map(|i| actual.get_unchecked(i)).collect();
        let expected_bytes: alloc::vec::Vec<u8> =
            (0..expected.len()).map(|i| expected.get_unchecked(i)).collect();
        if actual_bytes != expected_bytes {
            // Find first divergence offset for an actionable error.
            let off = actual_bytes
                .iter()
                .zip(expected_bytes.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(actual_bytes.len().min(expected_bytes.len()));
            let end = (off + 8).min(actual_bytes.len()).min(expected_bytes.len());
            panic!(
                "buffered bytes diverge at offset {off}: \
                 actual[{off}..{end}]={:02x?} expected[{off}..{end}]={:02x?}",
                &actual_bytes[off..end],
                &expected_bytes[off..end],
            );
        }
    }

    // The `alloc::vec::Vec` use above requires the `alloc` import;
    // `soroban-sdk` enables `alloc` transitively for tests via its
    // testutils feature.
    extern crate alloc;
}
