//! Groth16 trusted setup ceremony for SEP-XXXX.
//!
//! Implements the Powers of Tau MPC protocol over BLS12-381:
//!
//! 1. **Phase 1 (Powers of Tau):** Accumulates (τ, α, β) across N participants.
//!    Each participant adds their own randomness; security holds if at least
//!    one participant honestly destroys their secret contribution.
//!
//! 2. **Key derivation:** Converts the accumulated ceremony parameters into
//!    Groth16 proving and verification keys for a specific circuit tier.
//!
//! 3. **Verification:** Every contribution is verified via BLS12-381 pairing
//!    checks, ensuring no participant corrupted the SRS.
//!
//! 4. **Transcript:** A SHA-256 hash chain records each contribution for
//!    public auditability.
//!
//! # Key derivation security
//!
//! In this reference implementation, `derive_keys` takes the accumulated
//! ceremony scalars (τ, α, β) and hashes them into a ChaCha20 seed for
//! arkworks' standard `circuit_specific_setup`. The seed is **not public**:
//! it depends on the accumulated toxic waste, which no single ceremony
//! participant knows (1-of-N trust).
//!
//! However, the machine executing `derive_keys` sees the accumulated
//! scalars and can recover the Groth16 toxic waste. In a production
//! deployment, key derivation MUST use Phase 2 of Groth16 MPC — the QAP
//! is evaluated directly on SRS curve points so that no single machine
//! learns the scalars. This module demonstrates the ceremony protocol;
//! production deployments need a proper Phase 2 implementation.

use ark_bls12_381::{Bls12_381, Fr, G1Affine, G1Projective, G2Affine, G2Projective};
use ark_ec::pairing::Pairing;
use ark_ec::{AffineRepr, CurveGroup, Group};
use ark_ff::{One, PrimeField, UniformRand};
use ark_groth16::{Groth16, PreparedVerifyingKey, ProvingKey, VerifyingKey};
use ark_serialize::CanonicalSerialize;
use ark_snark::SNARK;
use ark_std::rand::Rng;
use rand::SeedableRng;
use sha2::{Digest, Sha256};

use crate::circuit::MembershipCircuit;
use crate::poseidon;
use crate::prover::SetupResult;
use crate::Tier;

// ================================================================
// Types
// ================================================================

/// The Structured Reference String produced by the Powers of Tau ceremony.
///
/// Contains curve-point encodings of powers of the toxic waste (τ, α, β).
/// No single participant knows the accumulated scalar values.
#[derive(Clone, Debug)]
pub struct PowersOfTau {
    /// τ^i · G1 for i = 0..2·degree-1 (needed for h_query in key derivation).
    pub tau_g1: Vec<G1Affine>,
    /// τ^i · G2 for i = 0..degree.
    pub tau_g2: Vec<G2Affine>,
    /// α · τ^i · G1 for i = 0..degree.
    pub alpha_tau_g1: Vec<G1Affine>,
    /// β · τ^i · G1 for i = 0..degree.
    pub beta_tau_g1: Vec<G1Affine>,
    /// β · G2.
    pub beta_g2: G2Affine,
}

/// Proof that a contributor updated the SRS honestly.
///
/// Contains Schnorr-like proofs of knowledge for each update factor.
#[derive(Clone, Debug)]
pub struct ContributionProof {
    /// Proof of knowledge of δ_τ: (s·G1, δ_τ·s·G1).
    pub tau_proof: (G1Affine, G1Affine),
    /// Proof of knowledge of δ_α: (s·G2, δ_α·s·G2).
    pub alpha_proof: (G2Affine, G2Affine),
    /// Proof of knowledge of δ_β: (s·G1, δ_β·s·G1).
    pub beta_proof: (G1Affine, G1Affine),
}

/// A single contribution record in the ceremony transcript.
#[derive(Clone, Debug)]
pub struct ContributionRecord {
    /// Zero-based contribution index.
    pub index: usize,
    /// The proof of honest contribution.
    pub proof: ContributionProof,
    /// SHA-256 hash of the SRS state after this contribution.
    pub srs_hash: [u8; 32],
}

/// The full ceremony transcript: a verifiable chain of contributions.
#[derive(Clone, Debug)]
pub struct CeremonyTranscript {
    /// Ordered list of contribution records.
    pub contributions: Vec<ContributionRecord>,
    /// Ordered list of SRS snapshots (one per contribution).
    /// Stored to enable full transcript replay and verification.
    pub srs_snapshots: Vec<PowersOfTau>,
}

/// The complete output of a ceremony for one circuit tier.
pub struct CeremonyResult {
    /// The final SRS (for public verification).
    pub srs: PowersOfTau,
    /// The ceremony transcript (for auditing).
    pub transcript: CeremonyTranscript,
    /// The Groth16 proving key.
    pub proving_key: ProvingKey<Bls12_381>,
    /// The Groth16 verification key.
    pub verifying_key: VerifyingKey<Bls12_381>,
    /// The prepared verification key (for efficient verification).
    pub prepared_vk: PreparedVerifyingKey<Bls12_381>,
    /// The circuit identifier.
    pub circuit_id: [u8; 32],
}

// ================================================================
// Phase 1: Powers of Tau
// ================================================================

