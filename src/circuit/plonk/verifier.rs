//! Top-level Soroban-portable PLONK verifier.
//!
//! Wires together the byte-form parsers and the verifier prereqs
//! built in PRs #174, #177–183:
//!
//! 1. [`super::vk_format::parse_vk_bytes`] / [`super::proof_format::parse_proof_bytes`]
//!    structurally validate the byte streams (caller's responsibility
//!    upstream of `verify`).
//! 2. [`super::verifier_challenges::compute_challenges`] drives the
//!    Solidity-flavour Keccak transcript through the public inputs +
//!    proof to derive (β, γ, α, ζ, v, u).
//! 3. [`super::verifier_polys`] computes Z_H(ζ), L_0(ζ), and PI(ζ)
//!    over the evaluation domain.
//! 4. [`super::verifier_lin_poly::compute_lin_poly_constant_term`]
//!    produces the linearisation polynomial constant `r_0`.
//! 5. [`super::verifier_aggregate::aggregate_poly_commitments`]
//!    builds the 30-entry `(scalar, base)` MSM list + 10-entry
//!    `v_uv_buffer` (giving the batched commitment `[D]_1`).
//! 6. [`super::verifier_aggregate_evals::aggregate_evaluations`]
//!    folds the proof's evaluations + `r_0` into the `[E]_1` scalar.
//! 7. **This module's `final_pairing_check`** assembles the two G1
//!    arguments `A` and `B` per Plonk paper Section 8.4 step 12 and
//!    runs the pairing equation `e(A, [τ]_2) ?= e(B, [1]_2)`.
//!
//! For the no-Plookup, single-instance case (membership circuits at
//! depth 5/8/11):
//!
//! ```text
//!   A = opening_proof + u · shifted_opening_proof
//!   B = [D]_1
//!     + ζ · opening_proof
//!     + u · ζ·g · shifted_opening_proof
//!     − [E]_1 · g_1   (where g_1 = vk.open_key.g)
//!
//!   accept iff e(A, [τ]_2) = e(B, [1]_2)
//! ```
//!
//! The pairing-check formulation `e(A, [τ]_2) · e(−B, [1]_2) = 1`
//! lets us batch into a single multi-pairing — what jf-plonk does
//! and what `env.crypto().bls12_381().pairing_check(&[(A, βh), (−B, h)])`
//! does on the Soroban side.
//!
//! ## Soroban portability
//!
//! Public surface:
//!
//! ```rust,ignore
//! pub fn verify(
//!     vk: &ParsedVerifyingKey,
//!     proof: &ParsedProof,
//!     public_inputs_be: &[[u8; 32]],
//! ) -> Result<(), VerifyError>;
//! ```
//!
//! Returns `Ok(())` to accept, `Err(_)` to reject (with reason).
//! Mirrors `crate::prover::plonk::verify`'s `Result<(), PlonkError>`
//! convention. The contract port has the same shape; the only
//! differences are the host-fn substitutes for `Fr` arithmetic, G1
//! MSM, and the final pairing check.
//!
//! ## SRS G2 compressed encoding
//!
//! `compute_challenges` needs `to_bytes!(&vk.open_key.powers_of_h[1])`
//! — the SRS τ in G2, in arkworks-compressed form (96 bytes BE with
//! sign + infinity flags). [`compress_g2_for_transcript`] derives it
//! from the parsed VK's uncompressed `open_key_powers_of_h[1]`. The
//! Soroban contract port can either (a) extend the VK byte format to
//! ship the compressed form alongside the uncompressed (96 extra B
//! per VK), or (b) compute compression on-chain via the host's G2
//! ops if such a primitive exists.

#![cfg(feature = "plonk")]

use ark_bls12_381_v05::{Bls12_381, Fr, G1Affine, G1Projective, G2Affine};
use ark_ec_v05::pairing::{Pairing, PairingOutput};
use ark_ec_v05::{AffineRepr, CurveGroup};
use ark_ff_v05::{One, PrimeField, Zero};
use ark_serialize_v05::{CanonicalDeserialize, CanonicalSerialize};

