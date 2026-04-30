//! Soroban-portable port of jf-plonk's `SolidityTranscript`.
//!
//! The transcript is the Fiat-Shamir state machine the verifier uses
//! to derive challenges (β, γ, α, ζ, v, u) from a deterministic
//! sequence of public-input + commitment + evaluation appends. For
//! the verifier to accept a proof, the prover and verifier must agree
//! on the transcript byte-for-byte.
//!
//! jf-plonk's `SolidityTranscript` (`plonk/src/transcript/solidity.rs`)
//! is the reference. It uses Keccak-256 with this state machine:
//!
//! ```text
//!   state = [0u8; 32]
//!   transcript = Vec::new()
//!
//!   append_message(msg):
//!     transcript.extend(msg)
//!
//!   squeeze() -> [u8; 32]:
//!     state = keccak256(state || transcript)
//!     transcript.clear()
//!     return state
//! ```
//!
//! Field elements and G1 commitments are appended in **big-endian**
//! form (Solidity's native order). For BLS12-381 specifically:
//!
//! - The base field Fp (G1/G2 coordinates) is **already serialised BE**
//!   by arkworks 0.5, matching the IETF / EIP-2537 canonical encoding.
//!   Splitting `serialize_uncompressed` bytes in half gives `(x_be, y_be)`
//!   directly — only flag bits need stripping.
//! - The scalar field Fr (k constants, public inputs, evaluations) is
//!   serialised **LE**. [`arkworks_fr_le_to_be`] reverses it.
//!
//! The conversion helpers handle both cases.
//!
//! The Soroban port (Phase C.2) replaces `sha3::Keccak256` with
//! `env.crypto().keccak256_simple(bytes)` and the `Vec<u8>`
//! accumulator with Soroban `Bytes`. Everything else carries over
//! unchanged. Byte-exactness with jf-plonk is verified by
//! [`tests::transcript_matches_jf_plonk_oracle`].
//!
//! # Public-input encoding
//!
//! Membership proofs have two public inputs `(commitment, epoch)`,
//! both `Fr` (32 BE bytes). The transcript appends them as a single
//! sequence of length-prefixed BE field elements, matching jf-plonk's
//! `append_field_elems`. Callers that hold their public inputs as
//! arkworks-LE bytes should use [`arkworks_fr_le_to_be`] before
//! feeding them to [`SolidityTranscript::append_vk_and_public_inputs`].

#![cfg(feature = "plonk")]

use sha3::{Digest, Keccak256};

use crate::circuit::plonk::vk_format::{
    ParsedVerifyingKey, FR_LEN, G1_LEN, G2_LEN, NUM_K_CONSTANTS, NUM_SELECTOR_COMMS,
    NUM_SIGMA_COMMS,
};

/// BLS12-381 scalar-field modulus bit size, used in
/// `append_vk_and_public_inputs` (matches jf-plonk's
/// `E::ScalarField::MODULUS_BIT_SIZE`).
pub const FR_MODULUS_BITS: u32 = 255;

/// Half of the uncompressed G1 byte length — one field-element x or y.
const G1_HALF: usize = G1_LEN / 2; // 48

/// Soroban-portable Fiat-Shamir transcript, byte-compatible with
/// jf-plonk's `SolidityTranscript`.
///
/// Holds a 32-byte rolling state and a buffered byte vector of
/// not-yet-squeezed input. Each `squeeze` call hashes
/// `state || transcript` and rolls the result into `state`.
#[derive(Clone, Debug)]
pub struct SolidityTranscript {
    state: [u8; 32],
    transcript: Vec<u8>,
}

impl Default for SolidityTranscript {
    fn default() -> Self {
        Self::new()
    }
}

impl SolidityTranscript {
    /// Create a fresh transcript with zero-state.
    pub fn new() -> Self {
        Self {
            state: [0u8; 32],
            transcript: Vec::new(),
        }
    }

    /// Append raw bytes to the unsqueezed buffer.
    pub fn append_message(&mut self, msg: &[u8]) {
        self.transcript.extend_from_slice(msg);
    }