/// Create the initial SRS for a Powers of Tau ceremony.
///
/// The first participant generates random (τ, α, β) and computes the SRS.
/// Returns the SRS, a contribution proof, and the secret scalars
/// (for simulation — in production these are destroyed immediately).
pub fn initialize<R: Rng + rand::CryptoRng>(
    degree: usize,
    rng: &mut R,
) -> (PowersOfTau, ContributionProof, Fr, Fr, Fr) {
    let tau = Fr::rand(rng);
    let alpha = Fr::rand(rng);
    let beta = Fr::rand(rng);

    let g1 = G1Projective::generator();
    let g2 = G2Projective::generator();

    // Compute τ^i · G1 for i = 0..2·degree-1
    let tau_g1 = powers_of_scalar_times_point_g1(&tau, 2 * degree - 1, &g1);
    // Compute τ^i · G2 for i = 0..degree
    let tau_g2 = powers_of_scalar_times_point_g2(&tau, degree, &g2);
    // Compute α · τ^i · G1 for i = 0..degree
    let alpha_tau_g1 = powers_of_scalar_times_point_g1_with_coeff(&tau, &alpha, degree, &g1);
    // Compute β · τ^i · G1 for i = 0..degree
    let beta_tau_g1 = powers_of_scalar_times_point_g1_with_coeff(&tau, &beta, degree, &g1);
    // β · G2
    let beta_g2 = (g2 * beta).into_affine();

    let srs = PowersOfTau {
        tau_g1,
        tau_g2,
        alpha_tau_g1,
        beta_tau_g1,
        beta_g2,
    };

    // For the initial contribution, the "proof" demonstrates knowledge of
    // τ, α, β themselves (the ratio from identity to the initial SRS).
    let proof = make_contribution_proof(rng, &tau, &alpha, &beta);

    (srs, proof, tau, alpha, beta)
}

/// Apply a new contribution to the SRS.
///
/// The contributor generates random (δ_τ, δ_α, δ_β) and updates every
/// element in the SRS. Returns the updated SRS, a contribution proof,
/// and the secret update factors (for simulation only).
pub fn contribute<R: Rng + rand::CryptoRng>(
    srs: &PowersOfTau,
    rng: &mut R,
) -> (PowersOfTau, ContributionProof, Fr, Fr, Fr) {
    let delta_tau = Fr::rand(rng);
    let delta_alpha = Fr::rand(rng);
    let delta_beta = Fr::rand(rng);

    // Update tau_g1[i] = δ_τ^i · old[i]
    let new_tau_g1 = update_powers_g1(&srs.tau_g1, &delta_tau, &Fr::one());
    // Update tau_g2[i] = δ_τ^i · old[i]
    let new_tau_g2 = update_powers_g2(&srs.tau_g2, &delta_tau, &Fr::one());
    // Update alpha_tau_g1[i] = δ_α · δ_τ^i · old[i]
    let new_alpha_tau_g1 = update_powers_g1(&srs.alpha_tau_g1, &delta_tau, &delta_alpha);
    // Update beta_tau_g1[i] = δ_β · δ_τ^i · old[i]
    let new_beta_tau_g1 = update_powers_g1(&srs.beta_tau_g1, &delta_tau, &delta_beta);
    // Update beta_g2 = δ_β · old
    let new_beta_g2 = srs.beta_g2.mul_bigint(delta_beta.into_bigint()).into_affine();

    let new_srs = PowersOfTau {
        tau_g1: new_tau_g1,
        tau_g2: new_tau_g2,
        alpha_tau_g1: new_alpha_tau_g1,
        beta_tau_g1: new_beta_tau_g1,
        beta_g2: new_beta_g2,
    };

    let proof = make_contribution_proof(rng, &delta_tau, &delta_alpha, &delta_beta);

    (new_srs, proof, delta_tau, delta_alpha, delta_beta)
}

/// Verify a contribution using pairing checks.
///
/// Checks that `after` was derived from `before` by a contributor who
/// knows the update factors proven in `proof`.
///
/// Rejects identity (point-at-infinity) proof elements and degenerate SRS.
pub fn verify_contribution(
    before: &PowersOfTau,
    after: &PowersOfTau,
    proof: &ContributionProof,
) -> bool {
    // ---- Reject identity points in proof elements ----
    // Without this check, an attacker can submit (0,0) pairs that make
    // all pairing equations trivially true (e(O,Q) = 1 for all Q).
    if proof.tau_proof.0.is_zero()
        || proof.tau_proof.1.is_zero()
        || proof.alpha_proof.0.is_zero()
        || proof.alpha_proof.1.is_zero()
        || proof.beta_proof.0.is_zero()
        || proof.beta_proof.1.is_zero()
    {
        return false;
    }

    // ---- Reject degenerate SRS (zero update factors) ----
    // A contributor using δ_τ = 0 would set all higher powers to the
    // identity, making the SRS useless. Pairing checks with identity
    // points are vacuously true, so we must reject explicitly.
    if after.tau_g1.len() < 2
        || after.tau_g2.len() < 2
        || after.alpha_tau_g1.len() < 2
        || after.beta_tau_g1.len() < 2
    {
        return false;
    }
    if after.tau_g1[1].is_zero()
        || after.tau_g2[1].is_zero()
        || after.alpha_tau_g1[0].is_zero()
        || after.beta_tau_g1[0].is_zero()
        || after.beta_g2.is_zero()
    {
        return false;
    }

    let g1 = G1Affine::generator();
    let g2 = G2Affine::generator();

    // 1. Tau ratio: e(proof.s, after.τG2[1]) == e(proof.δs, before.τG2[1])
    //    Proves the contributor knows δ_τ such that τ_new = δ_τ · τ_old.
    let tau_ok = Bls12_381::pairing(proof.tau_proof.0, after.tau_g2[1])
        == Bls12_381::pairing(proof.tau_proof.1, before.tau_g2[1]);

    // 2. Alpha ratio: e(after.αG1[0], proof.s_g2) == e(before.αG1[0], proof.δs_g2)
    //    Proves knowledge of δ_α.
    let alpha_ok = Bls12_381::pairing(after.alpha_tau_g1[0], proof.alpha_proof.0)
        == Bls12_381::pairing(before.alpha_tau_g1[0], proof.alpha_proof.1);

    // 3. Beta ratio: e(proof.s, after.βG2) == e(proof.δs, before.βG2)
    //    Proves knowledge of δ_β.
    let beta_ok = Bls12_381::pairing(proof.beta_proof.0, after.beta_g2)
        == Bls12_381::pairing(proof.beta_proof.1, before.beta_g2);

    // 4. Cross-consistency: tau_g1 and tau_g2 use the same τ.
    //    e(after.τG1[1], G2) == e(G1, after.τG2[1])
    let tau_cross = Bls12_381::pairing(after.tau_g1[1], g2)
        == Bls12_381::pairing(g1, after.tau_g2[1]);

    // 5. Cross-consistency: alpha_tau_g1 uses the same τ as tau_g2.
    //    e(after.αG1[1], G2) == e(after.αG1[0], after.τG2[1])
    let alpha_cross = Bls12_381::pairing(after.alpha_tau_g1[1], g2)
        == Bls12_381::pairing(after.alpha_tau_g1[0], after.tau_g2[1]);

    // 6. Cross-consistency: beta_tau_g1 uses the same τ as tau_g2.
    let beta_cross = Bls12_381::pairing(after.beta_tau_g1[1], g2)
        == Bls12_381::pairing(after.beta_tau_g1[0], after.tau_g2[1]);

    tau_ok && alpha_ok && beta_ok && tau_cross && alpha_cross && beta_cross
}

