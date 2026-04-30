//! `compute_challenges` Soroban port — drives [`SolidityTranscript`]
//! through the canonical TurboPlonk byte stream (VK + public inputs +
//! commitments + evaluations + opening proofs) to derive the six
//! Fiat-Shamir challenges β, γ, α, ζ, v, u.
//!
//! Mirrors `sep-xxxx-circuits::circuit::plonk::verifier_challenges`
//! (PR #179), which is itself a verbatim port of jf-plonk's
//! `Verifier::compute_challenges` for the no-Plookup, single-instance
//! case our membership circuits use.
//!
//! Output is six raw 32-byte BE challenges. Caller reduces `mod r`
//! via `Fr::from_bytes(BytesN<32>)` (which Soroban host fns
//! conveniently take by `BytesN<32>` / `Fr` directly).
//!
//! ## Append sequence (verbatim from jf-plonk)
//!
//! ```text
//! transcript = SolidityTranscript::new()
//! transcript.append_vk_and_public_inputs(vk, srs_g2_compressed, public_inputs_be)
//! for wc in proof.wire_commitments:
//!     transcript.append_g1(wc)
//! beta  = transcript.squeeze()
//! gamma = transcript.squeeze()
//! transcript.append_g1(proof.prod_perm_commitment)
//! alpha = transcript.squeeze()
//! for qc in proof.split_quot_commitments:
//!     transcript.append_g1(qc)
//! zeta  = transcript.squeeze()
//! for ev in proof.wires_evals:        transcript.append_fr(ev)
//! for ev in proof.wire_sigma_evals:   transcript.append_fr(ev)
//! transcript.append_fr(proof.perm_next_eval)
//! v     = transcript.squeeze()
//! transcript.append_g1(proof.opening_proof)
//! transcript.append_g1(proof.shifted_opening_proof)
//! u     = transcript.squeeze()
//! ```

use soroban_sdk::Env;

use crate::proof_format::ParsedProof;
use crate::transcript::{
    arkworks_fr_le_to_be, arkworks_g1_uncompressed_to_be_xy, SolidityTranscript,
};
use crate::vk_format::{ParsedVerifyingKey, FR_LEN, G2_COMPRESSED_LEN};

/// Six Fiat-Shamir challenges produced by [`compute_challenges`], all
/// as raw 32-byte BE bytes. Caller reduces `mod r` via Soroban
/// `Fr::from_bytes(BytesN<32>)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Challenges {
    pub beta: [u8; FR_LEN],
    pub gamma: [u8; FR_LEN],
    pub alpha: [u8; FR_LEN],
    pub zeta: [u8; FR_LEN],
    pub v: [u8; FR_LEN],
    pub u: [u8; FR_LEN],
}

