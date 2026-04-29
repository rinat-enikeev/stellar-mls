//! Groth16 prover and verifier pipeline for SEP-XXXX.
//!
//! This module provides the end-to-end workflow:
//! 1. Trusted setup (generates proving key + verification key)
//! 2. Proof generation (member creates a 192-byte proof)
//! 3. Proof verification (contract/anyone verifies with 3 pairings)

#[cfg(feature = "plonk")]
pub mod srs;

use ark_bls12_381::{Bls12_381, Fr};
use ark_groth16::{Groth16, PreparedVerifyingKey, Proof, ProvingKey, VerifyingKey};
use ark_snark::SNARK;
use ark_std::rand::Rng;

use crate::circuit::MembershipCircuit;
use crate::circuit::update::UpdateCircuit;
use crate::commitment::{
    bytes_be_to_field_checked, field_to_bytes_be, Salt, compute_poseidon_commitment,
};
use crate::merkle::{CanonicalMember, PoseidonMerkleTree, canonicalize_members, compressed_public_key_bytes};
use crate::poseidon::poseidon_config;

/// Result of the trusted setup ceremony.
pub struct SetupResult {
    pub proving_key: ProvingKey<Bls12_381>,
    pub verifying_key: VerifyingKey<Bls12_381>,
    pub prepared_vk: PreparedVerifyingKey<Bls12_381>,
}

/// Input needed to generate a membership proof.
pub struct ProverInput {
    /// Member records. Each member contributes:
    /// - `public_key_bytes`: compressed G1 bytes used for canonical sorting
    /// - `leaf_hash`: `Poseidon(sk_i)`
    ///
    /// The prover canonicalizes this roster internally, making commitment
    /// construction deterministic regardless of input ordering.
    pub members: Vec<CanonicalMember<Fr>>,
    /// The prover's secret key (BLS12-381 private key scalar).
    /// Only the prover knows this value.
    pub secret_key: Fr,
    /// Current epoch.
    pub epoch: u64,
    /// Current salt (shared secret among group members).
    pub salt: Salt,
    /// Tree depth (must match the setup circuit).
    pub depth: usize,
}

/// Public inputs for verification (2 field elements).
#[derive(Debug)]
pub struct PublicInputs {
    /// Poseidon commitment: `Poseidon(Poseidon(root, epoch), salt)`
    pub commitment: Fr,
    /// Epoch as a field element.
    pub epoch: Fr,
}

/// Run the trusted setup for a given circuit depth.
///
/// In production, this would be an MPC ceremony.
/// Here it uses a single random source (suitable for testing).
pub fn setup<R: Rng + rand::CryptoRng>(depth: usize, rng: &mut R) -> Result<SetupResult, Box<dyn std::error::Error>> {
    let empty_circuit = MembershipCircuit::<Fr>::empty(depth);
    let (pk, vk) = Groth16::<Bls12_381>::circuit_specific_setup(empty_circuit, rng)?;
    let pvk = Groth16::<Bls12_381>::process_vk(&vk)?;

    Ok(SetupResult {
        proving_key: pk,
        verifying_key: vk,
        prepared_vk: pvk,
    })
}

/// Generate a membership proof.
pub fn prove<R: Rng + rand::CryptoRng>(
    pk: &ProvingKey<Bls12_381>,
    input: &ProverInput,
    rng: &mut R,
) -> Result<(Proof<Bls12_381>, PublicInputs), Box<dyn std::error::Error>> {
    let config = poseidon_config::<Fr>();
    let ordered_members = canonicalize_members(&input.members)?;
    let prover_public_key_bytes = compressed_public_key_bytes(&input.secret_key);
    let prover_leaf_hash = compute_leaf_hash(&input.secret_key);
    let prover_index = ordered_members
        .iter()
        .position(|member| member.public_key_bytes == prover_public_key_bytes)
        .ok_or("prover public key is not present in the member roster")?;

    if ordered_members[prover_index].leaf_hash != prover_leaf_hash {
        return Err("member roster leaf hash does not match prover secret key".into());
    }

    // Build Merkle tree from canonicalized member records.
    let tree = PoseidonMerkleTree::build_from_members(&config, &ordered_members, input.depth)?;
    let root = tree.root();
    let merkle_proof = tree.prove(prover_index);

    // Compute the Poseidon commitment binding
    let commitment = compute_poseidon_commitment(&config, &root, input.epoch, &input.salt);

    let circuit = MembershipCircuit::new(
        commitment,
        input.epoch,
        input.secret_key,
        root,
        input.salt,
        merkle_proof.path,
        merkle_proof.leaf_index,
        input.depth,
    );

    let proof = Groth16::<Bls12_381>::prove(pk, circuit, rng)?;

    let public_inputs = PublicInputs {
        commitment,
        epoch: Fr::from(input.epoch),
    };

    Ok((proof, public_inputs))
}

/// Verify a membership proof.
pub fn verify(
    pvk: &PreparedVerifyingKey<Bls12_381>,
    proof: &Proof<Bls12_381>,
    public_inputs: &PublicInputs,
) -> Result<bool, Box<dyn std::error::Error>> {
    let inputs = vec![
        public_inputs.commitment,
        public_inputs.epoch,
    ];

    let result = Groth16::<Bls12_381>::verify_with_processed_vk(pvk, &inputs, proof)?;
    Ok(result)
}