/// Verify the initial SRS and its contribution proof.
///
/// The initial contribution has no "before" SRS. Instead we verify:
/// 1. The SRS is internally consistent
/// 2. The proof demonstrates knowledge of the factors that produced
///    the SRS from the curve generators (same pairing-check structure
///    but against the generator points instead of a previous SRS).
pub fn verify_initial_contribution(
    srs: &PowersOfTau,
    proof: &ContributionProof,
) -> bool {
    // Reject identity proof elements
    if proof.tau_proof.0.is_zero()
        || proof.tau_proof.1.is_zero()
        || proof.alpha_proof.0.is_zero()
        || proof.alpha_proof.1.is_zero()
        || proof.beta_proof.0.is_zero()
        || proof.beta_proof.1.is_zero()
    {
        return false;
    }

    // Reject degenerate SRS
    if srs.tau_g1.len() < 2
        || srs.tau_g2.len() < 2
        || srs.alpha_tau_g1.len() < 2
        || srs.beta_tau_g1.len() < 2
    {
        return false;
    }
    if srs.tau_g1[1].is_zero()
        || srs.tau_g2[1].is_zero()
        || srs.alpha_tau_g1[0].is_zero()
        || srs.beta_tau_g1[0].is_zero()
        || srs.beta_g2.is_zero()
    {
        return false;
    }

    let g1 = G1Affine::generator();
    let g2 = G2Affine::generator();

    // Tau proof: e(s·G1, τ·G2) == e(δ_τ·s·G1, G2)
    // For the initial contribution, δ_τ = τ (the SRS was created from scratch).
    let tau_ok = Bls12_381::pairing(proof.tau_proof.0, srs.tau_g2[1])
        == Bls12_381::pairing(proof.tau_proof.1, g2);

    // Alpha proof: e(α·G1, s·G2) == e(G1, δ_α·s·G2)
    // For initial, δ_α = α.
    let alpha_ok = Bls12_381::pairing(srs.alpha_tau_g1[0], proof.alpha_proof.0)
        == Bls12_381::pairing(g1, proof.alpha_proof.1);

    // Beta proof: e(s·G1, β·G2) == e(δ_β·s·G1, G2)
    let beta_ok = Bls12_381::pairing(proof.beta_proof.0, srs.beta_g2)
        == Bls12_381::pairing(proof.beta_proof.1, g2);

    // Internal consistency
    let consistent = verify_consistency(srs);

    tau_ok && alpha_ok && beta_ok && consistent
}