use crate::circuit::plonk::proof_format::ParsedProof;
use crate::circuit::plonk::verifier_aggregate::{
    aggregate_poly_commitments, ChallengesFr,
};
use crate::circuit::plonk::verifier_aggregate_evals::aggregate_evaluations;
use crate::circuit::plonk::verifier_challenges::compute_challenges;
use crate::circuit::plonk::verifier_lin_poly::compute_lin_poly_constant_term;
use crate::circuit::plonk::verifier_polys::{
    evaluate_pi_poly, evaluate_vanishing_poly, first_and_last_lagrange_coeffs, DomainParams,
};
use crate::circuit::plonk::proof_format::{NUM_WIRE_SIGMA_EVALS, NUM_WIRE_TYPES};
use crate::circuit::plonk::vk_format::{ParsedVerifyingKey, FR_LEN, G2_COMPRESSED_LEN};

/// Errors `verify` can raise. `PairingMismatch` is the verifier's
/// "rejected the proof" outcome; the others reflect malformed-input
/// conditions that should not happen if the caller has run the
/// upstream byte parsers.
#[derive(Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// Public-input count doesn't match what the VK expects.
    BadPublicInputCount { expected: u64, actual: usize },
    /// Pairing equation failed — proof rejected.
    PairingMismatch,
    /// A G1/G2 byte slice from the parsed VK or proof failed
    /// arkworks' on-curve / subgroup / canonicity check. Indicates
    /// an adversarial input that survived the structural parser.
    InvalidPoint,
    /// An Fr byte slice from the parsed VK or proof failed
    /// canonicity check.
    InvalidScalar,
}

impl core::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadPublicInputCount { expected, actual } => write!(
                f,
                "expected {expected} public inputs, got {actual}"
            ),
            Self::PairingMismatch => write!(f, "pairing equation failed; proof rejected"),
            Self::InvalidPoint => write!(f, "G1/G2 point failed curve / subgroup check"),
            Self::InvalidScalar => write!(f, "Fr scalar failed canonicity check"),
        }
    }
}

impl std::error::Error for VerifyError {}

/// Verify a TurboPlonk proof for a circuit using the no-Plookup,
/// single-instance flow. Returns `Ok(())` to accept, `Err(_)` to
/// reject.
///
/// Inputs are byte-form so the function is Soroban-portable. The
/// contract version has the same signature; only the underlying Fr /
/// G1 / G2 / pairing operations differ.
pub fn verify(
    vk: &ParsedVerifyingKey,
    proof: &ParsedProof,
    public_inputs_be: &[[u8; FR_LEN]],
) -> Result<(), VerifyError> {
    // --- 0. Public-input count must match the VK header. ----------
    if public_inputs_be.len() as u64 != vk.num_inputs {
        return Err(VerifyError::BadPublicInputCount {
            expected: vk.num_inputs,
            actual: public_inputs_be.len(),
        });
    }

    // --- 1. Compress the SRS G2 element once for the transcript. --
    let srs_g2_compressed = compress_g2_for_transcript(&vk.open_key_powers_of_h[1])?;

    // --- 2. Drive the transcript and reduce the 6 challenges. -----
    let raw = compute_challenges(vk, &srs_g2_compressed, public_inputs_be, proof);
    let challenges = ChallengesFr {
        beta: Fr::from_be_bytes_mod_order(&raw.beta),
        gamma: Fr::from_be_bytes_mod_order(&raw.gamma),
        alpha: Fr::from_be_bytes_mod_order(&raw.alpha),
        zeta: Fr::from_be_bytes_mod_order(&raw.zeta),
        v: Fr::from_be_bytes_mod_order(&raw.v),
        u: Fr::from_be_bytes_mod_order(&raw.u),
    };

    // --- 3. Domain-derived polynomial evaluations at ζ. -----------
    let params = DomainParams::for_size(vk.domain_size);
    let vanish_eval = evaluate_vanishing_poly(challenges.zeta, &params);
    let (lagrange_1_eval, _lagrange_n_eval) =
        first_and_last_lagrange_coeffs(challenges.zeta, vanish_eval, &params);

    // --- 4. Public-input polynomial evaluation. -------------------
    let public_inputs_fr: Vec<Fr> = public_inputs_be
        .iter()
        .map(|bytes| Fr::from_be_bytes_mod_order(bytes))
        .collect();
    let pi_eval = evaluate_pi_poly(&public_inputs_fr, challenges.zeta, vanish_eval, &params);

    // --- 5. Linearisation-polynomial constant term r_0. -----------
    //
    // Decode proof evaluations LE→Fr once (consumed both here and by
    // aggregate_evaluations).
    let w_evals: [Fr; NUM_WIRE_TYPES] = decode_fr_array(&proof.wires_evals)?;
    let sigma_evals: [Fr; NUM_WIRE_SIGMA_EVALS] = decode_fr_array(&proof.wire_sigma_evals)?;
    let perm_next_eval =
        Fr::deserialize_uncompressed(&proof.perm_next_eval[..]).map_err(|_| VerifyError::InvalidScalar)?;
    let lin_poly_constant = compute_lin_poly_constant_term(
        challenges.alpha,
        challenges.beta,
        challenges.gamma,
        pi_eval,
        lagrange_1_eval,
        &w_evals,
        &sigma_evals,
        perm_next_eval,
    );

    // --- 6. Aggregate poly commitments → MSM-able [D]_1. ----------
    let agg = aggregate_poly_commitments(
        challenges,
        vanish_eval,
        lagrange_1_eval,
        vk,
        proof,
    );
    let d_1 = agg.multi_scalar_multiply();

    // --- 7. Aggregate evaluations → scalar [E]_1. -----------------
    let aggregate_eval = aggregate_evaluations(lin_poly_constant, proof, &agg.v_uv_buffer);

    // --- 8. Pairing check. ----------------------------------------
    final_pairing_check(
        vk,
        proof,
        challenges,
        params.group_gen,
        d_1,
        aggregate_eval,
    )
}