// ============================================================
// UpdateCircuit prover — binds (C_old, epoch_old, C_new) into the proof.
// See docs/update-circuit-binding-design.md §5.
// ============================================================

/// Input needed to generate an update-transition proof.
///
/// Canonicalisation matches `ProverInput`: members are supplied as
/// `CanonicalMember` records (public-key bytes + precomputed leaf hash);
/// the prover internally sorts them by compressed G1 bytes so the
/// resulting commitment is deterministic regardless of input ordering.
pub struct UpdateProverInput {
    /// The old tree's member roster.
    pub members_old: Vec<CanonicalMember<Fr>>,
    /// The new tree's member roster.
    pub members_new: Vec<CanonicalMember<Fr>>,
    /// The prover's secret key (must hash to a leaf in `members_old`).
    pub secret_key: Fr,
    /// The epoch of the old state. The new epoch is derived as
    /// `epoch_old + 1` both in-circuit and in the returned public inputs.
    pub epoch_old: u64,
    /// The old state's salt.
    pub salt_old: Salt,
    /// The new state's salt.
    pub salt_new: Salt,
    /// Tree depth (must match the setup circuit and both rosters' tier).
    pub depth: usize,
}

/// Public inputs for an update proof (3 field elements).
///
/// Allocation order — and therefore Groth16 IC-vector indexing order —
/// is `(c_old, epoch_old, c_new)`. Do not reorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdatePublicInputs {
    /// Old commitment: `Poseidon(Poseidon(root_old, epoch_old), salt_old)`.
    pub c_old: Fr,
    /// Epoch of the old state, as a field element.
    pub epoch_old: Fr,
    /// New commitment: `Poseidon(Poseidon(root_new, epoch_old + 1), salt_new)`.
    pub c_new: Fr,
}

impl UpdatePublicInputs {
    /// Wire-format version byte. Distinguishes the 3-scalar update-proof
    /// public inputs from the 2-scalar membership-proof public inputs.
    pub const VERSION: u8 = 2;

    /// Serialised length: 1 version byte + 32 C_old + 8 epoch_old + 32 C_new.
    pub const SERIALIZED_LEN: usize = 1 + 32 + 8 + 32;

    /// Serialise to the 73-byte wire format:
    /// `version || c_old_be || epoch_old_be || c_new_be`.
    pub fn serialize(&self) -> [u8; Self::SERIALIZED_LEN] {
        let mut out = [0u8; Self::SERIALIZED_LEN];
        out[0] = Self::VERSION;
        out[1..33].copy_from_slice(&field_to_bytes_be(&self.c_old));

        // Recover u64 from the Fr by reading the low 8 bytes of the LE
        // representation. This is lossy in general but safe here because
        // the field element was constructed from a u64 by the prover.
        let epoch_u64 = fr_to_u64(&self.epoch_old);
        out[33..41].copy_from_slice(&epoch_u64.to_be_bytes());

        out[41..73].copy_from_slice(&field_to_bytes_be(&self.c_new));
        out
    }

    /// Deserialise from the 73-byte wire format. Rejects length mismatch
    /// or version mismatch.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() != Self::SERIALIZED_LEN {
            return Err(format!(
                "UpdatePublicInputs length mismatch: got {}, expected {}",
                bytes.len(),
                Self::SERIALIZED_LEN
            ));
        }
        if bytes[0] != Self::VERSION {
            return Err(format!(
                "UpdatePublicInputs version mismatch: got {:#04x}, expected {:#04x}",
                bytes[0],
                Self::VERSION
            ));
        }

        let mut c_old_be = [0u8; 32];
        c_old_be.copy_from_slice(&bytes[1..33]);
        let c_old = bytes_be_to_field_checked::<Fr>(&c_old_be)
            .map_err(|e| format!("c_old: {}", e))?;

        let mut epoch_be = [0u8; 8];
        epoch_be.copy_from_slice(&bytes[33..41]);
        let epoch_old = Fr::from(u64::from_be_bytes(epoch_be));

        let mut c_new_be = [0u8; 32];
        c_new_be.copy_from_slice(&bytes[41..73]);
        let c_new = bytes_be_to_field_checked::<Fr>(&c_new_be)
            .map_err(|e| format!("c_new: {}", e))?;

        Ok(Self { c_old, epoch_old, c_new })
    }

    /// Return the three scalars in pinned allocation order for Groth16 verify.
    pub fn to_scalars(&self) -> [Fr; 3] {
        [self.c_old, self.epoch_old, self.c_new]
    }
}

/// Reconstruct a `u64` from an `Fr` that was built via `Fr::from(u: u64)`.
fn fr_to_u64(f: &Fr) -> u64 {
    use ark_ff::{BigInteger, PrimeField};
    let repr = f.into_bigint();
    let le = repr.to_bytes_le();
    let mut out = [0u8; 8];
    let n = le.len().min(8);
    out[..n].copy_from_slice(&le[..n]);
    u64::from_le_bytes(out)
}