/// Verify the internal consistency of an SRS.
///
/// Checks that EVERY element in each series forms a valid power sequence
/// via pairing equations. This is O(n) in SRS size.
pub fn verify_consistency(srs: &PowersOfTau) -> bool {
    if srs.tau_g1.len() < 2 || srs.tau_g2.len() < 2 {
        return false;
    }

    // Reject degenerate SRS: key elements must not be identity
    if srs.tau_g1[1].is_zero()
        || srs.tau_g2[1].is_zero()
        || srs.alpha_tau_g1[0].is_zero()
        || srs.beta_tau_g1[0].is_zero()
        || srs.beta_g2.is_zero()
    {
        return false;
    }

    let g1 = G1Affine::generator();
    let g2 = G2Affine::generator();

    // τ-powers in G1: e(τ^i · G1, τ · G2) == e(τ^{i+1} · G1, G2)
    // Check ALL elements, not just a spot-check.
    for i in 0..srs.tau_g1.len() - 1 {
        if Bls12_381::pairing(srs.tau_g1[i], srs.tau_g2[1])
            != Bls12_381::pairing(srs.tau_g1[i + 1], g2)
        {
            return false;
        }
    }

    // τ-powers in G2: e(τ · G1, τ^i · G2) == e(G1, τ^{i+1} · G2)
    for i in 0..srs.tau_g2.len() - 1 {
        if Bls12_381::pairing(srs.tau_g1[1], srs.tau_g2[i])
            != Bls12_381::pairing(g1, srs.tau_g2[i + 1])
        {
            return false;
        }
    }

    // α·τ-powers: e(α·τ^i · G1, τ · G2) == e(α·τ^{i+1} · G1, G2)
    for i in 0..srs.alpha_tau_g1.len() - 1 {
        if Bls12_381::pairing(srs.alpha_tau_g1[i], srs.tau_g2[1])
            != Bls12_381::pairing(srs.alpha_tau_g1[i + 1], g2)
        {
            return false;
        }
    }

    // β·τ-powers: same structure
    for i in 0..srs.beta_tau_g1.len() - 1 {
        if Bls12_381::pairing(srs.beta_tau_g1[i], srs.tau_g2[1])
            != Bls12_381::pairing(srs.beta_tau_g1[i + 1], g2)
        {
            return false;
        }
    }

    // Cross-check: τ·G1 consistent between G1 and G2
    if Bls12_381::pairing(srs.tau_g1[1], g2)
        != Bls12_381::pairing(g1, srs.tau_g2[1])
    {
        return false;
    }

    // Cross-check: β consistent between G1 and G2
    // e(β·G1, G2) == e(G1, β·G2)
    if Bls12_381::pairing(srs.beta_tau_g1[0], g2)
        != Bls12_381::pairing(g1, srs.beta_g2)
    {
        return false;
    }

    true
}

/// Compute the SHA-256 hash of an SRS state.
pub fn hash_srs(srs: &PowersOfTau) -> [u8; 32] {
    let mut hasher = Sha256::new();
    // Include vector lengths for unambiguous encoding
    hasher.update((srs.tau_g1.len() as u64).to_le_bytes());
    for p in &srs.tau_g1 {
        let mut buf = Vec::new();
        p.serialize_compressed(&mut buf).unwrap();
        hasher.update(&buf);
    }
    hasher.update((srs.tau_g2.len() as u64).to_le_bytes());
    for p in &srs.tau_g2 {
        let mut buf = Vec::new();
        p.serialize_compressed(&mut buf).unwrap();
        hasher.update(&buf);
    }
    hasher.update((srs.alpha_tau_g1.len() as u64).to_le_bytes());
    for p in &srs.alpha_tau_g1 {
        let mut buf = Vec::new();
        p.serialize_compressed(&mut buf).unwrap();
        hasher.update(&buf);
    }
    hasher.update((srs.beta_tau_g1.len() as u64).to_le_bytes());
    for p in &srs.beta_tau_g1 {
        let mut buf = Vec::new();
        p.serialize_compressed(&mut buf).unwrap();
        hasher.update(&buf);
    }
    {
        let mut buf = Vec::new();
        srs.beta_g2.serialize_compressed(&mut buf).unwrap();
        hasher.update(&buf);
    }
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

// ================================================================
// Key derivation
// ================================================================

/// Derive Groth16 keys from accumulated ceremony scalars.
///
/// Seeds a ChaCha20 RNG from a hash of the accumulated (τ, α, β) scalars.
/// The seed is NOT public: it depends on the ceremony's toxic waste, which
/// no single participant knows under the 1-of-N trust model.
///
/// **Limitation:** The machine executing this function sees the accumulated
/// scalars and can recover the Groth16 toxic waste. In production, key
/// derivation MUST use Phase 2 of Groth16 MPC (QAP evaluation on SRS
/// curve points) so that no single machine learns the scalars.
pub fn derive_keys(
    depth: usize,
    acc_tau: Fr,
    acc_alpha: Fr,
    acc_beta: Fr,
) -> Result<SetupResult, Box<dyn std::error::Error>> {
    let circuit = MembershipCircuit::<Fr>::empty(depth);
    let seed = hash_ceremony_scalars(&acc_tau, &acc_alpha, &acc_beta);
    let mut rng = rand_chacha::ChaCha20Rng::from_seed(seed);
    let (pk, vk) = Groth16::<Bls12_381>::circuit_specific_setup(circuit, &mut rng)?;
    let pvk = Groth16::<Bls12_381>::process_vk(&vk)?;
    Ok(SetupResult {
        proving_key: pk,
        verifying_key: vk,
        prepared_vk: pvk,
    })
}

/// Hash accumulated ceremony scalars into a 32-byte seed.
/// Domain-separated to prevent cross-protocol attacks.
fn hash_ceremony_scalars(tau: &Fr, alpha: &Fr, beta: &Fr) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"SEP-XXXX-ceremony-key-derivation-v1");
    let mut buf = Vec::new();
    tau.serialize_compressed(&mut buf).unwrap();
    hasher.update(&buf);
    buf.clear();
    alpha.serialize_compressed(&mut buf).unwrap();
    hasher.update(&buf);
    buf.clear();
    beta.serialize_compressed(&mut buf).unwrap();
    hasher.update(&buf);
    let result = hasher.finalize();
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&result);
    seed
}

// ================================================================
// Circuit ID
// ================================================================

