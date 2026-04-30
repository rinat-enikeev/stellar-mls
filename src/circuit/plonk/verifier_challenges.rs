//! `compute_challenges` port — drives [`SolidityTranscript`] through
//! the canonical TurboPlonk byte stream (VK + public inputs +
//! commitments + evaluations + opening proofs) to derive the six
//! Fiat-Shamir challenges β, γ, α, ζ, v, u.
//!
//! Mirror of jf-plonk's `Verifier::compute_challenges`
//! (`plonk/src/proof_system/verifier.rs:262-339`), specialised to the
//! single-instance, no-Plookup case our membership circuits use.
//! Soroban-portable: takes byte-form inputs (`ParsedVerifyingKey`,
//! `ParsedProof`, public inputs in BE) and returns raw 32-byte BE
//! challenges. Caller reduces `mod r` (typically via
//! `Fr::from_be_bytes_mod_order` on the prover side or
//! `bls12_381::Fr::from_bytes` host-fn equivalent on the contract
//! side).
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
//!
//! No τ challenge (Plookup absent), no extra-transcript-init message,
//! single-instance only.

#![cfg(feature = "plonk")]

use crate::circuit::plonk::proof_format::ParsedProof;
use crate::circuit::plonk::transcript::{
    arkworks_fr_le_to_be, arkworks_g1_uncompressed_to_be_xy, SolidityTranscript,
};
use crate::circuit::plonk::vk_format::{ParsedVerifyingKey, FR_LEN, G2_COMPRESSED_LEN};

