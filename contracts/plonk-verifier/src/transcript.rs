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

    /// G1 conversion: a known generator-shaped input round-trips. The
    /// BLS12-381 G1 generator x is
    /// `0x17F1D3A7…BB`; arkworks writes BE so the first 48 bytes of
    /// the uncompressed buffer are exactly this BE form.
    #[test]
    fn g1_le_to_be_strips_only_flag_bits() {
        // Synthetic non-infinity point: x_be high byte = 0x17 (no flags),
        // y_be high byte = 0x08 (no flags).
        let mut bytes = [0u8; G1_LEN];
        bytes[0] = 0x17; // x[0]
        bytes[1] = 0xF1;
        bytes[G1_HALF] = 0x08; // y[0]
        bytes[G1_HALF + 1] = 0xB3;
        let (x, y) = arkworks_g1_uncompressed_to_be_xy(&bytes);
        assert_eq!(x[0], 0x17);
        assert_eq!(x[1], 0xF1);
        assert_eq!(y[0], 0x08);
        assert_eq!(y[1], 0xB3);
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
}