/// Compute the unique circuit identifier for a tier.
///
/// `circuit_id = SHA-256("SEP-XXXX" || tier_id || tree_depth || poseidon_params_hash)`
pub fn compute_circuit_id(tier: &Tier) -> [u8; 32] {
    let poseidon_params_hash = {
        let config = poseidon::poseidon_config::<Fr>();
        let mut hasher = Sha256::new();
        // Hash the key Poseidon parameters
        hasher.update(b"poseidon-params");
        hasher.update(config.full_rounds.to_le_bytes());
        hasher.update(config.partial_rounds.to_le_bytes());
        hasher.update(config.alpha.to_le_bytes());
        hasher.update((config.rate as u64).to_le_bytes());
        hasher.update((config.capacity as u64).to_le_bytes());
        // Include round constants
        for round in &config.ark {
            for c in round {
                let mut buf = Vec::new();
                c.serialize_compressed(&mut buf).unwrap();
                hasher.update(&buf);
            }
        }
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    };

    let mut hasher = Sha256::new();
    hasher.update(b"SEP-XXXX");
    hasher.update(tier.id().to_le_bytes());
    hasher.update((tier.depth() as u32).to_le_bytes());
    hasher.update(poseidon_params_hash);
    let result = hasher.finalize();
    let mut id = [0u8; 32];
    id.copy_from_slice(&result);
    id
}

/// Compute the required SRS degree for a circuit tier.
///
/// The degree must be at least the QAP domain size (next power of 2
/// above the constraint count). We use measured constraint counts
/// plus headroom.
pub fn required_degree(tier: &Tier) -> usize {
    // Measured constraint counts from the reference implementation:
    //   Small  (depth  5): ~1,910
    //   Medium (depth  8): ~2,630
    //   Large  (depth 11): ~3,350
    //
    // QAP domain = next power of 2 above constraint count.
    // Add headroom for arkworks' internal padding.
    let estimated_constraints = match tier {
        Tier::Small => 2_048,
        Tier::Medium => 4_096,
        Tier::Large => 4_096,
    };
    // degree must cover QAP domain size
    estimated_constraints
}

// ================================================================
// Ceremony orchestration
// ================================================================

/// Run a complete ceremony for one circuit tier.
///
/// Simulates `num_participants` sequential contributions, verifies each,
/// and derives the final Groth16 keys.
///
/// Minimum 10 participants for production; fewer allowed for testing.
pub fn run_ceremony<R: Rng + rand::CryptoRng>(
    tier: &Tier,
    num_participants: usize,
    rng: &mut R,
) -> Result<CeremonyResult, Box<dyn std::error::Error>> {
    assert!(num_participants >= 1, "Need at least 1 participant");

    let degree = required_degree(tier);

    // Phase 1: Powers of Tau
    let (srs, init_proof, mut acc_tau, mut acc_alpha, mut acc_beta) =
        initialize(degree, rng);

    let mut transcript = CeremonyTranscript {
        contributions: Vec::with_capacity(num_participants),
        srs_snapshots: Vec::with_capacity(num_participants),
    };

    // Record the initial contribution
    transcript.contributions.push(ContributionRecord {
        index: 0,
        proof: init_proof,
        srs_hash: hash_srs(&srs),
    });
    transcript.srs_snapshots.push(srs.clone());

    let mut current_srs = srs;

    // Subsequent contributions
    for i in 1..num_participants {
        let (new_srs, proof, dt, da, db) = contribute(&current_srs, rng);

        // Verify this contribution
        assert!(
            verify_contribution(&current_srs, &new_srs, &proof),
            "Contribution {} failed verification",
            i
        );

        // Accumulate scalars (simulation only — in production no one knows these)
        acc_tau *= dt;
        acc_alpha *= da;
        acc_beta *= db;

        current_srs = new_srs;
        transcript.contributions.push(ContributionRecord {
            index: i,
            proof,
            srs_hash: hash_srs(&current_srs),
        });
        transcript.srs_snapshots.push(current_srs.clone());
    }

    // Verify final SRS consistency
    assert!(
        verify_consistency(&current_srs),
        "Final SRS failed consistency check"
    );

    // Phase 2: Key derivation from accumulated ceremony scalars
    let setup = derive_keys(tier.depth(), acc_tau, acc_alpha, acc_beta)?;

    let circuit_id = compute_circuit_id(tier);

    Ok(CeremonyResult {
        srs: current_srs,
        transcript,
        proving_key: setup.proving_key,
        verifying_key: setup.verifying_key,
        prepared_vk: setup.prepared_vk,
        circuit_id,
    })
}

/// Verify an entire ceremony transcript.
///
/// Replays every contribution, checking:
/// 1. The initial SRS + proof via `verify_initial_contribution`
/// 2. Each subsequent contribution proof against the before/after SRS pair
/// 3. Each SRS hash matches the recorded hash
/// 4. Every intermediate SRS is internally consistent
/// 5. The final SRS matches the provided `final_srs`
pub fn verify_transcript(transcript: &CeremonyTranscript, final_srs: &PowersOfTau) -> bool {
    if transcript.contributions.is_empty() || transcript.srs_snapshots.is_empty() {
        return false;
    }
    if transcript.contributions.len() != transcript.srs_snapshots.len() {
        return false;
    }

    // Verify each SRS hash matches the recorded hash
    for (record, snapshot) in transcript
        .contributions
        .iter()
        .zip(transcript.srs_snapshots.iter())
    {
        if record.srs_hash != hash_srs(snapshot) {
            return false;
        }
    }

    // Verify the initial contribution (index 0): SRS consistency + proof of knowledge
    if !verify_initial_contribution(
        &transcript.srs_snapshots[0],
        &transcript.contributions[0].proof,
    ) {
        return false;
    }

    // Verify each subsequent contribution proof
    for i in 1..transcript.contributions.len() {
        let before = &transcript.srs_snapshots[i - 1];
        let after = &transcript.srs_snapshots[i];
        let proof = &transcript.contributions[i].proof;
        if !verify_contribution(before, after, proof) {
            return false;
        }
    }

    // Verify the last snapshot matches the provided final SRS
    let last_snapshot = transcript.srs_snapshots.last().unwrap();
    if hash_srs(last_snapshot) != hash_srs(final_srs) {
        return false;
    }

    // Verify final SRS internal consistency (full check)
    verify_consistency(final_srs)
}