    /// Append a G1 commitment in Solidity-BE form (`x_be(48) || y_be(48)`).
    /// Callers holding arkworks-LE bytes should convert with
    /// [`arkworks_g1_uncompressed_to_be_xy`] first.
    pub fn append_g1_commitment_be(&mut self, x_be: &[u8; G1_HALF], y_be: &[u8; G1_HALF]) {
        self.transcript.extend_from_slice(x_be);
        self.transcript.extend_from_slice(y_be);
    }

    /// Append a 32-byte BE field element.
    pub fn append_field_elem_be(&mut self, fe_be: &[u8; FR_LEN]) {
        self.transcript.extend_from_slice(fe_be);
    }

    /// Squeeze a 32-byte raw challenge: `state := keccak256(state || transcript);
    /// transcript.clear(); return state`. Caller reduces `mod r` as needed.
    pub fn squeeze(&mut self) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update(self.state);
        hasher.update(&self.transcript);
        let buf = hasher.finalize();
        self.state.copy_from_slice(&buf);
        self.transcript.clear();
        self.state
    }

    /// Test-only inspection: peek at the unsqueezed buffer.
    #[cfg(test)]
    pub(crate) fn buffered_bytes(&self) -> &[u8] {
        &self.transcript
    }

    /// Mirror of jf-plonk's `append_vk_and_pub_input`. Drives the
    /// initial state of the transcript before the verifier consumes
    /// the proof. `srs_g2_compressed` is `to_bytes!(&vk.open_key.powers_of_h[1])`
    /// (arkworks-compressed, 96 LE bytes). Public inputs must already
    /// be in BE form.
    ///
    /// The 12-byte zero pad after the three header fields is the
    /// EVM-word-alignment quirk from `SolidityTranscript`: 4 (field
    /// modulus bits) + 8 (domain size) + 8 (input size) = 20 bytes;
    /// pad to the 32-byte EVM word boundary.
    pub fn append_vk_and_public_inputs(
        &mut self,
        vk: &ParsedVerifyingKey,
        srs_g2_compressed: &[u8; G2_LEN / 2],
        public_inputs_be: &[[u8; FR_LEN]],
    ) {
        // 1. field size in bits — 4 bytes BE u32
        self.append_message(&FR_MODULUS_BITS.to_be_bytes());
        // 2. domain size — 8 bytes BE u64
        self.append_message(&vk.domain_size.to_be_bytes());
        // 3. input size — 8 bytes BE u64
        self.append_message(&vk.num_inputs.to_be_bytes());
        // 4. EVM-word-alignment pad
        self.append_message(&[0u8; 12]);
        // 5. SRS G2 element — 96 bytes compressed LE
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
        // Sanity bounds — prevents silently feeding mis-sized inputs.
        debug_assert_eq!(vk.k_constants.len(), NUM_K_CONSTANTS);
        debug_assert_eq!(vk.selector_commitments.len(), NUM_SELECTOR_COMMS);
        debug_assert_eq!(vk.sigma_commitments.len(), NUM_SIGMA_COMMS);
    }
}

// ---------------------------------------------------------------------------
// Byte-format conversions: arkworks-uncompressed-LE ↔ Solidity-BE.
// ---------------------------------------------------------------------------

/// Reverse a 32-byte LE Fr representation into 32 BE bytes. arkworks
/// `Fr::serialize_uncompressed` writes LE; jf-plonk's transcript
/// expects `Fr::into_bigint().to_bytes_be()` form.
///
/// Identical to a byte-reverse — Fr has no flag bits, so the high
/// byte is unmasked.
pub fn arkworks_fr_le_to_be(le: &[u8; FR_LEN]) -> [u8; FR_LEN] {
    let mut out = [0u8; FR_LEN];
    for i in 0..FR_LEN {
        out[i] = le[FR_LEN - 1 - i];
    }
    out
}

