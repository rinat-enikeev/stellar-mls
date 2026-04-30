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
        bake_membership_vk, build_canonical_membership_witness,
    };
    use crate::circuit::plonk::membership::synthesize_membership;
    use crate::circuit::plonk::proof_format::parse_proof_bytes;
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
}