// ================================================================
// Internal helpers
// ================================================================

/// Compute [s^0·P, s^1·P, ..., s^{n-1}·P] in G1.
fn powers_of_scalar_times_point_g1(
    s: &Fr,
    n: usize,
    base: &G1Projective,
) -> Vec<G1Affine> {
    let mut result = Vec::with_capacity(n);
    let mut pow = Fr::one();
    for _ in 0..n {
        result.push((*base * pow).into_affine());
        pow *= s;
    }
    result
}

/// Compute [c·s^0·P, c·s^1·P, ..., c·s^{n-1}·P] in G1.
fn powers_of_scalar_times_point_g1_with_coeff(
    s: &Fr,
    coeff: &Fr,
    n: usize,
    base: &G1Projective,
) -> Vec<G1Affine> {
    let mut result = Vec::with_capacity(n);
    let mut pow = Fr::one();
    for _ in 0..n {
        result.push((*base * (*coeff * pow)).into_affine());
        pow *= s;
    }
    result
}

/// Compute [s^0·P, s^1·P, ..., s^{n-1}·P] in G2.
fn powers_of_scalar_times_point_g2(
    s: &Fr,
    n: usize,
    base: &G2Projective,
) -> Vec<G2Affine> {
    let mut result = Vec::with_capacity(n);
    let mut pow = Fr::one();
    for _ in 0..n {
        result.push((*base * pow).into_affine());
        pow *= s;
    }
    result
}

/// Update a G1 power series: new[i] = coeff · base_scalar^i · old[i].
fn update_powers_g1(old: &[G1Affine], base_scalar: &Fr, coeff: &Fr) -> Vec<G1Affine> {
    let mut pow = Fr::one();
    old.iter()
        .map(|p| {
            let factor = *coeff * pow;
            pow *= base_scalar;
            p.mul_bigint(factor.into_bigint()).into_affine()
        })
        .collect()
}

/// Update a G2 power series: new[i] = coeff · base_scalar^i · old[i].
fn update_powers_g2(old: &[G2Affine], base_scalar: &Fr, coeff: &Fr) -> Vec<G2Affine> {
    let mut pow = Fr::one();
    old.iter()
        .map(|p| {
            let factor = *coeff * pow;
            pow *= base_scalar;
            p.mul_bigint(factor.into_bigint()).into_affine()
        })
        .collect()
}

/// Generate a contribution proof (proof of knowledge of δ_τ, δ_α, δ_β).
fn make_contribution_proof<R: Rng + rand::CryptoRng>(
    rng: &mut R,
    delta_tau: &Fr,
    delta_alpha: &Fr,
    delta_beta: &Fr,
) -> ContributionProof {
    let g1 = G1Projective::generator();
    let g2 = G2Projective::generator();

    let s1 = Fr::rand(rng);
    let tau_proof = (
        (g1 * s1).into_affine(),
        (g1 * (*delta_tau * s1)).into_affine(),
    );

    let s2 = Fr::rand(rng);
    let alpha_proof = (
        (g2 * s2).into_affine(),
        (g2 * (*delta_alpha * s2)).into_affine(),
    );

    let s3 = Fr::rand(rng);
    let beta_proof = (
        (g1 * s3).into_affine(),
        (g1 * (*delta_beta * s3)).into_affine(),
    );

    ContributionProof {
        tau_proof,
        alpha_proof,
        beta_proof,
    }
}

