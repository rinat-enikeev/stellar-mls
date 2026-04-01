//! Phase 2 MPC adapter for Groth16 trusted setup.
//!
//! Phase 1 (Powers of Tau) produces a universal SRS that is circuit-agnostic.
//! Phase 2 MPC takes that SRS and derives circuit-specific proving/verifying keys
//! without any single party holding the toxic waste scalars.
//!
//! This module provides serialization of the Phase 1 SRS into `.ptau`-compatible
//! format for use with external ceremony tools (e.g., snarkjs), and import of
//! Phase 2 outputs back into arkworks key types.
//!
//! # Workflow
//!
//! 1. Run Phase 1 via `ceremony::run_ceremony()`.
//! 2. Export SRS with `export_srs()`.
//! 3. Run Phase 2 MPC using snarkjs or equivalent (external to this library).
//! 4. Import the resulting keys with `import_phase2_keys()`.

use ark_bls12_381::{Bls12_381, G1Affine, G2Affine};
use ark_groth16::{ProvingKey, VerifyingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};

use super::PowersOfTau;

/// Serialized Phase 1 SRS for export to external Phase 2 MPC tools.
#[derive(Clone, Debug)]
pub struct ExportedSRS {
    /// Serialized SRS bytes in arkworks canonical format.
    pub srs_bytes: Vec<u8>,
    /// Number of G1 powers (2·degree - 1).
    pub tau_g1_count: usize,
    /// Number of G2 powers (degree + 1).
    pub tau_g2_count: usize,
}

/// Phase 2 output: circuit-specific keys imported from an external ceremony.
#[derive(Clone)]
pub struct Phase2Keys {
    pub proving_key: ProvingKey<Bls12_381>,
    pub verifying_key: VerifyingKey<Bls12_381>,
}

/// Export Phase 1 SRS to a serialized format suitable for external Phase 2 tools.
///
/// The output is in arkworks canonical serialization. To convert to `.ptau` format
/// for snarkjs, use the companion script `scripts/srs_to_ptau.py` (see
/// `docs/phase2-mpc-integration.md`).
pub fn export_srs(srs: &PowersOfTau) -> Result<ExportedSRS, String> {
    let mut buf = Vec::new();

    // Write tau_g1 count + elements
    let tau_g1_count = srs.tau_g1.len();
    (tau_g1_count as u64)
        .serialize_compressed(&mut buf)
        .map_err(|e| format!("failed to serialize tau_g1 count: {e}"))?;
    for pt in &srs.tau_g1 {
        pt.serialize_compressed(&mut buf)
            .map_err(|e| format!("failed to serialize tau_g1 element: {e}"))?;
    }

    // Write tau_g2 count + elements
    let tau_g2_count = srs.tau_g2.len();
    (tau_g2_count as u64)
        .serialize_compressed(&mut buf)
        .map_err(|e| format!("failed to serialize tau_g2 count: {e}"))?;
    for pt in &srs.tau_g2 {
        pt.serialize_compressed(&mut buf)
            .map_err(|e| format!("failed to serialize tau_g2 element: {e}"))?;
    }

    // Write alpha_tau_g1 count + elements
    let alpha_count = srs.alpha_tau_g1.len();
    (alpha_count as u64)
        .serialize_compressed(&mut buf)
        .map_err(|e| format!("failed to serialize alpha_tau_g1 count: {e}"))?;
    for pt in &srs.alpha_tau_g1 {
        pt.serialize_compressed(&mut buf)
            .map_err(|e| format!("failed to serialize alpha_tau_g1 element: {e}"))?;
    }

    // Write beta_tau_g1 count + elements
    let beta_count = srs.beta_tau_g1.len();
    (beta_count as u64)
        .serialize_compressed(&mut buf)
        .map_err(|e| format!("failed to serialize beta_tau_g1 count: {e}"))?;
    for pt in &srs.beta_tau_g1 {
        pt.serialize_compressed(&mut buf)
            .map_err(|e| format!("failed to serialize beta_tau_g1 element: {e}"))?;
    }

    // Write beta_g2
    srs.beta_g2
        .serialize_compressed(&mut buf)
        .map_err(|e| format!("failed to serialize beta_g2: {e}"))?;

    Ok(ExportedSRS {
        srs_bytes: buf,
        tau_g1_count,
        tau_g2_count,
    })
}