/// Run the trusted setup for the update circuit at a given depth.
pub fn setup_update<R: Rng + rand::CryptoRng>(
    depth: usize,
    rng: &mut R,
) -> Result<SetupResult, Box<dyn std::error::Error>> {
    let empty_circuit = UpdateCircuit::<Fr>::empty(depth);
    let (pk, vk) = Groth16::<Bls12_381>::circuit_specific_setup(empty_circuit, rng)?;
    let pvk = Groth16::<Bls12_381>::process_vk(&vk)?;
    Ok(SetupResult {
        proving_key: pk,
        verifying_key: vk,
        prepared_vk: pvk,
    })
}

/// Generate an update-transition proof.
///
/// Workflow:
/// 1. Canonicalise both rosters and build both Poseidon Merkle trees.
/// 2. Locate the prover's canonical index in the old tree; build its
///    authentication path.
/// 3. Derive `C_old = Poseidon(Poseidon(root_old, epoch_old), salt_old)`
///    and `C_new = Poseidon(Poseidon(root_new, epoch_old + 1), salt_new)`.
/// 4. Run Groth16 over `UpdateCircuit` with the three public inputs
///    `(C_old, epoch_old, C_new)`.
pub fn prove_update<R: Rng + rand::CryptoRng>(
    pk: &ProvingKey<Bls12_381>,
    input: &UpdateProverInput,
    rng: &mut R,
) -> Result<(Proof<Bls12_381>, UpdatePublicInputs), Box<dyn std::error::Error>> {
    let config = poseidon_config::<Fr>();

    let ordered_old = canonicalize_members(&input.members_old)?;
    let ordered_new = canonicalize_members(&input.members_new)?;

    let prover_pk_bytes = compressed_public_key_bytes(&input.secret_key);
    let prover_leaf = compute_leaf_hash(&input.secret_key);
    let prover_index_old = ordered_old
        .iter()
        .position(|m| m.public_key_bytes == prover_pk_bytes)
        .ok_or("prover public key not present in old member roster")?;
    if ordered_old[prover_index_old].leaf_hash != prover_leaf {
        return Err("old roster leaf hash does not match prover secret key".into());
    }

    let tree_old = PoseidonMerkleTree::build_from_members(&config, &ordered_old, input.depth)?;
    let tree_new = PoseidonMerkleTree::build_from_members(&config, &ordered_new, input.depth)?;
    let root_old = tree_old.root();
    let root_new = tree_new.root();
    let merkle_proof = tree_old.prove(prover_index_old);

    let c_old = compute_poseidon_commitment(&config, &root_old, input.epoch_old, &input.salt_old);
    let c_new = compute_poseidon_commitment(
        &config,
        &root_new,
        input.epoch_old + 1,
        &input.salt_new,
    );

    let circuit = UpdateCircuit::new(
        c_old,
        input.epoch_old,
        c_new,
        input.secret_key,
        root_old,
        input.salt_old,
        merkle_proof.path,
        merkle_proof.leaf_index,
        root_new,
        input.salt_new,
        input.depth,
    );

    let proof = Groth16::<Bls12_381>::prove(pk, circuit, rng)?;

    let public_inputs = UpdatePublicInputs {
        c_old,
        epoch_old: Fr::from(input.epoch_old),
        c_new,
    };

    Ok((proof, public_inputs))
}

/// Verify an update-transition proof with a prepared verifying key.
pub fn verify_update(
    pvk: &PreparedVerifyingKey<Bls12_381>,
    proof: &Proof<Bls12_381>,
    public_inputs: &UpdatePublicInputs,
) -> Result<bool, Box<dyn std::error::Error>> {
    let inputs = public_inputs.to_scalars();
    let result = Groth16::<Bls12_381>::verify_with_processed_vk(pvk, &inputs, proof)?;
    Ok(result)
}

/// Serialize a Groth16 proof to the canonical 192-byte format:
/// π_A (G1, 48 bytes compressed) || π_B (G2, 96 bytes compressed) || π_C (G1, 48 bytes compressed)
pub fn proof_to_bytes(proof: &Proof<Bls12_381>) -> Vec<u8> {
    use ark_serialize::CanonicalSerialize;
    let mut bytes = Vec::new();
    proof.serialize_compressed(&mut bytes).expect("Proof serialization should not fail");
    bytes
}

/// Deserialize a Groth16 proof from bytes.
pub fn proof_from_bytes(bytes: &[u8]) -> Result<Proof<Bls12_381>, Box<dyn std::error::Error>> {
    use ark_serialize::CanonicalDeserialize;
    let proof = Proof::<Bls12_381>::deserialize_compressed(bytes)?;
    Ok(proof)
}

/// Decompress a Groth16 proof into uncompressed curve-point components
/// matching the Soroban contract's expected format:
///   π_A (G1, 96 bytes) || π_B (G2, 192 bytes) || π_C (G1, 96 bytes)
/// Total: 384 bytes.
pub fn proof_to_uncompressed_components(proof: &Proof<Bls12_381>) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    use ark_serialize::CanonicalSerialize;
    let mut a = Vec::new();
    proof.a.serialize_uncompressed(&mut a).expect("G1 serialization should not fail");
    let mut b = Vec::new();
    proof.b.serialize_uncompressed(&mut b).expect("G2 serialization should not fail");
    let mut c = Vec::new();
    proof.c.serialize_uncompressed(&mut c).expect("G1 serialization should not fail");
    (a, b, c)
}