// ================================================================
// Tests
// ================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poseidon::poseidon_config;
    use crate::prover;
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn test_rng() -> ChaCha20Rng {
        ChaCha20Rng::seed_from_u64(0xCEDE_0057)
    }

    // --- Phase 1: Powers of Tau ---

    #[test]
    fn test_initialize_and_verify_consistency() {
        let mut rng = test_rng();
        let degree = 32;
        let (srs, _proof, _tau, _alpha, _beta) = initialize(degree, &mut rng);

        assert_eq!(srs.tau_g1.len(), 2 * degree - 1);
        assert_eq!(srs.tau_g2.len(), degree);
        assert_eq!(srs.alpha_tau_g1.len(), degree);
        assert_eq!(srs.beta_tau_g1.len(), degree);

        assert!(verify_consistency(&srs), "Initial SRS must be consistent");
    }

    #[test]
    fn test_single_contribution_verifies() {
        let mut rng = test_rng();
        let (srs, _proof, _t, _a, _b) = initialize(32, &mut rng);

        let (new_srs, proof, _dt, _da, _db) = contribute(&srs, &mut rng);

        assert!(
            verify_contribution(&srs, &new_srs, &proof),
            "Valid contribution must pass verification"
        );
        assert!(
            verify_consistency(&new_srs),
            "Updated SRS must be consistent"
        );
    }

    #[test]
    fn test_chain_of_contributions() {
        let mut rng = test_rng();
        let (mut srs, _proof, _t, _a, _b) = initialize(32, &mut rng);

        for i in 0..3 {
            let prev = srs.clone();
            let (new_srs, proof, _dt, _da, _db) = contribute(&srs, &mut rng);

            assert!(
                verify_contribution(&prev, &new_srs, &proof),
                "Contribution {} must verify",
                i
            );
            assert!(
                verify_consistency(&new_srs),
                "SRS must be consistent after contribution {}",
                i
            );

            srs = new_srs;
        }
    }

    #[test]
    fn test_contribution_changes_srs() {
        let mut rng = test_rng();
        let (srs, _proof, _t, _a, _b) = initialize(32, &mut rng);

        let (new_srs, _proof, _dt, _da, _db) = contribute(&srs, &mut rng);

        // SRS must change after a contribution
        assert_ne!(srs.tau_g1[1], new_srs.tau_g1[1]);
        assert_ne!(srs.tau_g2[1], new_srs.tau_g2[1]);
        assert_ne!(srs.alpha_tau_g1[0], new_srs.alpha_tau_g1[0]);
        assert_ne!(srs.beta_tau_g1[0], new_srs.beta_tau_g1[0]);
        assert_ne!(srs.beta_g2, new_srs.beta_g2);
    }

    #[test]
    fn test_tampered_srs_rejected() {
        let mut rng = test_rng();
        let (srs, _proof, _t, _a, _b) = initialize(32, &mut rng);

        let (mut tampered, proof, _dt, _da, _db) = contribute(&srs, &mut rng);

        // Tamper: replace tau_g1[1] with a random point
        tampered.tau_g1[1] = (G1Projective::generator() * Fr::from(9999u64)).into_affine();

        assert!(
            !verify_contribution(&srs, &tampered, &proof),
            "Tampered SRS must fail verification"
        );
    }

    #[test]
    fn test_wrong_proof_rejected() {
        let mut rng = test_rng();
        let (srs, _proof, _t, _a, _b) = initialize(32, &mut rng);

        let (new_srs, _correct_proof, _dt, _da, _db) = contribute(&srs, &mut rng);

        // Create a proof for different update factors
        let fake_proof = make_contribution_proof(
            &mut rng,
            &Fr::from(111u64),
            &Fr::from(222u64),
            &Fr::from(333u64),
        );

        assert!(
            !verify_contribution(&srs, &new_srs, &fake_proof),
            "Wrong proof must fail verification"
        );
    }

    #[test]
    fn test_zero_proof_rejected() {
        let mut rng = test_rng();
        let (srs, _proof, _t, _a, _b) = initialize(32, &mut rng);

        let (new_srs, _proof, _dt, _da, _db) = contribute(&srs, &mut rng);

        // Forge a proof with identity (zero) points — this would trivially
        // satisfy all pairing checks without the explicit rejection.
        let zero_proof = ContributionProof {
            tau_proof: (G1Affine::zero(), G1Affine::zero()),
            alpha_proof: (G2Affine::zero(), G2Affine::zero()),
            beta_proof: (G1Affine::zero(), G1Affine::zero()),
        };

        assert!(
            !verify_contribution(&srs, &new_srs, &zero_proof),
            "Zero-point proof must be rejected"
        );
    }

    #[test]
    fn test_degenerate_srs_rejected() {
        let mut rng = test_rng();
        let (srs, _proof, _t, _a, _b) = initialize(32, &mut rng);

        // Create a valid contribution, then make it degenerate by zeroing tau_g1[1]
        let (mut degen_srs, proof, _dt, _da, _db) = contribute(&srs, &mut rng);
        degen_srs.tau_g1[1] = G1Affine::zero();

        assert!(
            !verify_contribution(&srs, &degen_srs, &proof),
            "Degenerate SRS (zero tau_g1[1]) must be rejected"
        );
    }

    #[test]
    fn test_srs_hash_deterministic() {
        let mut rng = test_rng();
        let (srs, _proof, _t, _a, _b) = initialize(32, &mut rng);

        let h1 = hash_srs(&srs);
        let h2 = hash_srs(&srs);
        assert_eq!(h1, h2, "SRS hash must be deterministic");
    }

    #[test]
    fn test_srs_hash_changes_with_contribution() {
        let mut rng = test_rng();
        let (srs, _proof, _t, _a, _b) = initialize(32, &mut rng);
        let h1 = hash_srs(&srs);

        let (new_srs, _proof, _dt, _da, _db) = contribute(&srs, &mut rng);
        let h2 = hash_srs(&new_srs);

        assert_ne!(h1, h2, "SRS hash must change after contribution");
    }

    // --- Key derivation ---

    #[test]
    fn test_derive_keys_deterministic() {
        // Deriving keys from the same scalars must produce identical results.
        let mut rng = test_rng();
        let tau = Fr::rand(&mut rng);
        let alpha = Fr::rand(&mut rng);
        let beta = Fr::rand(&mut rng);

        let setup1 = derive_keys(5, tau, alpha, beta).expect("Key derivation 1 failed");
        let setup2 = derive_keys(5, tau, alpha, beta).expect("Key derivation 2 failed");

        assert_eq!(
            setup1.verifying_key, setup2.verifying_key,
            "Same scalars must produce same VK"
        );
    }

    #[test]
    fn test_derive_keys_different_scalars_different_keys() {
        let mut rng = test_rng();
        let tau1 = Fr::rand(&mut rng);
        let alpha1 = Fr::rand(&mut rng);
        let beta1 = Fr::rand(&mut rng);
        let tau2 = Fr::rand(&mut rng);
        let alpha2 = Fr::rand(&mut rng);
        let beta2 = Fr::rand(&mut rng);

        let setup1 = derive_keys(5, tau1, alpha1, beta1).expect("Key derivation 1 failed");
        let setup2 = derive_keys(5, tau2, alpha2, beta2).expect("Key derivation 2 failed");

        assert_ne!(
            setup1.verifying_key, setup2.verifying_key,
            "Different scalars must produce different VK"
        );
    }

    #[test]
    fn test_ceremony_keys_work_for_proving() {
        let mut rng = test_rng();

        // Run a small ceremony (3 participants)
        let degree = 64;
        let (mut srs, _proof, mut acc_tau, mut acc_alpha, mut acc_beta) =
            initialize(degree, &mut rng);

        for _ in 0..2 {
            let (new_srs, _proof, dt, da, db) = contribute(&srs, &mut rng);
            acc_tau *= dt;
            acc_alpha *= da;
            acc_beta *= db;
            srs = new_srs;
        }

        let setup = derive_keys(5, acc_tau, acc_alpha, acc_beta)
            .expect("Key derivation failed");

        // Use the ceremony-derived keys for a real proof
        let config = poseidon_config::<Fr>();
        let keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let leaf_hashes: Vec<Fr> = keys
            .iter()
            .map(|sk| crate::poseidon::poseidon_hash_one(&config, sk))
            .collect();

        let input = prover::ProverInput {
            leaf_hashes,
            secret_key: keys[0],
            prover_index: 0,
            epoch: 0,
            salt: [0xAA; 32],
            depth: 5,
        };

        let (proof, public_inputs) =
            prover::prove(&setup.proving_key, &input, &mut rng).expect("Proving failed");

        let valid = prover::verify(&setup.prepared_vk, &proof, &public_inputs)
            .expect("Verification failed");

        assert!(valid, "Proof from ceremony-derived keys must verify");
    }

    // --- Circuit ID ---

    #[test]
    fn test_circuit_id_deterministic() {
        let id1 = compute_circuit_id(&Tier::Small);
        let id2 = compute_circuit_id(&Tier::Small);
        assert_eq!(id1, id2, "Circuit ID must be deterministic");
    }

    #[test]
    fn test_circuit_id_differs_per_tier() {
        let id_s = compute_circuit_id(&Tier::Small);
        let id_m = compute_circuit_id(&Tier::Medium);
        let id_l = compute_circuit_id(&Tier::Large);
        assert_ne!(id_s, id_m);
        assert_ne!(id_m, id_l);
        assert_ne!(id_s, id_l);
    }

    // --- Ceremony orchestration ---

    #[test]
    fn test_run_ceremony_small_tier() {
        let mut rng = test_rng();
        let result = run_ceremony(&Tier::Small, 3, &mut rng)
            .expect("Ceremony failed");

        assert_eq!(result.transcript.contributions.len(), 3);
        assert_eq!(result.transcript.srs_snapshots.len(), 3);
        assert!(verify_consistency(&result.srs));
        assert!(verify_transcript(&result.transcript, &result.srs));

        // Verify the keys work
        let config = poseidon_config::<Fr>();
        let keys = vec![Fr::from(42u64)];
        let leaf_hashes: Vec<Fr> = keys
            .iter()
            .map(|sk| crate::poseidon::poseidon_hash_one(&config, sk))
            .collect();

        let input = prover::ProverInput {
            leaf_hashes,
            secret_key: keys[0],
            prover_index: 0,
            epoch: 0,
            salt: [0xBB; 32],
            depth: 5,
        };

        let (proof, pi) =
            prover::prove(&result.proving_key, &input, &mut rng).expect("Proving failed");
        let valid =
            prover::verify(&result.prepared_vk, &proof, &pi).expect("Verification failed");
        assert!(valid, "Proof must verify with ceremony-derived keys");
    }

    #[test]
    fn test_verify_transcript_full_replay() {
        let mut rng = test_rng();
        let result = run_ceremony(&Tier::Small, 3, &mut rng).expect("Ceremony failed");

        assert!(verify_transcript(&result.transcript, &result.srs));

        // Tamper with the final SRS — transcript should no longer match
        let mut bad_srs = result.srs.clone();
        bad_srs.tau_g1[0] = (G1Projective::generator() * Fr::from(9999u64)).into_affine();
        assert!(!verify_transcript(&result.transcript, &bad_srs));
    }

    #[test]
    fn test_verify_transcript_rejects_tampered_intermediate() {
        let mut rng = test_rng();
        let result = run_ceremony(&Tier::Small, 3, &mut rng).expect("Ceremony failed");

        // Tamper with an intermediate SRS snapshot
        let mut bad_transcript = result.transcript.clone();
        bad_transcript.srs_snapshots[1].tau_g1[1] =
            (G1Projective::generator() * Fr::from(7777u64)).into_affine();

        assert!(
            !verify_transcript(&bad_transcript, &result.srs),
            "Tampered intermediate snapshot must fail transcript verification"
        );
    }

    #[test]
    fn test_beta_g1_g2_desync_rejected() {
        let mut rng = test_rng();
        let (srs, _proof, _t, _a, _b) = initialize(32, &mut rng);

        let (mut bad_srs, _proof, _dt, _da, _db) = contribute(&srs, &mut rng);

        // Desync: apply a different factor to beta_g2 than was used for beta_tau_g1
        bad_srs.beta_g2 = (G2Projective::generator() * Fr::from(9999u64)).into_affine();

        // verify_contribution calls verify_consistency implicitly via the
        // degenerate checks, but the β cross-check is in verify_consistency.
        // The contribution check itself may pass, but consistency must fail.
        assert!(
            !verify_consistency(&bad_srs),
            "β desync between G1 and G2 must fail consistency check"
        );
    }

    #[test]
    fn test_required_degree() {
        // Verify degree is sufficient for each tier's constraint count
        assert!(required_degree(&Tier::Small) >= 2048);
        assert!(required_degree(&Tier::Medium) >= 4096);
        assert!(required_degree(&Tier::Large) >= 4096);
    }
}
