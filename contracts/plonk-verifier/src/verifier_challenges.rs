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
pub fn compute_challenges(
    env: &Env,
    vk: &ParsedVerifyingKey,
    srs_g2_compressed: &[u8; G2_COMPRESSED_LEN],
    public_inputs_be: &[[u8; FR_LEN]],
    proof: &ParsedProof,
) -> Challenges {
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

    use crate::proof_format::parse_proof_bytes;
    use crate::test_fixtures::{build_synthetic_proof_bytes, build_synthetic_vk_bytes};
    use crate::vk_format::{parse_vk_bytes, NUM_K_CONSTANTS, NUM_SELECTOR_COMMS, NUM_SIGMA_COMMS};

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
        // Flip a byte inside wire_commitments[0]'s body. Skip the
        // 8-byte length prefix; commitment[0] starts at byte 8.
        // The flag-strip mask only touches bytes[0]; flip a body byte
        // (offset 47) so structural fields stay valid.
        proof_bytes_b[8 + 47] ^= 0x01;

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

    /// β is the first challenge — its value equals
    /// `Keccak256(0^32 || vk_bytes || 5×wire_commits)` per jf-plonk's
    /// flow. We compute the expected hash via `sha3` and compare.
    /// This pins both the byte-stream layout and the host keccak256
    /// against the reference Keccak implementation, end-to-end
    /// through the verifier-challenges path.
    #[test]
    fn beta_matches_sha3_oracle_on_synthetic_input() {
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

        // Reconstruct the byte stream β should hash:
        //   FR_MODULUS_BITS BE u32 (4)
        //   domain_size BE u64 (8)
        //   num_inputs BE u64 (8)
        //   12 zeros pad
        //   srs_g2_compressed (96)
        //   k constants (5 × 32 BE)
        //   selector commitments (13 × 96 BE x||y)
        //   sigma commitments (5 × 96 BE x||y)
        //   public inputs (2 × 32 BE)
        //   wire commitments (5 × 96 BE x||y)
        let mut hasher = Keccak256::new();
        hasher.update([0u8; 32]); // initial state
        hasher.update(crate::transcript::FR_MODULUS_BITS.to_be_bytes());
        hasher.update(domain_size.to_be_bytes());
        hasher.update(num_inputs.to_be_bytes());
        hasher.update([0u8; 12]);
        hasher.update(srs_g2);
        for i in 0..NUM_K_CONSTANTS {
            // Synthetic k_constants[i][0] = 0x30 + i (LE), reversed → BE last byte
            let mut k_be = [0u8; FR_LEN];
            k_be[FR_LEN - 1] = 0x30 + i as u8;
            hasher.update(k_be);
        }
        for i in 0..NUM_SELECTOR_COMMS {
            // Synthetic selector_commitments[i][0] = 0x20 + i (LE), masked & 0x1F.
            let mut x_be = [0u8; 48];
            x_be[0] = (0x20u8 + i as u8) & 0x1F;
            hasher.update(x_be);
            hasher.update([0u8; 48]); // y all zero
        }
        for i in 0..NUM_SIGMA_COMMS {
            let mut x_be = [0u8; 48];
            x_be[0] = (0x10u8 + i as u8) & 0x1F;
            hasher.update(x_be);
            hasher.update([0u8; 48]);
        }
        // Public inputs (already in BE form via synthetic helper).
        for pi in &pis {
            hasher.update(pi);
        }
        // Wire commitments: synthetic wire_commitments[i][0] = 0x10 + i,
        // & 0x1F is no-op since 0x10..0x14 < 0x20.
        for i in 0..crate::proof_format::NUM_WIRE_TYPES {
            let mut x_be = [0u8; 48];
            x_be[0] = (0x10u8 + i as u8) & 0x1F;
            hasher.update(x_be);
            hasher.update([0u8; 48]);
        }
        let expected_beta: [u8; 32] = hasher.finalize().into();

        assert_eq!(
            challenges.beta, expected_beta,
            "β diverges from sha3 oracle on synthetic input — \
             check append sequence in compute_challenges or transcript"
        );
    }
}
