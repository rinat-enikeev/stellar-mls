//! Groth16 prover and verifier pipeline for SEP-XXXX.
//!
//! This module provides the end-to-end workflow:
//! 1. Trusted setup (generates proving key + verification key)
//! 2. Proof generation (member creates a 192-byte proof)
//! 3. Proof verification (contract/anyone verifies with 3 pairings)

use ark_bls12_381::{Bls12_381, Fr};
use ark_groth16::{Groth16, PreparedVerifyingKey, Proof, ProvingKey, VerifyingKey};
use ark_snark::SNARK;
use ark_std::rand::Rng;

use crate::circuit::MembershipCircuit;
use crate::commitment::{Salt, compute_poseidon_commitment};
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
}