/// Run the final pairing equation
/// `e(A, [τ]_2) ?= e(B, [1]_2)` in the form
/// `e(A, [τ]_2) · e(−B, [1]_2) = 1`.
///
/// `A = opening_proof + u·shifted_opening_proof`
/// `B = [D]_1 + ζ·opening_proof + u·ζ·g·shifted_opening_proof − [E]_1·g_1`
fn final_pairing_check(
    vk: &ParsedVerifyingKey,
    proof: &ParsedProof,
    challenges: ChallengesFr,
    group_gen: Fr,
    d_1: G1Projective,
    aggregate_eval: Fr,
) -> Result<(), VerifyError> {
    let opening = parse_g1(&proof.opening_proof)?;
    let shifted_opening = parse_g1(&proof.shifted_opening_proof)?;
    let g_1 = parse_g1(&vk.open_key_g)?;
    let h = parse_g2(&vk.open_key_h)?;
    let beta_h = parse_g2(&vk.open_key_beta_h)?;

    let zeta = challenges.zeta;
    let u = challenges.u;
    let zeta_g = zeta * group_gen;

    // A = opening + u · shifted_opening
    let a = opening.into_group() + shifted_opening.into_group() * u;
    // B = [D]_1 + ζ·opening + u·ζ·g·shifted_opening − [E]_1·g_1
    let b = d_1
        + opening.into_group() * zeta
        + shifted_opening.into_group() * (u * zeta_g)
        - g_1.into_group() * aggregate_eval;

    let a_aff = a.into_affine();
    let neg_b_aff = (-b).into_affine();

    let pairing_product = Bls12_381::multi_pairing(&[a_aff, neg_b_aff], &[beta_h, h]);

    if pairing_product == PairingOutput::<Bls12_381>(<Bls12_381 as Pairing>::TargetField::one()) {
        Ok(())
    } else {
        Err(VerifyError::PairingMismatch)
    }
}

/// Compress an arkworks-uncompressed G2 byte slice into the 96-byte
/// compressed form `compute_challenges` expects for the transcript.
fn compress_g2_for_transcript(uncompressed: &[u8; 192]) -> Result<[u8; G2_COMPRESSED_LEN], VerifyError> {
    let g2 = G2Affine::deserialize_uncompressed(&uncompressed[..])
        .map_err(|_| VerifyError::InvalidPoint)?;
    let mut compressed = [0u8; G2_COMPRESSED_LEN];
    g2.serialize_compressed(&mut compressed[..])
        .map_err(|_| VerifyError::InvalidPoint)?;
    Ok(compressed)
}

fn parse_g1(bytes: &[u8; 96]) -> Result<G1Affine, VerifyError> {
    G1Affine::deserialize_uncompressed(&bytes[..]).map_err(|_| VerifyError::InvalidPoint)
}

fn parse_g2(bytes: &[u8; 192]) -> Result<G2Affine, VerifyError> {
    G2Affine::deserialize_uncompressed(&bytes[..]).map_err(|_| VerifyError::InvalidPoint)
}