/// Split a 96-byte arkworks-uncompressed G1 into `(x_be, y_be)`.
///
/// arkworks-bls12-381 0.5 has a custom serialiser for its SWCurveConfig
/// (`curves/util.rs::serialize_fq` + `EncodingFlags::encode_flags`)
/// that writes BLS12-381 G1 in **big-endian** bytes (IETF / EIP-2537
/// canonical encoding) and packs flag bits in the **top 3 bits of
/// `bytes[0]`** — the high byte of x:
///
///   bit 7: compression flag (0 for uncompressed)
///   bit 6: infinity flag
///   bit 5: lexographically-largest sort flag (compressed-only)
///
/// We mask those bits off so callers get the canonical x regardless
/// of the point's encoding flags. y has no flag bits in this format
/// but we mask its high byte too for defence-in-depth (BLS12-381
/// base-field values fit in the bottom 5 bits of byte 0 since `p` is
/// 381 bits, so the mask is a no-op on valid y values).
///
/// Note the asymmetry vs [`arkworks_fr_le_to_be`]: the **scalar**
/// field Fr serialises LE (`[lsb, …, msb]`) while the **base** field
/// Fp serialises BE. That's an arkworks-bls12-381 quirk, not a typo.
pub fn arkworks_g1_uncompressed_to_be_xy(
    bytes: &[u8; G1_LEN],
) -> ([u8; G1_HALF], [u8; G1_HALF]) {
    let mut x_be = [0u8; G1_HALF];
    let mut y_be = [0u8; G1_HALF];
    x_be.copy_from_slice(&bytes[0..G1_HALF]);
    y_be.copy_from_slice(&bytes[G1_HALF..G1_LEN]);
    x_be[0] &= 0x1F;
    y_be[0] &= 0x1F;
    (x_be, y_be)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bls12_381_v05::{Bls12_381, Fr, G1Affine};
    use ark_ec_v05::AffineRepr;
    use ark_ff_v05::{BigInteger, PrimeField};
    use ark_serialize_v05::{CanonicalDeserialize, CanonicalSerialize};
    use jf_plonk::{
        proof_system::structs::VerifyingKey,
        transcript::{PlonkTranscript, SolidityTranscript as JfTranscript},
    };

    use crate::circuit::plonk::baker::bake_membership_vk;
    use crate::circuit::plonk::vk_format::parse_vk_bytes;

    /// G1 conversion round-trips through arkworks `xy()`.
    #[test]
    fn g1_le_to_be_matches_arkworks_xy() {
        let g = G1Affine::generator();
        let mut le_bytes = [0u8; G1_LEN];
        g.serialize_uncompressed(&mut le_bytes[..]).unwrap();

        let (x_be, y_be) = arkworks_g1_uncompressed_to_be_xy(&le_bytes);

        let (x, y) = g.xy().unwrap();
        let expected_x_be = x.into_bigint().to_bytes_be();
        let expected_y_be = y.into_bigint().to_bytes_be();
        assert_eq!(x_be.as_slice(), expected_x_be.as_slice(), "x_be mismatch");
        assert_eq!(y_be.as_slice(), expected_y_be.as_slice(), "y_be mismatch");
    }

    /// Fr conversion matches arkworks `into_bigint().to_bytes_be()`.
    #[test]
    fn fr_le_to_be_matches_arkworks() {
        for &raw in &[1u64, 42, 1u64 << 50, u64::MAX] {
            let f = Fr::from(raw);
            let mut le = [0u8; FR_LEN];
            f.serialize_uncompressed(&mut le[..]).unwrap();
            let be = arkworks_fr_le_to_be(&le);
            let expected = f.into_bigint().to_bytes_be();
            assert_eq!(be.as_slice(), expected.as_slice(), "raw={raw}");
        }
    }

    /// Squeezing a fresh transcript matches `Keccak256(0^32)` —
    /// catches silly state-init mistakes.
    #[test]
    fn squeeze_empty_matches_keccak_of_zeros() {
        let mut t = SolidityTranscript::new();
        let chal = t.squeeze();

        let expected = Keccak256::digest([0u8; 32]);
        assert_eq!(chal.as_slice(), expected.as_slice());
    }

    /// Diagnostic: walk through `append_vk_and_public_inputs` step
    /// by step, recreating jf-plonk's byte sequence by hand, and assert
    /// the cumulative buffered bytes match after each step. Pinpoints
    /// the exact diverging append if the oracle test fails.
    #[test]
    fn append_vk_step_by_step_matches_manual_byte_stream() {
        let vk_bytes = bake_membership_vk(5).expect("bake d=5");
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let oracle_vk: VerifyingKey<Bls12_381> =
            VerifyingKey::deserialize_uncompressed(&vk_bytes[..]).unwrap();

        let public_inputs_fr: Vec<Fr> = vec![Fr::from(7u64), Fr::from(13u64)];
        let public_inputs_be: Vec<[u8; FR_LEN]> = public_inputs_fr
            .iter()
            .map(|fr| {
                let bytes = fr.into_bigint().to_bytes_be();
                let mut arr = [0u8; FR_LEN];
                arr.copy_from_slice(&bytes);
                arr
            })
            .collect();

        let mut srs_g2_compressed = [0u8; G2_LEN / 2];
        oracle_vk.open_key.powers_of_h[1]
            .serialize_compressed(&mut srs_g2_compressed[..])
            .unwrap();

        // What our port buffers
        let mut ours = SolidityTranscript::new();
        ours.append_vk_and_public_inputs(&parsed_vk, &srs_g2_compressed, &public_inputs_be);
        let ours_buf = ours.buffered_bytes().to_vec();

        // What jf-plonk's logic would produce, recreated manually
        let mut expected = Vec::new();
        expected.extend_from_slice(&FR_MODULUS_BITS.to_be_bytes());
        expected.extend_from_slice(&(oracle_vk.domain_size as u64).to_be_bytes());
        expected.extend_from_slice(&(oracle_vk.num_inputs as u64).to_be_bytes());
        expected.extend_from_slice(&[0u8; 12]);
        expected.extend_from_slice(&srs_g2_compressed);
        for k in &oracle_vk.k {
            expected.extend_from_slice(&k.into_bigint().to_bytes_be());
        }
        // Mirror jf-plonk's `append_commitment`: substitute (0, 0) for
        // points at infinity. Some selector polynomials are zero, so
        // their commitments are the identity G1 element.
        let g1_xy_be_or_zero = |p: &<Bls12_381 as ark_ec_v05::pairing::Pairing>::G1Affine| {
            use ark_bls12_381_v05::Fq;
            use ark_ff_v05::Zero;
            if p.is_zero() {
                let zero_bytes = Fq::zero().into_bigint().to_bytes_be();
                (zero_bytes.clone(), zero_bytes)
            } else {
                let (x, y) = p.xy().unwrap();
                (x.into_bigint().to_bytes_be(), y.into_bigint().to_bytes_be())
            }
        };
        for sel in &oracle_vk.selector_comms {
            let (x, y) = g1_xy_be_or_zero(&sel.0);
            expected.extend_from_slice(&x);
            expected.extend_from_slice(&y);
        }
        for sig in &oracle_vk.sigma_comms {
            let (x, y) = g1_xy_be_or_zero(&sig.0);
            expected.extend_from_slice(&x);
            expected.extend_from_slice(&y);
        }
        for pi in &public_inputs_fr {
            expected.extend_from_slice(&pi.into_bigint().to_bytes_be());
        }

        if ours_buf != expected {
            // Find first divergence offset for an actionable error message.
            let off = ours_buf
                .iter()
                .zip(expected.iter())
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| ours_buf.len().min(expected.len()));
            panic!(
                "buffered bytes diverge at offset {off}: \
                 ours[{off}..{}]={:02x?} expected[{off}..{}]={:02x?}",
                (off + 8).min(ours_buf.len()),
                &ours_buf[off..(off + 8).min(ours_buf.len())],
                (off + 8).min(expected.len()),
                &expected[off..(off + 8).min(expected.len())],
            );
        }
    }

    /// Run jf-plonk's `SolidityTranscript` and our port on the same
    /// inputs (drawn from a real baked VK + canonical proof). Both
    /// must produce byte-equal challenge sequences after each squeeze.
    /// This is the load-bearing test: byte-equality across the
    /// transcript flow guarantees the verifier challenges agree.
    #[test]
    fn transcript_matches_jf_plonk_oracle() {
        // Build a real VK + proof at depth=5.
        let vk_bytes = bake_membership_vk(5).expect("bake d=5");
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let oracle_vk: VerifyingKey<Bls12_381> =
            VerifyingKey::deserialize_uncompressed(&vk_bytes[..]).unwrap();

        // Build a canonical proof to extract a consistent set of
        // commitments + evaluations to feed into both transcripts.
        use crate::circuit::plonk::baker::build_canonical_membership_witness;
        use crate::circuit::plonk::membership::synthesize_membership;
        use crate::prover::plonk;
        use jf_relation::PlonkCircuit;
        use rand_chacha::rand_core::SeedableRng;

        let depth = 5usize;
        let witness = build_canonical_membership_witness(depth);
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_membership(&mut circuit, &witness).unwrap();
        circuit.finalize_for_arithmetization().unwrap();
        let keys = plonk::preprocess(&circuit).unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let oracle_proof =
            plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();

        let public_inputs_fr: Vec<Fr> =
            vec![witness.commitment, Fr::from(witness.epoch)];
        let public_inputs_be: Vec<[u8; FR_LEN]> = public_inputs_fr
            .iter()
            .map(|fr| {
                let bytes = fr.into_bigint().to_bytes_be();
                let mut arr = [0u8; FR_LEN];
                arr.copy_from_slice(&bytes);
                arr
            })
            .collect();

        // ---- Reference transcript (jf-plonk) ----
        let mut jf = <JfTranscript as PlonkTranscript<
            <Bls12_381 as ark_ec_v05::pairing::Pairing>::BaseField,
        >>::new(b"membership-test");
        <JfTranscript as PlonkTranscript<_>>::append_vk_and_pub_input::<Bls12_381, _>(
            &mut jf,
            &oracle_vk,
            &public_inputs_fr,
        )
        .unwrap();

        // ---- Our port ----
        let mut srs_g2_compressed = [0u8; G2_LEN / 2];
        oracle_vk
            .open_key
            .powers_of_h[1]
            .serialize_compressed(&mut srs_g2_compressed[..])
            .unwrap();

        let mut ours = SolidityTranscript::new();
        ours.append_vk_and_public_inputs(
            &parsed_vk,
            &srs_g2_compressed,
            &public_inputs_be,
        );

        // After append_vk_and_public_inputs, both transcripts must
        // squeeze the same first challenge.
        let jf_chal_1 = <JfTranscript as PlonkTranscript<_>>::get_challenge::<Bls12_381>(
            &mut jf,
            b"chal-1",
        )
        .unwrap();
        let our_chal_1 = ours.squeeze();
        assert_eq!(
            jf_chal_1.into_bigint().to_bytes_be().as_slice(),
            // jf returns Fr — reducing mod r.  We return raw bytes;
            // for a fair comparison reduce on our side too.
            arkworks_fr_le_to_be(&{
                let mut le = [0u8; FR_LEN];
                Fr::from_be_bytes_mod_order(&our_chal_1)
                    .serialize_uncompressed(&mut le[..])
                    .unwrap();
                le
            })
            .as_slice(),
            "first challenge after append_vk_and_public_inputs",
        );

        // Now mirror the verifier's commit → squeeze flow on a few
        // wire commitments. We append wire commitments in
        // arkworks-uncompressed form on our side, going through the
        // BE conversion helper; jf-plonk uses its own append_commitment.
        for (i, wire_comm) in oracle_proof
            .wires_poly_comms
            .iter()
            .enumerate()
            .take(3)
        {
            <JfTranscript as PlonkTranscript<_>>::append_commitment::<Bls12_381, _>(
                &mut jf,
                b"wire",
                wire_comm,
            )
            .unwrap();

            let mut wc_le = [0u8; G1_LEN];
            wire_comm.0.serialize_uncompressed(&mut wc_le[..]).unwrap();
            let (x_be, y_be) = arkworks_g1_uncompressed_to_be_xy(&wc_le);
            ours.append_g1_commitment_be(&x_be, &y_be);

            let jf_chal = <JfTranscript as PlonkTranscript<_>>::get_challenge::<Bls12_381>(
                &mut jf,
                b"after-wire",
            )
            .unwrap();
            let our_raw = ours.squeeze();
            let our_reduced = Fr::from_be_bytes_mod_order(&our_raw);
            assert_eq!(
                jf_chal, our_reduced,
                "challenge mismatch after wire commitment {i}"
            );
        }
    }
}