/// Six Fiat-Shamir challenges produced by [`compute_challenges`], all
/// as raw 32-byte BE bytes (caller reduces `mod r`).
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
/// derive (β, γ, α, ζ, v, u). See module docs for the exact sequence.
///
/// `srs_g2_compressed` is `to_bytes!(&vk.open_key.powers_of_h[1])`
/// (arkworks-compressed BE, 96 bytes). `public_inputs_be` are
/// `Fr::into_bigint().to_bytes_be()` form. The proof's commitment
/// and evaluation bytes are arkworks-uncompressed; conversion to
/// Solidity-BE happens inside the appends via the helpers in
/// [`crate::circuit::plonk::transcript`].
pub fn compute_challenges(
    vk: &ParsedVerifyingKey,
    srs_g2_compressed: &[u8; G2_COMPRESSED_LEN],
    public_inputs_be: &[[u8; FR_LEN]],
    proof: &ParsedProof,
) -> Challenges {
    let mut t = SolidityTranscript::new();

    // 1. VK + public inputs (initial transcript header).
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
    use ark_bls12_381_v05::{Bls12_381, Fr};
    use ark_ff_v05::{BigInteger, PrimeField};
    use ark_serialize_v05::{CanonicalDeserialize, CanonicalSerialize};
    use jf_plonk::{
        proof_system::structs::VerifyingKey,
        transcript::{PlonkTranscript, SolidityTranscript as JfTranscript},
    };
    use jf_relation::PlonkCircuit;
    use rand_chacha::rand_core::SeedableRng;

    use crate::circuit::plonk::baker::{bake_membership_vk, build_canonical_membership_witness};
    use crate::circuit::plonk::membership::synthesize_membership;
    use crate::circuit::plonk::proof_format::parse_proof_bytes;
    use crate::circuit::plonk::vk_format::parse_vk_bytes;
    use crate::prover::plonk;

    /// Build a real proof at the given depth, return (vk_bytes,
    /// proof_bytes, oracle_vk, oracle_proof, public_inputs_fr).
    fn build_canonical_artifacts(
        depth: usize,
    ) -> (
        Vec<u8>,
        Vec<u8>,
        VerifyingKey<Bls12_381>,
        jf_plonk::proof_system::structs::Proof<Bls12_381>,
        Vec<Fr>,
    ) {
        let vk_bytes = bake_membership_vk(depth).expect("bake vk");
        let oracle_vk: VerifyingKey<Bls12_381> =
            VerifyingKey::deserialize_uncompressed(&vk_bytes[..]).expect("deserialise vk");

        let witness = build_canonical_membership_witness(depth);
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_membership(&mut circuit, &witness).unwrap();
        circuit.finalize_for_arithmetization().unwrap();
        let keys = plonk::preprocess(&circuit).unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let oracle_proof = plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();

        let mut proof_bytes = Vec::new();
        oracle_proof
            .serialize_uncompressed(&mut proof_bytes)
            .unwrap();

        let public_inputs_fr = vec![witness.commitment, Fr::from(witness.epoch)];

        (
            vk_bytes,
            proof_bytes,
            oracle_vk,
            oracle_proof,
            public_inputs_fr,
        )
    }

    /// Walk jf-plonk's `SolidityTranscript` through the same byte
    /// sequence its own `compute_challenges` uses (mirroring
    /// `verifier.rs:262-339` for the no-Plookup, single-instance
    /// case). Returns the 6 challenges as Fr.
    ///
    /// jf-plonk's `compute_challenges` is `pub(crate)` so we can't
    /// call it directly; this helper re-derives the same challenges
    /// via the same public-API appends + squeezes for use as oracle.
    fn jf_compute_challenges(
        oracle_vk: &VerifyingKey<Bls12_381>,
        oracle_proof: &jf_plonk::proof_system::structs::Proof<Bls12_381>,
        public_inputs_fr: &[Fr],
    ) -> [Fr; 6] {
        let mut t = <JfTranscript as PlonkTranscript<
            <Bls12_381 as ark_ec_v05::pairing::Pairing>::BaseField,
        >>::new(b"PlonkProof");

        <JfTranscript as PlonkTranscript<_>>::append_vk_and_pub_input::<Bls12_381, _>(
            &mut t,
            oracle_vk,
            public_inputs_fr,
        )
        .unwrap();

        <JfTranscript as PlonkTranscript<_>>::append_commitments::<Bls12_381, _>(
            &mut t,
            b"witness_poly_comms",
            &oracle_proof.wires_poly_comms,
        )
        .unwrap();
        let beta = <JfTranscript as PlonkTranscript<_>>::get_challenge::<Bls12_381>(
            &mut t,
            b"beta",
        )
        .unwrap();
        let gamma = <JfTranscript as PlonkTranscript<_>>::get_challenge::<Bls12_381>(
            &mut t,
            b"gamma",
        )
        .unwrap();

        <JfTranscript as PlonkTranscript<_>>::append_commitment::<Bls12_381, _>(
            &mut t,
            b"perm_poly_comms",
            &oracle_proof.prod_perm_poly_comm,
        )
        .unwrap();
        let alpha = <JfTranscript as PlonkTranscript<_>>::get_challenge::<Bls12_381>(
            &mut t,
            b"alpha",
        )
        .unwrap();

        <JfTranscript as PlonkTranscript<_>>::append_commitments::<Bls12_381, _>(
            &mut t,
            b"quot_poly_comms",
            &oracle_proof.split_quot_poly_comms,
        )
        .unwrap();
        let zeta = <JfTranscript as PlonkTranscript<_>>::get_challenge::<Bls12_381>(
            &mut t,
            b"zeta",
        )
        .unwrap();

        <JfTranscript as PlonkTranscript<_>>::append_proof_evaluations::<Bls12_381>(
            &mut t,
            &oracle_proof.poly_evals,
        )
        .unwrap();
        let v =
            <JfTranscript as PlonkTranscript<_>>::get_challenge::<Bls12_381>(&mut t, b"v").unwrap();

        <JfTranscript as PlonkTranscript<_>>::append_commitment::<Bls12_381, _>(
            &mut t,
            b"open_proof",
            &oracle_proof.opening_proof,
        )
        .unwrap();
        <JfTranscript as PlonkTranscript<_>>::append_commitment::<Bls12_381, _>(
            &mut t,
            b"shifted_open_proof",
            &oracle_proof.shifted_opening_proof,
        )
        .unwrap();
        let u =
            <JfTranscript as PlonkTranscript<_>>::get_challenge::<Bls12_381>(&mut t, b"u").unwrap();

        [beta, gamma, alpha, zeta, v, u]
    }

    /// Convert a [u8; 32] BE byte challenge to Fr (mod r).
    fn be_bytes_to_fr(bytes: &[u8; FR_LEN]) -> Fr {
        Fr::from_be_bytes_mod_order(bytes)
    }

    /// Load-bearing: my `compute_challenges` produces the same 6 Fr
    /// challenges as jf-plonk's transcript walk, on real proofs at
    /// all three tiers.
    #[test]
    fn compute_challenges_matches_jf_plonk_oracle_for_all_tiers() {
        for &depth in &[5usize, 8, 11] {
            let (vk_bytes, proof_bytes, oracle_vk, oracle_proof, public_inputs_fr) =
                build_canonical_artifacts(depth);

            let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
            let parsed_proof = parse_proof_bytes(&proof_bytes).expect("parse proof");

            let mut srs_g2_compressed = [0u8; G2_COMPRESSED_LEN];
            oracle_vk
                .open_key
                .powers_of_h[1]
                .serialize_compressed(&mut srs_g2_compressed[..])
                .unwrap();

            let public_inputs_be: Vec<[u8; FR_LEN]> = public_inputs_fr
                .iter()
                .map(|fr| {
                    let bytes = fr.into_bigint().to_bytes_be();
                    let mut arr = [0u8; FR_LEN];
                    arr.copy_from_slice(&bytes);
                    arr
                })
                .collect();

            let ours = compute_challenges(
                &parsed_vk,
                &srs_g2_compressed,
                &public_inputs_be,
                &parsed_proof,
            );
            let oracle = jf_compute_challenges(&oracle_vk, &oracle_proof, &public_inputs_fr);

            let ours_fr = [
                be_bytes_to_fr(&ours.beta),
                be_bytes_to_fr(&ours.gamma),
                be_bytes_to_fr(&ours.alpha),
                be_bytes_to_fr(&ours.zeta),
                be_bytes_to_fr(&ours.v),
                be_bytes_to_fr(&ours.u),
            ];
            let labels = ["beta", "gamma", "alpha", "zeta", "v", "u"];
            for (i, label) in labels.iter().enumerate() {
                assert_eq!(
                    ours_fr[i], oracle[i],
                    "depth={depth} {label} challenge mismatch:\n  ours={:?}\n  oracle={:?}",
                    ours_fr[i], oracle[i],
                );
            }
        }
    }

    /// Determinism: re-running `compute_challenges` on the same inputs
    /// produces byte-identical output. Cheap regression check.
    #[test]
    fn compute_challenges_is_deterministic() {
        let (vk_bytes, proof_bytes, oracle_vk, _, public_inputs_fr) =
            build_canonical_artifacts(5);
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof = parse_proof_bytes(&proof_bytes).expect("parse proof");
        let mut srs_g2_compressed = [0u8; G2_COMPRESSED_LEN];
        oracle_vk
            .open_key
            .powers_of_h[1]
            .serialize_compressed(&mut srs_g2_compressed[..])
            .unwrap();
        let public_inputs_be: Vec<[u8; FR_LEN]> = public_inputs_fr
            .iter()
            .map(|fr| {
                let bytes = fr.into_bigint().to_bytes_be();
                let mut arr = [0u8; FR_LEN];
                arr.copy_from_slice(&bytes);
                arr
            })
            .collect();

        let a = compute_challenges(&parsed_vk, &srs_g2_compressed, &public_inputs_be, &parsed_proof);
        let b = compute_challenges(&parsed_vk, &srs_g2_compressed, &public_inputs_be, &parsed_proof);
        assert_eq!(a, b);
    }

    /// Tampering with the proof bytes changes the challenges (sanity
    /// check — guards against a bug where the proof input is dropped
    /// silently).
    #[test]
    fn compute_challenges_changes_when_proof_is_tampered() {
        let (vk_bytes, proof_bytes, oracle_vk, _, public_inputs_fr) =
            build_canonical_artifacts(5);
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof_a = parse_proof_bytes(&proof_bytes).expect("parse proof");

        // Flip a byte inside the first wire commitment x-coord.
        let mut tampered_bytes = proof_bytes.clone();
        // Skip the 8-byte length prefix; first wire commitment x starts
        // at byte 8.  Flip a low byte to keep the result on-curve-ish
        // for the parser (the parser is structural-only).
        tampered_bytes[8 + 47] ^= 0x01;
        let parsed_proof_b = parse_proof_bytes(&tampered_bytes).expect("parse tampered proof");

        let mut srs_g2_compressed = [0u8; G2_COMPRESSED_LEN];
        oracle_vk
            .open_key
            .powers_of_h[1]
            .serialize_compressed(&mut srs_g2_compressed[..])
            .unwrap();
        let public_inputs_be: Vec<[u8; FR_LEN]> = public_inputs_fr
            .iter()
            .map(|fr| {
                let bytes = fr.into_bigint().to_bytes_be();
                let mut arr = [0u8; FR_LEN];
                arr.copy_from_slice(&bytes);
                arr
            })
            .collect();

        let a = compute_challenges(
            &parsed_vk,
            &srs_g2_compressed,
            &public_inputs_be,
            &parsed_proof_a,
        );
        let b = compute_challenges(
            &parsed_vk,
            &srs_g2_compressed,
            &public_inputs_be,
            &parsed_proof_b,
        );

        assert_ne!(a, b, "challenges did not change when proof was tampered");
    }
}