fn decode_fr_array<const N: usize>(arrays: &[[u8; FR_LEN]; N]) -> Result<[Fr; N], VerifyError> {
    let mut out = [Fr::zero(); N];
    for (i, bytes) in arrays.iter().enumerate() {
        out[i] = Fr::deserialize_uncompressed(&bytes[..]).map_err(|_| VerifyError::InvalidScalar)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bls12_381_v05::{Bls12_381, Fr};
    use ark_ff_v05::BigInteger;
    use ark_serialize_v05::CanonicalSerialize;
    use jf_plonk::proof_system::structs::VerifyingKey;
    use jf_relation::PlonkCircuit;
    use rand_chacha::rand_core::SeedableRng;

    use crate::circuit::plonk::baker::{
        bake_membership_vk, bake_oneonone_create_vk, bake_tyranny_create_vk,
        bake_tyranny_update_vk, bake_update_vk, build_canonical_membership_witness,
        build_canonical_oneonone_create_witness, build_canonical_tyranny_create_witness,
        build_canonical_tyranny_update_witness, build_canonical_update_witness,
    };
    use crate::circuit::plonk::membership::synthesize_membership;
    use crate::circuit::plonk::oneonone_create::synthesize_oneonone_create;
    use crate::circuit::plonk::proof_format::parse_proof_bytes;
    use crate::circuit::plonk::tyranny::{
        synthesize_tyranny_create, synthesize_tyranny_update,
    };
    use crate::circuit::plonk::update::synthesize_update;
    use crate::circuit::plonk::vk_format::parse_vk_bytes;
    use crate::prover::plonk;

    /// Build a real proof at depth, return (parsed_vk, parsed_proof,
    /// public_inputs_be). Uses the canonical witness so we can reuse
    /// VKs from the bake-vk tool.
    fn build_canonical_artifacts(
        depth: usize,
    ) -> (
        crate::circuit::plonk::vk_format::ParsedVerifyingKey,
        crate::circuit::plonk::proof_format::ParsedProof,
        Vec<[u8; FR_LEN]>,
    ) {
        let vk_bytes = bake_membership_vk(depth).expect("bake vk");
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

        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let parsed_proof = parse_proof_bytes(&proof_bytes).expect("parse proof");

        let public_inputs_fr = vec![witness.commitment, Fr::from(witness.epoch)];
        let public_inputs_be: Vec<[u8; FR_LEN]> = public_inputs_fr
            .iter()
            .map(|fr| {
                let bytes = fr.into_bigint().to_bytes_be();
                let mut arr = [0u8; FR_LEN];
                arr.copy_from_slice(&bytes);
                arr
            })
            .collect();

        // Sanity: VerifyingKey roundtrip via bytes still parses.
        let _: VerifyingKey<Bls12_381> = VerifyingKey::deserialize_uncompressed(
            &bake_membership_vk(depth).expect("bake vk")[..],
        )
        .unwrap();

        (parsed_vk, parsed_proof, public_inputs_be)
    }

    /// **Load-bearing.** A canonical proof verifies at every tier.
    /// This is the test the entire C.2 prereq stack has been
    /// building toward.
    #[test]
    fn accepts_canonical_proof_for_all_tiers() {
        for &depth in &[5usize, 8, 11] {
            let (vk, proof, public_inputs_be) = build_canonical_artifacts(depth);
            let result = verify(&vk, &proof, &public_inputs_be);
            assert!(
                result.is_ok(),
                "depth={depth} verifier rejected a canonical proof: {result:?}",
            );
        }
    }

    /// Tampering with the public commitment (input[0]) flips
    /// acceptance. Catches a bug where public inputs aren't fed into
    /// the transcript correctly.
    #[test]
    fn rejects_tampered_commitment() {
        let (vk, proof, mut public_inputs_be) = build_canonical_artifacts(5);
        // Bump the LSB of the commitment (BE) to change its value
        // without violating canonicity (Fr is reduced mod r).
        public_inputs_be[0][FR_LEN - 1] ^= 0x01;
        let result = verify(&vk, &proof, &public_inputs_be);
        assert_eq!(
            result,
            Err(VerifyError::PairingMismatch),
            "tampered commitment should reject with PairingMismatch",
        );
    }

    /// Tampering with the epoch (input[1]) also rejects. Catches
    /// public-input ordering bugs.
    #[test]
    fn rejects_tampered_epoch() {
        let (vk, proof, mut public_inputs_be) = build_canonical_artifacts(5);
        public_inputs_be[1][FR_LEN - 1] ^= 0x01;
        let result = verify(&vk, &proof, &public_inputs_be);
        assert_eq!(result, Err(VerifyError::PairingMismatch));
    }

    /// Wrong number of public inputs is rejected up-front (no
    /// crypto work).
    #[test]
    fn rejects_wrong_public_input_count() {
        let (vk, proof, public_inputs_be) = build_canonical_artifacts(5);
        let too_few: Vec<_> = public_inputs_be.iter().take(1).copied().collect();
        let result = verify(&vk, &proof, &too_few);
        assert_eq!(
            result,
            Err(VerifyError::BadPublicInputCount {
                expected: vk.num_inputs,
                actual: 1,
            }),
        );
    }

    /// Tampering with the proof's first wire commitment changes
    /// challenges (and the whole verification).
    #[test]
    fn rejects_tampered_wire_commitment() {
        let (vk, mut proof, public_inputs_be) = build_canonical_artifacts(5);
        // Substitute wire_commitments[0] with prod_perm_commitment —
        // still on-curve but now an unrelated G1 point.
        proof.wire_commitments[0] = proof.prod_perm_commitment;
        let result = verify(&vk, &proof, &public_inputs_be);
        assert_eq!(result, Err(VerifyError::PairingMismatch));
    }

    /// A proof produced for one tier doesn't verify against another
    /// tier's VK (cross-tier swap).
    #[test]
    fn rejects_cross_tier_vk_swap() {
        // Build a depth=5 proof.
        let (_vk_5, proof_5, public_inputs_5) = build_canonical_artifacts(5);
        // Verify it against the depth=11 VK.
        let (vk_11, _proof_11, _) = build_canonical_artifacts(11);
        let result = verify(&vk_11, &proof_5, &public_inputs_5);
        assert!(
            matches!(result, Err(_)),
            "depth-5 proof verified against depth-11 VK: {result:?}",
        );
    }

    /// Build the *raw* (pre-parse) bytes the Soroban verifier crate
    /// consumes via `include_bytes!`: the baked VK, the
    /// arkworks-uncompressed proof, the 96-byte compressed `[τ]_2`
    /// the transcript expects, and the BE public-input scalars
    /// concatenated. Reuses the same canonical witness path as
    /// `build_canonical_artifacts`, so any byte the on-chain verifier
    /// sees is byte-identical to what this test asserts on.
    fn build_canonical_artifact_bytes(depth: usize) -> (Vec<u8>, Vec<u8>, [u8; G2_COMPRESSED_LEN], Vec<u8>) {
        let vk_bytes = bake_membership_vk(depth).expect("bake vk");
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

        // Re-derive the 96-byte compressed [τ]_2 the same way the
        // off-chain verifier does at runtime (line 149).
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse vk");
        let srs_g2_compressed =
            super::compress_g2_for_transcript(&parsed_vk.open_key_powers_of_h[1])
                .expect("compress [τ]_2");

        // Public inputs in BE form, concatenated.
        let mut pi_concat = Vec::with_capacity(2 * FR_LEN);
        for fr in [witness.commitment, Fr::from(witness.epoch)] {
            let bytes = fr.into_bigint().to_bytes_be();
            pi_concat.extend_from_slice(&bytes);
        }

        (vk_bytes, proof_bytes, srs_g2_compressed, pi_concat)
    }

    fn build_canonical_tyranny_create_artifact_bytes(
        depth: usize,
    ) -> (Vec<u8>, Vec<u8>, [u8; G2_COMPRESSED_LEN], Vec<u8>) {
        let vk_bytes = bake_tyranny_create_vk(depth).expect("bake tyranny-create vk");
        let witness = build_canonical_tyranny_create_witness(depth);
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_tyranny_create(&mut circuit, &witness).unwrap();
        circuit.finalize_for_arithmetization().unwrap();
        let keys = plonk::preprocess(&circuit).unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let oracle_proof = plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();
        let mut proof_bytes = Vec::new();
        oracle_proof
            .serialize_uncompressed(&mut proof_bytes)
            .unwrap();
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse tyranny-create vk");
        let srs_g2_compressed =
            super::compress_g2_for_transcript(&parsed_vk.open_key_powers_of_h[1])
                .expect("compress");
        // PI: (commitment, 0, admin_pubkey_commitment, group_id_fr).
        let mut pi_concat = Vec::with_capacity(4 * FR_LEN);
        for fr in [
            witness.commitment,
            Fr::from(0u64),
            witness.admin_pubkey_commitment,
            witness.group_id_fr,
        ] {
            pi_concat.extend_from_slice(&fr.into_bigint().to_bytes_be());
        }
        (vk_bytes, proof_bytes, srs_g2_compressed, pi_concat)
    }

    fn build_canonical_tyranny_update_artifact_bytes(
        depth: usize,
    ) -> (Vec<u8>, Vec<u8>, [u8; G2_COMPRESSED_LEN], Vec<u8>) {
        let vk_bytes = bake_tyranny_update_vk(depth).expect("bake tyranny-update vk");
        let witness = build_canonical_tyranny_update_witness(depth);
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_tyranny_update(&mut circuit, &witness).unwrap();
        circuit.finalize_for_arithmetization().unwrap();
        let keys = plonk::preprocess(&circuit).unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let oracle_proof = plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();
        let mut proof_bytes = Vec::new();
        oracle_proof
            .serialize_uncompressed(&mut proof_bytes)
            .unwrap();
        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse tyranny-update vk");
        let srs_g2_compressed =
            super::compress_g2_for_transcript(&parsed_vk.open_key_powers_of_h[1])
                .expect("compress");
        // PI: (c_old, epoch_old, c_new, admin_pubkey_commitment, group_id_fr).
        let mut pi_concat = Vec::with_capacity(5 * FR_LEN);
        for fr in [
            witness.c_old,
            Fr::from(witness.epoch_old),
            witness.c_new,
            witness.admin_pubkey_commitment,
            witness.group_id_fr,
        ] {
            pi_concat.extend_from_slice(&fr.into_bigint().to_bytes_be());
        }
        (vk_bytes, proof_bytes, srs_g2_compressed, pi_concat)
    }

    /// 1v1-create-circuit equivalent of
    /// `build_canonical_artifact_bytes`. Single tier (depth=5);
    /// public-input shape `(commitment, epoch=0)`.
    fn build_canonical_oneonone_create_artifact_bytes(
    ) -> (Vec<u8>, Vec<u8>, [u8; G2_COMPRESSED_LEN], Vec<u8>) {
        let vk_bytes = bake_oneonone_create_vk().expect("bake oneonone-create vk");
        let witness = build_canonical_oneonone_create_witness();
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_oneonone_create(&mut circuit, &witness).unwrap();
        circuit.finalize_for_arithmetization().unwrap();
        let keys = plonk::preprocess(&circuit).unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let oracle_proof = plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();
        let mut proof_bytes = Vec::new();
        oracle_proof
            .serialize_uncompressed(&mut proof_bytes)
            .unwrap();

        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse oneonone-create vk");
        let srs_g2_compressed =
            super::compress_g2_for_transcript(&parsed_vk.open_key_powers_of_h[1])
                .expect("compress [τ]_2");

        // PI: (commitment, epoch=0).
        let mut pi_concat = Vec::with_capacity(2 * FR_LEN);
        for fr in [witness.commitment, Fr::from(0u64)] {
            let bytes = fr.into_bigint().to_bytes_be();
            pi_concat.extend_from_slice(&bytes);
        }
        (vk_bytes, proof_bytes, srs_g2_compressed, pi_concat)
    }

    /// Update-circuit equivalent of `build_canonical_artifact_bytes`.
    /// Returns (vk_bytes, proof_bytes, srs_g2_compressed, pi_concat
    /// for `(c_old, epoch_old, c_new)` BE-encoded). The
    /// `srs_g2_compressed` is identical to the membership one (same
    /// EF KZG SRS); both sides assert this in
    /// `plonk_verifier_fixtures_match_or_regenerate`.
    fn build_canonical_update_artifact_bytes(
        depth: usize,
    ) -> (Vec<u8>, Vec<u8>, [u8; G2_COMPRESSED_LEN], Vec<u8>) {
        let vk_bytes = bake_update_vk(depth).expect("bake update vk");
        let witness = build_canonical_update_witness(depth);
        let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
        synthesize_update(&mut circuit, &witness).unwrap();
        circuit.finalize_for_arithmetization().unwrap();
        let keys = plonk::preprocess(&circuit).unwrap();
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
        let oracle_proof = plonk::prove(&mut rng, &keys.pk, &circuit).unwrap();
        let mut proof_bytes = Vec::new();
        oracle_proof
            .serialize_uncompressed(&mut proof_bytes)
            .unwrap();

        let parsed_vk = parse_vk_bytes(&vk_bytes).expect("parse update vk");
        let srs_g2_compressed =
            super::compress_g2_for_transcript(&parsed_vk.open_key_powers_of_h[1])
                .expect("compress [τ]_2");

        // PI order: (c_old, epoch_old, c_new) — matches Groth16
        // reference's allocation order in src/circuit/update.rs.
        let mut pi_concat = Vec::with_capacity(3 * FR_LEN);
        for fr in [
            witness.c_old,
            Fr::from(witness.epoch_old),
            witness.c_new,
        ] {
            let bytes = fr.into_bigint().to_bytes_be();
            pi_concat.extend_from_slice(&bytes);
        }

        (vk_bytes, proof_bytes, srs_g2_compressed, pi_concat)
    }

    /// Doubles as a fixture **emitter** and a **drift detector** for
    /// the byte streams the Soroban `plonk-verifier` crate consumes
    /// via `include_bytes!`.
    ///
    /// - Default mode: re-runs the canonical-artifacts pipeline at
    ///   each membership tier (depth=5/8/11) and asserts the resulting
    ///   bytes equal what's currently checked into
    ///   `contracts/plonk-verifier/tests/fixtures/`. If the prover
    ///   side ever changes shape (layout-pin tweak, RNG seed change,
    ///   per-tier `DomainParams` regression), this test fails first —
    ///   surfacing the drift before any verifier code is exercised.
    /// - `STELLAR_REGEN_FIXTURES=1`: writes fresh bytes to those
    ///   files instead of asserting. Run when prover output has
    ///   legitimately changed and the on-chain verifier should pick
    ///   up the new bytes.
    ///
    /// Bundling all three tiers matters because the *on-chain*
    /// verifier is what's being shipped — a size-dependent regression
    /// in `DomainParams` / FFT precompute on the Soroban side could
    /// slip past the off-chain `accepts_canonical_proof_for_all_tiers`.
    /// The verifier crate drives `accepts_canonical_proof_d{N}` per
    /// tier against these fixtures, closing that gap.
    #[test]
    fn plonk_verifier_fixtures_match_or_regenerate() {
        use std::fs;
        use std::path::PathBuf;

        let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("contracts/plonk-verifier/tests/fixtures");

        // Defensive guard: `CARGO_MANIFEST_DIR` only resolves to the
        // right path while `sep-xxxx-circuits` sits at workspace root.
        // If the prover crate is ever moved (e.g. into `crates/…`),
        // the join above would silently write to the wrong location;
        // this assertion fails first with a clear message.
        let verifier_manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("contracts/plonk-verifier/Cargo.toml");
        assert!(
            verifier_manifest.exists(),
            "CARGO_MANIFEST_DIR ({}) does not contain \
             contracts/plonk-verifier/Cargo.toml — this test assumes \
             sep-xxxx-circuits sits at workspace root. Update the \
             fixtures_dir derivation if the prover crate has moved.",
            env!("CARGO_MANIFEST_DIR"),
        );

        // Accept only `=1` — `is_ok()` would treat `=0` / `=false` as
        // truthy and silently regenerate, masking drift.
        let regenerate = matches!(
            std::env::var("STELLAR_REGEN_FIXTURES").as_deref(),
            Ok("1")
        );

        if regenerate {
            fs::create_dir_all(&fixtures_dir).expect("create fixtures dir");
        }

        let on_disk = |name: &str| -> Vec<u8> {
            fs::read(fixtures_dir.join(name)).unwrap_or_else(|_| {
                panic!(
                    "fixture {name} missing — run with STELLAR_REGEN_FIXTURES=1 to create it"
                )
            })
        };
        let drift_msg = "fixture has drifted from prover-side canonical \
                         artifacts; rerun this test with STELLAR_REGEN_FIXTURES=1 \
                         to refresh bytes the Soroban plonk-verifier crate ships";

        // [τ]_2 comes from the EF KZG ceremony and is shared across
        // tiers AND across the membership / update circuits. Tracked
        // here so any divergence (which would indicate a baker bug)
        // trips this test before reaching the verifier crate.
        let mut srs_g2_first: Option<[u8; G2_COMPRESSED_LEN]> = None;

        let mut process_tier = |kind: &str,
                                depth: usize,
                                vk_bytes: Vec<u8>,
                                proof_bytes: Vec<u8>,
                                srs_g2_compressed: [u8; G2_COMPRESSED_LEN],
                                pi_concat: Vec<u8>| {
            if let Some(first) = srs_g2_first {
                assert_eq!(
                    first, srs_g2_compressed,
                    "srs-g2 differs between tiers / circuits (baker bug?)"
                );
            } else {
                srs_g2_first = Some(srs_g2_compressed);
            }

            // Membership fixtures keep their bare names (`vk-d5.bin`)
            // for compatibility with PR #193's verifier-crate
            // include_bytes! sites; other circuits get a prefix.
            let prefix = match kind {
                "membership" => "",
                "update" => "update-",
                "tyranny-create" => "tyranny-create-",
                "tyranny-update" => "tyranny-update-",
                _ => unreachable!(),
            };
            let vk_name = format!("{prefix}vk-d{depth}.bin");
            let proof_name = format!("{prefix}proof-d{depth}.bin");
            let pi_name = format!("{prefix}pi-d{depth}.bin");

            if regenerate {
                fs::write(fixtures_dir.join(&vk_name), &vk_bytes).unwrap();
                fs::write(fixtures_dir.join(&proof_name), &proof_bytes).unwrap();
                fs::write(fixtures_dir.join(&pi_name), &pi_concat).unwrap();
            } else {
                assert_eq!(on_disk(&vk_name), vk_bytes, "{vk_name}: {drift_msg}");
                assert_eq!(on_disk(&proof_name), proof_bytes, "{proof_name}: {drift_msg}");
                assert_eq!(on_disk(&pi_name), pi_concat, "{pi_name}: {drift_msg}");
            }
        };

        for &depth in &[5usize, 8, 11] {
            let (vk_bytes, proof_bytes, srs_g2_compressed, pi_concat) =
                build_canonical_artifact_bytes(depth);
            process_tier(
                "membership",
                depth,
                vk_bytes,
                proof_bytes,
                srs_g2_compressed,
                pi_concat,
            );

            let (vk_bytes, proof_bytes, srs_g2_compressed, pi_concat) =
                build_canonical_update_artifact_bytes(depth);
            process_tier(
                "update",
                depth,
                vk_bytes,
                proof_bytes,
                srs_g2_compressed,
                pi_concat,
            );

            let (vk_bytes, proof_bytes, srs_g2_compressed, pi_concat) =
                build_canonical_tyranny_create_artifact_bytes(depth);
            process_tier(
                "tyranny-create",
                depth,
                vk_bytes,
                proof_bytes,
                srs_g2_compressed,
                pi_concat,
            );

            let (vk_bytes, proof_bytes, srs_g2_compressed, pi_concat) =
                build_canonical_tyranny_update_artifact_bytes(depth);
            process_tier(
                "tyranny-update",
                depth,
                vk_bytes,
                proof_bytes,
                srs_g2_compressed,
                pi_concat,
            );
        }

        // 1v1 create — single tier (depth=5). Bare filenames
        // `oneonone-create-{vk,proof,pi}.bin` (no `-d{N}` suffix).
        let (vk_bytes, proof_bytes, srs_g2_compressed, pi_concat) =
            build_canonical_oneonone_create_artifact_bytes();
        if let Some(first) = srs_g2_first {
            assert_eq!(
                first, srs_g2_compressed,
                "oneonone-create srs-g2 differs from membership/update"
            );
        }
        if regenerate {
            fs::write(fixtures_dir.join("oneonone-create-vk.bin"), &vk_bytes).unwrap();
            fs::write(
                fixtures_dir.join("oneonone-create-proof.bin"),
                &proof_bytes,
            )
            .unwrap();
            fs::write(fixtures_dir.join("oneonone-create-pi.bin"), &pi_concat).unwrap();
        } else {
            assert_eq!(
                on_disk("oneonone-create-vk.bin"),
                vk_bytes,
                "oneonone-create-vk.bin: {drift_msg}"
            );
            assert_eq!(
                on_disk("oneonone-create-proof.bin"),
                proof_bytes,
                "oneonone-create-proof.bin: {drift_msg}"
            );
            assert_eq!(
                on_disk("oneonone-create-pi.bin"),
                pi_concat,
                "oneonone-create-pi.bin: {drift_msg}"
            );
        }

        let srs_g2 = srs_g2_first.expect("at least one tier processed");
        if regenerate {
            fs::write(fixtures_dir.join("srs-g2-compressed.bin"), srs_g2).unwrap();
            eprintln!(
                "regenerated plonk-verifier fixtures \
                 (membership + update at d5/d8/d11) at {}",
                fixtures_dir.display()
            );
        } else {
            assert_eq!(
                on_disk("srs-g2-compressed.bin"),
                srs_g2.to_vec(),
                "srs-g2-compressed.bin: {drift_msg}",
            );
        }
    }
}