/// Helper: compute leaf hashes from secret keys.
/// Each member calls `Poseidon(sk)` on their own secret key and shares
/// the result during group registration.
pub fn compute_leaf_hash(secret_key: &Fr) -> Fr {
    let config = poseidon_config::<Fr>();
    crate::poseidon::poseidon_hash_one(&config, secret_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::poseidon::{poseidon_config, poseidon_hash_one};
    use rand::SeedableRng;
    use rand_chacha::ChaCha20Rng;

    fn test_rng() -> ChaCha20Rng {
        ChaCha20Rng::seed_from_u64(42)
    }

    /// Helper: build ProverInput from secret keys (for testing convenience).
    fn make_prover_input(
        secret_keys: &[Fr],
        prover_index: usize,
        epoch: u64,
        salt: Salt,
        depth: usize,
    ) -> ProverInput {
        let config = poseidon_config::<Fr>();
        let members: Vec<CanonicalMember<Fr>> = secret_keys.iter()
            .map(|sk| CanonicalMember {
                public_key_bytes: compressed_public_key_bytes(sk),
                leaf_hash: poseidon_hash_one(&config, sk),
            })
            .collect();

        ProverInput {
            members,
            secret_key: secret_keys[prover_index],
            epoch,
            salt,
            depth,
        }
    }

    #[test]
    fn test_full_pipeline_depth_5() {
        let mut rng = test_rng();

        // Setup
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        // Prove
        let input = make_prover_input(
            &[Fr::from(100u64), Fr::from(200u64)],
            0, 0, [0xAA; 32], 5,
        );

        let (proof, public_inputs) = prove(&setup_result.proving_key, &input, &mut rng)
            .expect("Proving failed");

        // Verify
        let valid = verify(&setup_result.prepared_vk, &proof, &public_inputs)
            .expect("Verification failed");

        assert!(valid, "Valid proof must verify");
    }

    #[test]
    fn test_different_members_different_proofs() {
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let keys = vec![Fr::from(100u64), Fr::from(200u64)];

        // Member 0 proves
        let input_0 = make_prover_input(&keys, 0, 0, [0xAA; 32], 5);
        let (proof_0, pi_0) = prove(&setup_result.proving_key, &input_0, &mut rng).unwrap();

        // Member 1 proves
        let mut rng2 = ChaCha20Rng::seed_from_u64(99);
        let input_1 = make_prover_input(&keys, 1, 0, [0xAA; 32], 5);
        let (proof_1, pi_1) = prove(&setup_result.proving_key, &input_1, &mut rng2).unwrap();

        // Both should verify
        assert!(verify(&setup_result.prepared_vk, &proof_0, &pi_0).unwrap());
        assert!(verify(&setup_result.prepared_vk, &proof_1, &pi_1).unwrap());

        // But the proofs themselves should be different (randomized)
        let bytes_0 = proof_to_bytes(&proof_0);
        let bytes_1 = proof_to_bytes(&proof_1);
        assert_ne!(bytes_0, bytes_1, "Different provers should produce different proofs");
    }

    #[test]
    fn test_proof_serialization_roundtrip() {
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let input = make_prover_input(&[Fr::from(42u64)], 0, 0, [0x11; 32], 5);

        let (proof, public_inputs) = prove(&setup_result.proving_key, &input, &mut rng).unwrap();

        // Serialize and deserialize
        let bytes = proof_to_bytes(&proof);
        let recovered = proof_from_bytes(&bytes).expect("Deserialization failed");

        // Verify the recovered proof
        let valid = verify(&setup_result.prepared_vk, &recovered, &public_inputs).unwrap();
        assert!(valid, "Deserialized proof must still verify");
    }

    #[test]
    fn test_proof_size() {
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let input = make_prover_input(&[Fr::from(1u64)], 0, 0, [0; 32], 5);

        let (proof, _) = prove(&setup_result.proving_key, &input, &mut rng).unwrap();
        let bytes = proof_to_bytes(&proof);

        // Groth16 over BLS12-381 compressed: 48 + 96 + 48 = 192 bytes
        println!("Proof size: {} bytes", bytes.len());
        assert_eq!(bytes.len(), 192, "Groth16/BLS12-381 proof must be 192 bytes");
    }

    #[test]
    fn test_verification_rejects_tampered_proof() {
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let input = make_prover_input(&[Fr::from(100u64)], 0, 0, [0xAA; 32], 5);

        let (proof, public_inputs) = prove(&setup_result.proving_key, &input, &mut rng).unwrap();

        // Tamper with the proof bytes
        let mut bytes = proof_to_bytes(&proof);
        bytes[10] ^= 0xFF;

        // Tampered proof should either fail to deserialize or fail to verify
        match proof_from_bytes(&bytes) {
            Ok(tampered_proof) => {
                let result = verify(&setup_result.prepared_vk, &tampered_proof, &public_inputs);
                match result {
                    Ok(valid) => assert!(!valid, "Tampered proof must not verify"),
                    Err(_) => {} // Verification error is also acceptable
                }
            }
            Err(_) => {} // Deserialization failure is acceptable
        }
    }

    #[test]
    fn test_verification_rejects_wrong_public_inputs() {
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let input = make_prover_input(&[Fr::from(100u64)], 0, 0, [0xAA; 32], 5);

        let (proof, _) = prove(&setup_result.proving_key, &input, &mut rng).unwrap();

        // Wrong public inputs
        let wrong_pi = PublicInputs {
            commitment: Fr::from(9999u64),
            epoch: Fr::from(9999u64),
        };

        let valid = verify(&setup_result.prepared_vk, &proof, &wrong_pi).unwrap();
        assert!(!valid, "Proof must not verify against wrong public inputs");
    }

    #[test]
    fn test_epoch_transition() {
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let keys = vec![Fr::from(10u64), Fr::from(20u64), Fr::from(30u64)];

        // Epoch 0
        let input_0 = make_prover_input(&keys, 0, 0, [0xAA; 32], 5);
        let (proof_0, pi_0) = prove(&setup_result.proving_key, &input_0, &mut rng).unwrap();
        assert!(verify(&setup_result.prepared_vk, &proof_0, &pi_0).unwrap());

        // Epoch 1 — same members, new salt (as per SEP requirement)
        let input_1 = make_prover_input(&keys, 1, 1, [0xBB; 32], 5);
        let (proof_1, pi_1) = prove(&setup_result.proving_key, &input_1, &mut rng).unwrap();
        assert!(verify(&setup_result.prepared_vk, &proof_1, &pi_1).unwrap());

        // Epoch 0 proof should NOT verify against epoch 1 public inputs
        let valid_cross = verify(&setup_result.prepared_vk, &proof_0, &pi_1).unwrap();
        assert!(!valid_cross, "Epoch 0 proof must not verify against epoch 1 inputs");
    }

    #[test]
    fn test_wrong_secret_key_rejected_in_proof() {
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let config = poseidon_config::<Fr>();
        let real_keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let members: Vec<CanonicalMember<Fr>> = real_keys.iter()
            .map(|sk| CanonicalMember {
                public_key_bytes: compressed_public_key_bytes(sk),
                leaf_hash: poseidon_hash_one(&config, sk),
            })
            .collect();

        // Attacker knows the leaf hashes but not the secret keys.
        // They try to use a wrong secret key.
        let wrong_key = Fr::from(999u64);
        let input = ProverInput {
            members: members.clone(),
            secret_key: wrong_key,   // not a real member's key
            epoch: 0,
            salt: [0xAA; 32],
            depth: 5,
        };

        // arkworks' Groth16::prove asserts constraint satisfaction,
        // so an invalid witness causes a panic. Catch it.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prove(&setup_result.proving_key, &input, &mut ChaCha20Rng::seed_from_u64(77))
        }));

        // Either the prover panics (assertion failure) or returns an error —
        // both confirm that an invalid secret key cannot produce a valid proof.
        match result {
            Ok(Ok((proof, pi))) => {
                let valid = verify(&setup_result.prepared_vk, &proof, &pi).unwrap();
                assert!(!valid, "Proof with wrong secret key must not verify");
            }
            Ok(Err(_)) | Err(_) => {
                // Expected: proving failed due to unsatisfied constraints
            }
        }
    }

    #[test]
    fn test_compute_leaf_hash_matches_tree() {
        let config = poseidon_config::<Fr>();
        let sk = Fr::from(42u64);

        let leaf = compute_leaf_hash(&sk);
        let expected = poseidon_hash_one(&config, &sk);
        assert_eq!(leaf, expected);
    }

    #[test]
    fn test_same_group_different_members_same_public_inputs() {
        // Different members of the same group, same epoch/salt, must produce
        // identical public inputs (same commitment, same epoch). Only the
        // proof bytes differ.
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let salt = [0xAA; 32];
        let epoch = 0u64;

        let input_0 = make_prover_input(&keys, 0, epoch, salt, 5);
        let (_, pi_0) = prove(&setup_result.proving_key, &input_0, &mut rng).unwrap();

        let mut rng2 = ChaCha20Rng::seed_from_u64(99);
        let input_1 = make_prover_input(&keys, 1, epoch, salt, 5);
        let (_, pi_1) = prove(&setup_result.proving_key, &input_1, &mut rng2).unwrap();

        assert_eq!(
            pi_0.commitment, pi_1.commitment,
            "Same group state must produce same commitment regardless of prover"
        );
        assert_eq!(
            pi_0.epoch, pi_1.epoch,
            "Same epoch must produce same epoch public input"
        );
    }

    #[test]
    fn test_proof_uncompressed_components_sizes() {
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let input = make_prover_input(&[Fr::from(42u64)], 0, 0, [0x11; 32], 5);
        let (proof, _) = prove(&setup_result.proving_key, &input, &mut rng).unwrap();

        // Compressed: 48 + 96 + 48 = 192
        let compressed = proof_to_bytes(&proof);
        assert_eq!(compressed.len(), 192);

        // Uncompressed components: G1(96) + G2(192) + G1(96) = 384
        let (a, b, c) = proof_to_uncompressed_components(&proof);
        assert_eq!(a.len(), 96, "proof_a (G1 uncompressed) must be 96 bytes");
        assert_eq!(b.len(), 192, "proof_b (G2 uncompressed) must be 192 bytes");
        assert_eq!(c.len(), 96, "proof_c (G1 uncompressed) must be 96 bytes");
    }

    #[test]
    fn test_proof_compressed_to_uncompressed_roundtrip() {
        use ark_serialize::CanonicalDeserialize;
        use ark_bls12_381::{G1Affine, G2Affine};

        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let input = make_prover_input(&[Fr::from(42u64)], 0, 0, [0x11; 32], 5);
        let (proof, public_inputs) = prove(&setup_result.proving_key, &input, &mut rng).unwrap();

        // Decompress → re-parse each component as a valid curve point
        let (a_bytes, b_bytes, c_bytes) = proof_to_uncompressed_components(&proof);

        let a = G1Affine::deserialize_uncompressed(&a_bytes[..])
            .expect("proof_a must deserialize as a valid G1 point");
        let b = G2Affine::deserialize_uncompressed(&b_bytes[..])
            .expect("proof_b must deserialize as a valid G2 point");
        let c = G1Affine::deserialize_uncompressed(&c_bytes[..])
            .expect("proof_c must deserialize as a valid G1 point");

        // Reconstruct a Proof from the uncompressed components and verify
        let reconstructed = ark_groth16::Proof::<Bls12_381> { a, b, c };
        let valid = verify(&setup_result.prepared_vk, &reconstructed, &public_inputs).unwrap();
        assert!(valid, "Proof reconstructed from uncompressed components must verify");
    }

    /// End-to-end integration test: simulates the full pipeline from proof
    /// generation through to contract-ready artifacts.
    ///
    /// 1. Trusted setup → VK + PK
    /// 2. Proof generation → compressed (192 bytes)
    /// 3. Decompression → uncompressed components (96 + 192 + 96)
    /// 4. VK serialization → uncompressed alpha/beta/gamma/delta/IC
    /// 5. Epoch transition: generate second proof at epoch 1
    /// 6. Cross-epoch rejection: epoch 0 proof fails against epoch 1 inputs
    ///
    /// This validates the exact byte-level pipeline that the testnet
    /// deployment script and Soroban contract consume.
    #[test]
    fn test_end_to_end_proof_to_contract_pipeline() {
        use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
        use ark_bls12_381::{G1Affine, G2Affine};
        use crate::commitment::field_to_bytes_be;

        let mut rng = test_rng();

        // === Phase 1: Trusted setup ===
        let setup_result = setup(5, &mut rng).expect("Setup failed");
        let vk = &setup_result.verifying_key;

        // Serialize VK to uncompressed (contract format)
        let mut alpha_bytes = Vec::new();
        vk.alpha_g1.serialize_uncompressed(&mut alpha_bytes).unwrap();
        assert_eq!(alpha_bytes.len(), 96, "alpha_g1 must be 96 bytes uncompressed");

        let mut beta_bytes = Vec::new();
        vk.beta_g2.serialize_uncompressed(&mut beta_bytes).unwrap();
        assert_eq!(beta_bytes.len(), 192, "beta_g2 must be 192 bytes uncompressed");

        let mut gamma_bytes = Vec::new();
        vk.gamma_g2.serialize_uncompressed(&mut gamma_bytes).unwrap();
        assert_eq!(gamma_bytes.len(), 192, "gamma_g2 must be 192 bytes uncompressed");

        let mut delta_bytes = Vec::new();
        vk.delta_g2.serialize_uncompressed(&mut delta_bytes).unwrap();
        assert_eq!(delta_bytes.len(), 192, "delta_g2 must be 192 bytes uncompressed");

        // IC points — contract expects exactly 3 (base + commitment + epoch)
        assert_eq!(vk.gamma_abc_g1.len(), 3, "VK must have exactly 3 IC points");
        for (i, ic_point) in vk.gamma_abc_g1.iter().enumerate() {
            let mut ic_bytes = Vec::new();
            ic_point.serialize_uncompressed(&mut ic_bytes).unwrap();
            assert_eq!(ic_bytes.len(), 96, "IC[{i}] must be 96 bytes uncompressed");
        }

        // === Phase 2: Proof generation at epoch 0 ===
        let keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let input_0 = make_prover_input(&keys, 0, 0, [0xAA; 32], 5);
        let (proof_0, pi_0) = prove(&setup_result.proving_key, &input_0, &mut rng).unwrap();

        // Compressed proof
        let compressed = proof_to_bytes(&proof_0);
        assert_eq!(compressed.len(), 192, "Compressed proof must be 192 bytes");

        // === Phase 3: Decompress to contract format ===
        let (a_bytes, b_bytes, c_bytes) = proof_to_uncompressed_components(&proof_0);
        assert_eq!(a_bytes.len(), 96);
        assert_eq!(b_bytes.len(), 192);
        assert_eq!(c_bytes.len(), 96);

        // Verify they're valid curve points
        let a = G1Affine::deserialize_uncompressed(&a_bytes[..]).expect("proof_a must be valid G1");
        let b = G2Affine::deserialize_uncompressed(&b_bytes[..]).expect("proof_b must be valid G2");
        let c = G1Affine::deserialize_uncompressed(&c_bytes[..]).expect("proof_c must be valid G1");

        // Reconstruct and verify
        let reconstructed = ark_groth16::Proof::<Bls12_381> { a, b, c };
        assert!(verify(&setup_result.prepared_vk, &reconstructed, &pi_0).unwrap());

        // === Phase 4: Compressed → deserialized roundtrip ===
        let recovered = proof_from_bytes(&compressed).unwrap();
        assert!(verify(&setup_result.prepared_vk, &recovered, &pi_0).unwrap());

        // === Phase 5: Public inputs match contract format ===
        let commitment_bytes = field_to_bytes_be(&pi_0.commitment);
        assert_eq!(commitment_bytes.len(), 32, "Commitment must be 32 bytes");

        // === Phase 6: Epoch transition ===
        let mut rng2 = ChaCha20Rng::seed_from_u64(99);
        let input_1 = make_prover_input(&keys, 0, 1, [0xBB; 32], 5);
        let (proof_1, pi_1) = prove(&setup_result.proving_key, &input_1, &mut rng2).unwrap();

        // Epoch 1 proof verifies with epoch 1 inputs
        assert!(verify(&setup_result.prepared_vk, &proof_1, &pi_1).unwrap());

        // Epoch 0 proof MUST NOT verify with epoch 1 inputs (replay protection)
        let cross_valid = verify(&setup_result.prepared_vk, &proof_0, &pi_1).unwrap();
        assert!(!cross_valid, "Epoch 0 proof must not verify against epoch 1 inputs");

        // Epoch 1 decompressed proof also verifies
        let (a1, b1, c1) = proof_to_uncompressed_components(&proof_1);
        let a1 = G1Affine::deserialize_uncompressed(&a1[..]).unwrap();
        let b1 = G2Affine::deserialize_uncompressed(&b1[..]).unwrap();
        let c1 = G1Affine::deserialize_uncompressed(&c1[..]).unwrap();
        let reconstructed_1 = ark_groth16::Proof::<Bls12_381> { a: a1, b: b1, c: c1 };
        assert!(verify(&setup_result.prepared_vk, &reconstructed_1, &pi_1).unwrap());
    }

    #[test]
    fn test_max_epoch_value() {
        // Boundary test: epoch = u64::MAX must work correctly.
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let input = make_prover_input(
            &[Fr::from(42u64)],
            0, u64::MAX, [0xAA; 32], 5,
        );

        let (proof, public_inputs) = prove(&setup_result.proving_key, &input, &mut rng)
            .expect("Proving with max epoch failed");

        let valid = verify(&setup_result.prepared_vk, &proof, &public_inputs)
            .expect("Verification failed");

        assert!(valid, "Proof with u64::MAX epoch must verify");
        assert_eq!(public_inputs.epoch, Fr::from(u64::MAX));
    }

    #[test]
    fn test_prove_non_member_fails_at_roster_lookup() {
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let config = poseidon_config::<Fr>();
        let real_keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let members: Vec<CanonicalMember<Fr>> = real_keys.iter()
            .map(|sk| CanonicalMember {
                public_key_bytes: compressed_public_key_bytes(sk),
                leaf_hash: poseidon_hash_one(&config, sk),
            })
            .collect();

        // Non-member tries to prove — their public key won't be in the roster
        let non_member_key = Fr::from(999u64);
        let input = ProverInput {
            members,
            secret_key: non_member_key,
            epoch: 0,
            salt: [0xAA; 32],
            depth: 5,
        };

        let result = prove(&setup_result.proving_key, &input, &mut rng);
        assert!(result.is_err(), "Non-member must fail during proof generation");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not present"),
            "Error should indicate missing member, got: {err_msg}"
        );
    }

    #[test]
    fn test_prove_empty_member_set_fails() {
        let mut rng = test_rng();
        let setup_result = setup(5, &mut rng).expect("Setup failed");

        let input = ProverInput {
            members: vec![],
            secret_key: Fr::from(42u64),
            epoch: 0,
            salt: [0xAA; 32],
            depth: 5,
        };

        let result = prove(&setup_result.proving_key, &input, &mut rng);
        assert!(result.is_err(), "Empty member set must fail");
    }

    // ========================================================
    // UpdateCircuit prover tests
    // ========================================================

    fn make_members(keys: &[Fr]) -> Vec<CanonicalMember<Fr>> {
        let config = poseidon_config::<Fr>();
        keys.iter()
            .map(|sk| CanonicalMember {
                public_key_bytes: compressed_public_key_bytes(sk),
                leaf_hash: poseidon_hash_one(&config, sk),
            })
            .collect()
    }

    #[test]
    fn test_prove_update_round_trip() {
        let mut rng = test_rng();
        let setup_result = setup_update(5, &mut rng).expect("update setup failed");

        let old_keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let new_keys = vec![Fr::from(100u64), Fr::from(200u64), Fr::from(300u64)];

        let input = UpdateProverInput {
            members_old: make_members(&old_keys),
            members_new: make_members(&new_keys),
            secret_key: old_keys[0],
            epoch_old: 0,
            salt_old: [0xAA; 32],
            salt_new: [0xBB; 32],
            depth: 5,
        };

        let (proof, public_inputs) =
            prove_update(&setup_result.proving_key, &input, &mut rng).expect("prove failed");

        let valid = verify_update(&setup_result.prepared_vk, &proof, &public_inputs)
            .expect("verify failed");
        assert!(valid, "valid update proof must verify");
    }

    #[test]
    fn test_prove_update_canonical_member_order() {
        let mut rng = test_rng();
        let setup_result = setup_update(5, &mut rng).expect("update setup failed");

        let old_keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let new_keys_sorted = vec![Fr::from(100u64), Fr::from(200u64), Fr::from(300u64)];
        let new_keys_shuffled = vec![Fr::from(300u64), Fr::from(100u64), Fr::from(200u64)];

        let members_old = make_members(&old_keys);

        let base = UpdateProverInput {
            members_old: members_old.clone(),
            members_new: make_members(&new_keys_sorted),
            secret_key: old_keys[0],
            epoch_old: 0,
            salt_old: [0xAA; 32],
            salt_new: [0xBB; 32],
            depth: 5,
        };
        let shuffled = UpdateProverInput {
            members_old,
            members_new: make_members(&new_keys_shuffled),
            secret_key: old_keys[0],
            epoch_old: 0,
            salt_old: [0xAA; 32],
            salt_new: [0xBB; 32],
            depth: 5,
        };

        let (_, pi_a) = prove_update(&setup_result.proving_key, &base, &mut rng).unwrap();
        let (_, pi_b) = prove_update(&setup_result.proving_key, &shuffled, &mut rng).unwrap();

        assert_eq!(pi_a.c_new, pi_b.c_new, "canonical order must yield the same C_new");
    }

    #[test]
    fn test_prove_update_wrong_sk_fails() {
        let mut rng = test_rng();
        let setup_result = setup_update(5, &mut rng).expect("update setup failed");

        let old_keys = vec![Fr::from(100u64), Fr::from(200u64)];
        let new_keys = vec![Fr::from(100u64), Fr::from(200u64), Fr::from(300u64)];

        let input = UpdateProverInput {
            members_old: make_members(&old_keys),
            members_new: make_members(&new_keys),
            // Key 999 is not in members_old.
            secret_key: Fr::from(999u64),
            epoch_old: 0,
            salt_old: [0xAA; 32],
            salt_new: [0xBB; 32],
            depth: 5,
        };

        let result = prove_update(&setup_result.proving_key, &input, &mut rng);
        assert!(result.is_err(), "sk not in old roster must fail to prove");
    }

    #[test]
    fn test_update_public_inputs_serialize_roundtrip() {
        let c_old = Fr::from(0x1111_2222_3333_4444u64);
        let c_new = Fr::from(0xAAAA_BBBB_CCCC_DDDDu64);
        let pi = UpdatePublicInputs {
            c_old,
            epoch_old: Fr::from(42u64),
            c_new,
        };
        let bytes = pi.serialize();
        assert_eq!(bytes.len(), UpdatePublicInputs::SERIALIZED_LEN);
        assert_eq!(bytes[0], UpdatePublicInputs::VERSION);
        // Big-endian u64 at offset 33..41.
        assert_eq!(&bytes[33..41], &42u64.to_be_bytes());

        let round = UpdatePublicInputs::deserialize(&bytes).expect("deserialize");
        assert_eq!(round.c_old, pi.c_old);
        assert_eq!(round.epoch_old, pi.epoch_old);
        assert_eq!(round.c_new, pi.c_new);
    }

    #[test]
    fn test_update_public_inputs_version_mismatch_rejected() {
        let pi = UpdatePublicInputs {
            c_old: Fr::from(1u64),
            epoch_old: Fr::from(0u64),
            c_new: Fr::from(2u64),
        };
        let mut bytes = pi.serialize().to_vec();
        bytes[0] = 0x01; // wrong version
        let err = UpdatePublicInputs::deserialize(&bytes);
        assert!(err.is_err(), "wrong version must be rejected");
    }

    #[test]
    fn test_update_public_inputs_length_mismatch_rejected() {
        let mut bytes = vec![UpdatePublicInputs::VERSION];
        bytes.extend_from_slice(&[0u8; 50]); // < 73
        let err = UpdatePublicInputs::deserialize(&bytes);
        assert!(err.is_err(), "wrong length must be rejected");
    }

    #[test]
    fn test_update_public_inputs_scalar_order_pinned() {
        let pi = UpdatePublicInputs {
            c_old: Fr::from(0xCAFEu64),
            epoch_old: Fr::from(7u64),
            c_new: Fr::from(0xBEEFu64),
        };
        let scalars = pi.to_scalars();
        assert_eq!(scalars[0], pi.c_old);
        assert_eq!(scalars[1], pi.epoch_old);
        assert_eq!(scalars[2], pi.c_new);
    }
}