/// Drive the transcript through the canonical proof byte stream and
/// derive the six challenges. See module docs for the exact append
/// sequence.
///
/// ## `srs_g2_compressed` contract
///
/// `srs_g2_compressed` is the **compressed** (96 B BE) form of
/// `vk.open_key.powers_of_h[1]`, computed off-chain via
/// `to_bytes!(...)` (arkworks `serialize_compressed`). The VK
/// already carries the **uncompressed** form in
/// `vk.open_key_powers_of_h[1]` (192 B); the two are different
/// representations of the same G2 point.
///
/// We require the compressed form as a separate argument, rather
/// than deriving it on-chain, because Soroban's `bls12_381` host
/// primitives don't currently expose a "compress this G2 point"
/// operation — and computing the compressed sign bit requires
/// comparing `y` against `-y`, which would mean lifting the
/// uncompressed bytes through field-element arithmetic. The
/// contract bake-vk pipeline therefore ships **both** the VK
/// (uncompressed) and the compressed G2 element pre-computed.
///
/// **Caller invariant**: `srs_g2_compressed` must be the compressed
/// representation of `vk.open_key_powers_of_h[1]`. There's no
/// runtime check; passing inconsistent forms produces a transcript
/// that diverges from what the prover used, so verification rejects
/// silently. The contract entry point should source both fields
/// from the same baked VK fixture and either include them as a
/// single bundle or derive the compressed form once at deploy time.
pub fn compute_challenges(
    env: &Env,
    vk: &ParsedVerifyingKey,
    srs_g2_compressed: &[u8; G2_COMPRESSED_LEN],
    public_inputs_be: &[[u8; FR_LEN]],
    proof: &ParsedProof,
) -> Challenges {
    // Cross-check public-input count against the VK header so a
    // caller mismatch surfaces immediately in dev/test rather than
    // as an opaque "verification failed" later (the transcript would
    // diverge silently and the verifier would reject without
    // explaining why). Release builds keep the function pure-byte;
    // contract entry points that want a runtime check must guard
    // upstream.
    debug_assert_eq!(
        public_inputs_be.len() as u64,
        vk.num_inputs,
        "public_inputs_be.len() != vk.num_inputs — caller bug; transcript would diverge"
    );

    let mut t = SolidityTranscript::new(env);

    // 1. VK + public inputs.
    t.append_vk_and_public_inputs(vk, srs_g2_compressed, public_inputs_be);

    // 2. Wire commitments → β, γ.
    for wc in &proof.wire_commitments {
        let (x, y) = arkworks_g1_uncompressed_to_be_xy(wc);
        t.append_g1_commitment_be(&x, &y);
    }
    let beta = t.squeeze();
    let gamma = t.squeeze();

    // 3. Permutation product commitment → α.
    let (x, y) = arkworks_g1_uncompressed_to_be_xy(&proof.prod_perm_commitment);
    t.append_g1_commitment_be(&x, &y);
    let alpha = t.squeeze();

    // 4. Split quotient commitments → ζ.
    for qc in &proof.split_quot_commitments {
        let (x, y) = arkworks_g1_uncompressed_to_be_xy(qc);
        t.append_g1_commitment_be(&x, &y);
    }
    let zeta = t.squeeze();

    // 5. Polynomial evaluations → v.
    //    Order: wires_evals, wire_sigma_evals, perm_next_eval.
    for ev_le in &proof.wires_evals {
        let ev_be = arkworks_fr_le_to_be(ev_le);
        t.append_field_elem_be(&ev_be);
    }
    for ev_le in &proof.wire_sigma_evals {
        let ev_be = arkworks_fr_le_to_be(ev_le);
        t.append_field_elem_be(&ev_be);
    }
    let perm_next_be = arkworks_fr_le_to_be(&proof.perm_next_eval);
    t.append_field_elem_be(&perm_next_be);
    let v = t.squeeze();

    // 6. Opening proofs → u.
    let (x, y) = arkworks_g1_uncompressed_to_be_xy(&proof.opening_proof);
    t.append_g1_commitment_be(&x, &y);
    let (x, y) = arkworks_g1_uncompressed_to_be_xy(&proof.shifted_opening_proof);
    t.append_g1_commitment_be(&x, &y);
    let u = t.squeeze();

    Challenges {
        beta,
        gamma,
        alpha,
        zeta,
        v,
        u,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha3::{Digest, Keccak256};
    use soroban_sdk::Env;

    use crate::proof_format::{parse_proof_bytes, G1_LEN, NUM_WIRE_SIGMA_EVALS, NUM_WIRE_TYPES};
    use crate::test_fixtures::{build_synthetic_proof_bytes, build_synthetic_vk_bytes};
    use crate::vk_format::{parse_vk_bytes, NUM_K_CONSTANTS, NUM_SELECTOR_COMMS, NUM_SIGMA_COMMS};

    // Tamper-test byte offsets derived from `proof_format`'s layout
    // table. If the layout shifts, these compute to the new values
    // automatically rather than breaking with confusing offset
    // diagnostics.
    const TAMPER_OFF_WIRE_FIRST: usize = 8; // u64 length prefix
    const TAMPER_OFF_OPENING: usize =
        TAMPER_OFF_WIRE_FIRST + NUM_WIRE_TYPES * G1_LEN + G1_LEN + 8 + NUM_WIRE_TYPES * G1_LEN; // 1072
    const TAMPER_OFF_SIGMA_EVAL_FIRST: usize =
        TAMPER_OFF_OPENING + G1_LEN + G1_LEN + 8 + NUM_WIRE_TYPES * FR_LEN + 8; // 1440
    const TAMPER_OFF_PERM_NEXT_EVAL: usize =
        TAMPER_OFF_SIGMA_EVAL_FIRST + NUM_WIRE_SIGMA_EVALS * FR_LEN; // 1568

    /// Synthetic SRS G2 element + public inputs used by the tests.
    fn synthetic_srs_g2_compressed() -> [u8; G2_COMPRESSED_LEN] {
        let mut a = [0u8; G2_COMPRESSED_LEN];
        a[0] = 0xDE;
        a[1] = 0xAD;
        a
    }

    fn synthetic_public_inputs() -> [[u8; FR_LEN]; 2] {
        let mut p = [[0u8; FR_LEN]; 2];
        p[0][0] = 0x70;
        p[1][0] = 0x71;
        p
    }

    /// Determinism: `compute_challenges` on the same inputs produces
    /// byte-identical output across two runs.
    #[test]
    fn compute_challenges_is_deterministic() {
        let env = Env::default();

        let vk_bytes = build_synthetic_vk_bytes(8192, 2);
        let proof_bytes = build_synthetic_proof_bytes();
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof = parse_proof_bytes(&proof_bytes).expect("parse proof");
        let srs_g2 = synthetic_srs_g2_compressed();
        let pis = synthetic_public_inputs();

        let a = compute_challenges(&env, &parsed_vk, &srs_g2, &pis, &parsed_proof);
        let b = compute_challenges(&env, &parsed_vk, &srs_g2, &pis, &parsed_proof);
        assert_eq!(a, b);
    }

    /// Tampering with the proof's first wire commitment changes the
    /// derived challenges. Catches a bug where the proof input is
    /// silently dropped.
    #[test]
    fn compute_challenges_changes_when_proof_is_tampered() {
        let env = Env::default();

        let vk_bytes = build_synthetic_vk_bytes(8192, 2);
        let proof_bytes_a = build_synthetic_proof_bytes();
        let mut proof_bytes_b = proof_bytes_a;
        // Flip a byte inside wire_commitments[0]'s body. The flag-
        // strip mask only touches bytes[0] of the slot; flip a body
        // byte (offset 47) so structural fields stay valid.
        proof_bytes_b[TAMPER_OFF_WIRE_FIRST + 47] ^= 0x01;

        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof_a = parse_proof_bytes(&proof_bytes_a).expect("parse a");
        let parsed_proof_b = parse_proof_bytes(&proof_bytes_b).expect("parse b");
        let srs_g2 = synthetic_srs_g2_compressed();
        let pis = synthetic_public_inputs();

        let a = compute_challenges(&env, &parsed_vk, &srs_g2, &pis, &parsed_proof_a);
        let b = compute_challenges(&env, &parsed_vk, &srs_g2, &pis, &parsed_proof_b);
        assert_ne!(a, b);
    }

    /// Tampering with a public input changes the challenges.
    /// Catches a bug where public inputs aren't consumed by the
    /// transcript.
    #[test]
    fn compute_challenges_changes_when_public_input_is_tampered() {
        let env = Env::default();

        let vk_bytes = build_synthetic_vk_bytes(8192, 2);
        let proof_bytes = build_synthetic_proof_bytes();
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof = parse_proof_bytes(&proof_bytes).expect("parse proof");
        let srs_g2 = synthetic_srs_g2_compressed();
        let pis_a = synthetic_public_inputs();
        let mut pis_b = synthetic_public_inputs();
        pis_b[0][FR_LEN - 1] ^= 0x01;

        let a = compute_challenges(&env, &parsed_vk, &srs_g2, &pis_a, &parsed_proof);
        let b = compute_challenges(&env, &parsed_vk, &srs_g2, &pis_b, &parsed_proof);
        assert_ne!(a, b);

        // Sanity: a's challenges are stable regardless.
        let a2 = compute_challenges(&env, &parsed_vk, &srs_g2, &pis_a, &parsed_proof);
        assert_eq!(a, a2);
    }

    /// **Load-bearing.** Re-derives **all six** challenges via an
    /// independent `sha3::Keccak256` walk through the same byte
    /// sequence `compute_challenges` produces, and asserts each
    /// matches. Pins the entire append sequence end-to-end, not just
    /// β.
    ///
    /// Why all six and not just β: `squeeze` rolls state, so a tamper
    /// test that flips a byte in (say) `wire_commitments[0]` would
    /// surface a β change and propagate through every downstream
    /// challenge regardless of whether the rest of the byte stream
    /// is correct. Without per-challenge oracles, silent drops /
    /// reorderings of `prod_perm_commitment`,
    /// `split_quot_commitments`, evaluations, or opening proofs
    /// could pass tamper tests but break verification.
    #[test]
    fn all_six_challenges_match_sha3_oracle() {
        let env = Env::default();

        let domain_size = 8192u64;
        let num_inputs = 2u64;
        let vk_bytes = build_synthetic_vk_bytes(domain_size, num_inputs);
        let proof_bytes = build_synthetic_proof_bytes();
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof = parse_proof_bytes(&proof_bytes).expect("parse proof");
        let srs_g2 = synthetic_srs_g2_compressed();
        let pis = synthetic_public_inputs();

        let challenges =
            compute_challenges(&env, &parsed_vk, &srs_g2, &pis, &parsed_proof);

        // ----- Re-derive each challenge step-by-step via sha3. -----
        let mut state = [0u8; 32];

        // β-step: state || (vk header + 5×wire_commits)
        let mut hasher = Keccak256::new();
        hasher.update(state);
        hasher.update(crate::transcript::FR_MODULUS_BITS.to_be_bytes());
        hasher.update(domain_size.to_be_bytes());
        hasher.update(num_inputs.to_be_bytes());
        hasher.update([0u8; 12]);
        hasher.update(srs_g2);
        for i in 0..NUM_K_CONSTANTS {
            let mut k_be = [0u8; FR_LEN];
            k_be[FR_LEN - 1] = 0x30 + i as u8;
            hasher.update(k_be);
        }
        for i in 0..NUM_SELECTOR_COMMS {
            let mut x_be = [0u8; 48];
            x_be[0] = (0x20u8 + i as u8) & 0x1F;
            hasher.update(x_be);
            hasher.update([0u8; 48]);
        }
        for i in 0..NUM_SIGMA_COMMS {
            let mut x_be = [0u8; 48];
            x_be[0] = (0x10u8 + i as u8) & 0x1F;
            hasher.update(x_be);
            hasher.update([0u8; 48]);
        }
        for pi in &pis {
            hasher.update(pi);
        }
        for i in 0..crate::proof_format::NUM_WIRE_TYPES {
            let mut x_be = [0u8; 48];
            x_be[0] = (0x10u8 + i as u8) & 0x1F;
            hasher.update(x_be);
            hasher.update([0u8; 48]);
        }
        state = hasher.finalize().into();
        assert_eq!(challenges.beta, state, "β oracle mismatch");

        // γ-step: state || (nothing — squeeze hashes state alone)
        let mut hasher = Keccak256::new();
        hasher.update(state);
        state = hasher.finalize().into();
        assert_eq!(challenges.gamma, state, "γ oracle mismatch");

        // α-step: state || prod_perm_commitment
        // Synthetic prod_perm_commitment[0] = 0x20 (LE), & 0x1F = 0x00.
        let mut hasher = Keccak256::new();
        hasher.update(state);
        let mut prod_perm_x = [0u8; 48];
        prod_perm_x[0] = 0x20 & 0x1F;
        hasher.update(prod_perm_x);
        hasher.update([0u8; 48]);
        state = hasher.finalize().into();
        assert_eq!(challenges.alpha, state, "α oracle mismatch");

        // ζ-step: state || 5×split_quot_commitments
        // Synthetic split_quot_commitments[i][0] = 0x30 + i (LE),
        // & 0x1F: 0x30..0x34 → 0x10..0x14.
        let mut hasher = Keccak256::new();
        hasher.update(state);
        for i in 0..crate::proof_format::NUM_WIRE_TYPES {
            let mut x_be = [0u8; 48];
            x_be[0] = (0x30u8 + i as u8) & 0x1F;
            hasher.update(x_be);
            hasher.update([0u8; 48]);
        }
        state = hasher.finalize().into();
        assert_eq!(challenges.zeta, state, "ζ oracle mismatch");

        // v-step: state || 5×wires_evals + 4×wire_sigma_evals + perm_next_eval
        // Synthetic wires_evals[i][0] = 0x50 + i (LE), reversed → BE last byte
        // Synthetic wire_sigma_evals[i][0] = 0x60 + i (LE), reversed → BE last byte
        // Synthetic perm_next_eval[0] = 0x70 (LE), reversed → BE last byte
        let mut hasher = Keccak256::new();
        hasher.update(state);
        for i in 0..crate::proof_format::NUM_WIRE_TYPES {
            let mut ev_be = [0u8; FR_LEN];
            ev_be[FR_LEN - 1] = 0x50 + i as u8;
            hasher.update(ev_be);
        }
        for i in 0..crate::proof_format::NUM_WIRE_SIGMA_EVALS {
            let mut ev_be = [0u8; FR_LEN];
            ev_be[FR_LEN - 1] = 0x60 + i as u8;
            hasher.update(ev_be);
        }
        let mut perm_next_be = [0u8; FR_LEN];
        perm_next_be[FR_LEN - 1] = 0x70;
        hasher.update(perm_next_be);
        state = hasher.finalize().into();
        assert_eq!(challenges.v, state, "v oracle mismatch");

        // u-step: state || opening_proof || shifted_opening_proof
        // Synthetic opening_proof[0] = 0x40, shifted = 0x41 (both LE),
        // & 0x1F = 0x00, 0x01 (top 3 bits of 0x40, 0x41 are 010, but
        // bits 5-7 are 010_00000 = 0x40 → 0x40 & 0x1F = 0x00; 0x41 & 0x1F = 0x01).
        let mut hasher = Keccak256::new();
        hasher.update(state);
        let mut opening_x = [0u8; 48];
        opening_x[0] = 0x40 & 0x1F;
        hasher.update(opening_x);
        hasher.update([0u8; 48]);
        let mut shifted_x = [0u8; 48];
        shifted_x[0] = 0x41 & 0x1F;
        hasher.update(shifted_x);
        hasher.update([0u8; 48]);
        state = hasher.finalize().into();
        assert_eq!(challenges.u, state, "u oracle mismatch");
    }

    /// Tamper test: flipping a byte in `perm_next_eval` changes the
    /// challenges. Without this test, a bug that drops
    /// `perm_next_eval` from the v-step transcript would pass
    /// `compute_challenges_changes_when_proof_is_tampered` (which
    /// only flips a wire commitment).
    #[test]
    fn compute_challenges_changes_when_perm_next_eval_is_tampered() {
        let env = Env::default();

        let vk_bytes = build_synthetic_vk_bytes(8192, 2);
        let proof_bytes_a = build_synthetic_proof_bytes();
        let mut proof_bytes_b = proof_bytes_a;
        // Flip the LSB of `perm_next_eval` (Fr is LE, so byte 0 of
        // the slot = LSB of value). After the LE→BE reversal in the
        // transcript path, this becomes the BE high byte of the
        // eval — directly affects the v-step hash.
        proof_bytes_b[TAMPER_OFF_PERM_NEXT_EVAL] ^= 0x01;

        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof_a = parse_proof_bytes(&proof_bytes_a).expect("parse a");
        let parsed_proof_b = parse_proof_bytes(&proof_bytes_b).expect("parse b");
        let srs_g2 = synthetic_srs_g2_compressed();
        let pis = synthetic_public_inputs();

        let a = compute_challenges(&env, &parsed_vk, &srs_g2, &pis, &parsed_proof_a);
        let b = compute_challenges(&env, &parsed_vk, &srs_g2, &pis, &parsed_proof_b);
        assert_ne!(a, b);
        // Sanity: β and γ are unchanged (perm_next_eval feeds the
        // v-step, after both β and γ have been squeezed).
        assert_eq!(a.beta, b.beta, "β should not be affected");
        assert_eq!(a.gamma, b.gamma, "γ should not be affected");
        assert_eq!(a.alpha, b.alpha, "α should not be affected");
        assert_eq!(a.zeta, b.zeta, "ζ should not be affected");
        assert_ne!(a.v, b.v, "v must reflect perm_next_eval");
        assert_ne!(a.u, b.u, "u must propagate from v via state rolling");
    }

    /// Tamper test: flipping a byte in `wire_sigma_evals[0]` changes
    /// the challenges. Pins that those evals feed the v-step transcript.
    #[test]
    fn compute_challenges_changes_when_wire_sigma_eval_is_tampered() {
        let env = Env::default();

        let vk_bytes = build_synthetic_vk_bytes(8192, 2);
        let proof_bytes_a = build_synthetic_proof_bytes();
        let mut proof_bytes_b = proof_bytes_a;
        // Flip a body byte of `wire_sigma_evals[0]`; LE → byte 0 is
        // the LSB of the value.
        proof_bytes_b[TAMPER_OFF_SIGMA_EVAL_FIRST] ^= 0x01;

        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof_a = parse_proof_bytes(&proof_bytes_a).expect("parse a");
        let parsed_proof_b = parse_proof_bytes(&proof_bytes_b).expect("parse b");
        let srs_g2 = synthetic_srs_g2_compressed();
        let pis = synthetic_public_inputs();

        let a = compute_challenges(&env, &parsed_vk, &srs_g2, &pis, &parsed_proof_a);
        let b = compute_challenges(&env, &parsed_vk, &srs_g2, &pis, &parsed_proof_b);
        assert_ne!(a, b);
        assert_eq!(a.zeta, b.zeta, "ζ should not be affected");
        assert_ne!(a.v, b.v, "v must reflect wire_sigma_evals");
    }

    /// `debug_assert_eq!` on `public_inputs_be.len() == vk.num_inputs`
    /// fires on mismatch so caller bugs surface in dev/test rather
    /// than as an opaque "verification failed" later. Pinned with
    /// `#[should_panic]` against a future relaxation of the guard.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "public_inputs_be.len() != vk.num_inputs")]
    fn rejects_wrong_public_input_count_in_debug() {
        let env = Env::default();
        let vk_bytes = build_synthetic_vk_bytes(8192, 2);
        let proof_bytes = build_synthetic_proof_bytes();
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof = parse_proof_bytes(&proof_bytes).expect("parse proof");
        let srs_g2 = synthetic_srs_g2_compressed();
        // Synthetic VK declares num_inputs=2; pass only 1 public input.
        let too_few: [[u8; FR_LEN]; 1] = [[0x70; FR_LEN]];
        let _ = compute_challenges(&env, &parsed_vk, &srs_g2, &too_few, &parsed_proof);
    }

    /// Tamper test: flipping a byte in `opening_proof` changes the
    /// challenges. Pins that opening_proof feeds the u-step
    /// transcript.
    #[test]
    fn compute_challenges_changes_when_opening_proof_is_tampered() {
        let env = Env::default();

        let vk_bytes = build_synthetic_vk_bytes(8192, 2);
        let proof_bytes_a = build_synthetic_proof_bytes();
        let mut proof_bytes_b = proof_bytes_a;
        // Flip a body byte of `opening_proof` past the flag-mask byte.
        proof_bytes_b[TAMPER_OFF_OPENING + 47] ^= 0x01;

        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof_a = parse_proof_bytes(&proof_bytes_a).expect("parse a");
        let parsed_proof_b = parse_proof_bytes(&proof_bytes_b).expect("parse b");
        let srs_g2 = synthetic_srs_g2_compressed();
        let pis = synthetic_public_inputs();

        let a = compute_challenges(&env, &parsed_vk, &srs_g2, &pis, &parsed_proof_a);
        let b = compute_challenges(&env, &parsed_vk, &srs_g2, &pis, &parsed_proof_b);
        // Only u changes (opening_proof appended after v is squeezed).
        assert_eq!(a.beta, b.beta);
        assert_eq!(a.gamma, b.gamma);
        assert_eq!(a.alpha, b.alpha);
        assert_eq!(a.zeta, b.zeta);
        assert_eq!(a.v, b.v);
        assert_ne!(a.u, b.u, "u must reflect opening_proof");
    }
}