/// Deserialize a Phase 1 SRS from bytes produced by `export_srs`.
pub fn import_srs(bytes: &[u8]) -> Result<PowersOfTau, String> {
    let mut cursor = &bytes[..];

    let tau_g1_count = u64::deserialize_compressed(&mut cursor)
        .map_err(|e| format!("failed to read tau_g1 count: {e}"))? as usize;
    let mut tau_g1 = Vec::with_capacity(tau_g1_count);
    for i in 0..tau_g1_count {
        tau_g1.push(
            G1Affine::deserialize_compressed(&mut cursor)
                .map_err(|e| format!("failed to read tau_g1[{i}]: {e}"))?,
        );
    }

    let tau_g2_count = u64::deserialize_compressed(&mut cursor)
        .map_err(|e| format!("failed to read tau_g2 count: {e}"))? as usize;
    let mut tau_g2 = Vec::with_capacity(tau_g2_count);
    for i in 0..tau_g2_count {
        tau_g2.push(
            G2Affine::deserialize_compressed(&mut cursor)
                .map_err(|e| format!("failed to read tau_g2[{i}]: {e}"))?,
        );
    }

    let alpha_count = u64::deserialize_compressed(&mut cursor)
        .map_err(|e| format!("failed to read alpha_tau_g1 count: {e}"))? as usize;
    let mut alpha_tau_g1 = Vec::with_capacity(alpha_count);
    for i in 0..alpha_count {
        alpha_tau_g1.push(
            G1Affine::deserialize_compressed(&mut cursor)
                .map_err(|e| format!("failed to read alpha_tau_g1[{i}]: {e}"))?,
        );
    }

    let beta_count = u64::deserialize_compressed(&mut cursor)
        .map_err(|e| format!("failed to read beta_tau_g1 count: {e}"))? as usize;
    let mut beta_tau_g1 = Vec::with_capacity(beta_count);
    for i in 0..beta_count {
        beta_tau_g1.push(
            G1Affine::deserialize_compressed(&mut cursor)
                .map_err(|e| format!("failed to read beta_tau_g1[{i}]: {e}"))?,
        );
    }

    let beta_g2 = G2Affine::deserialize_compressed(&mut cursor)
        .map_err(|e| format!("failed to read beta_g2: {e}"))?;

    Ok(PowersOfTau {
        tau_g1,
        tau_g2,
        alpha_tau_g1,
        beta_tau_g1,
        beta_g2,
    })
}

/// Import Phase 2 proving and verifying keys from arkworks-serialized bytes.
///
/// After running Phase 2 MPC externally, serialize the resulting keys in
/// arkworks canonical compressed format and pass them here.
pub fn import_phase2_keys(
    proving_key_bytes: &[u8],
    verifying_key_bytes: &[u8],
) -> Result<Phase2Keys, String> {
    let proving_key = ProvingKey::<Bls12_381>::deserialize_compressed(proving_key_bytes)
        .map_err(|e| format!("failed to deserialize proving key: {e}"))?;
    let verifying_key = VerifyingKey::<Bls12_381>::deserialize_compressed(verifying_key_bytes)
        .map_err(|e| format!("failed to deserialize verifying key: {e}"))?;
    Ok(Phase2Keys {
        proving_key,
        verifying_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ceremony;
    use rand::SeedableRng;

    #[test]
    fn test_srs_export_import_roundtrip() {
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(42);
        let degree = 32; // Small tier
        let (srs, _proof, _tau, _alpha, _beta) = ceremony::initialize(degree, &mut rng);

        let exported = export_srs(&srs).expect("export should succeed");
        assert!(!exported.srs_bytes.is_empty());
        assert_eq!(exported.tau_g1_count, srs.tau_g1.len());
        assert_eq!(exported.tau_g2_count, srs.tau_g2.len());

        let imported = import_srs(&exported.srs_bytes).expect("import should succeed");
        assert_eq!(imported.tau_g1, srs.tau_g1);
        assert_eq!(imported.tau_g2, srs.tau_g2);
        assert_eq!(imported.alpha_tau_g1, srs.alpha_tau_g1);
        assert_eq!(imported.beta_tau_g1, srs.beta_tau_g1);
        assert_eq!(imported.beta_g2, srs.beta_g2);
    }

    #[test]
    fn test_srs_export_after_contribution() {
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(99);
        let degree = 32;
        let (srs, _proof, _tau, _alpha, _beta) = ceremony::initialize(degree, &mut rng);
        let (srs, _proof, _dt, _da, _db) = ceremony::contribute(&srs, &mut rng);

        let exported = export_srs(&srs).expect("export should succeed");
        let imported = import_srs(&exported.srs_bytes).expect("import should succeed");
        assert_eq!(imported.tau_g1, srs.tau_g1);
        assert_eq!(imported.beta_g2, srs.beta_g2);
    }
}
