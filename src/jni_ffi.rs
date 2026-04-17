//! JNI bindings for Android. Each function corresponds to a native method
//! in `com.stellarmls.mls.RustBridge`.

use std::panic;

use jni::objects::{JByteArray, JClass};
use jni::sys::jbyteArray;
use jni::JNIEnv;

use ark_bls12_381::{Bls12_381, Fr};
use ark_ff::PrimeField;
use ark_groth16::ProvingKey;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use k256::schnorr::{signature::hazmat::PrehashSigner, Signature, SigningKey, VerifyingKey};
use k256::schnorr::signature::hazmat::PrehashVerifier;
use rand::rngs::OsRng;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

use crate::commitment::{
    bytes_be_to_field_checked, compute_commitment, compute_poseidon_commitment, field_to_bytes_be,
    Salt, SALT_LEN,
};
use crate::merkle::{
    CanonicalMember, PoseidonMerkleTree, COMPRESSED_G1_PUBLIC_KEY_LEN, compressed_public_key_bytes,
};
use crate::poseidon::poseidon_config;
use crate::prover::{self, ProverInput, UpdateProverInput, UpdatePublicInputs};

const FR_BYTES: usize = 32;

fn throw_and_null(env: &mut JNIEnv, msg: &str) -> jbyteArray {
    let _ = env.throw_new("java/lang/RuntimeException", msg);
    JByteArray::default().into_raw()
}

/// Run a closure with panic catching. If the closure panics, throw a Java
/// RuntimeException instead of unwinding across the JNI boundary (which is UB).
fn run_jni<F>(env: &mut JNIEnv, f: F) -> jbyteArray
where
    F: FnOnce() -> Result<Vec<u8>, String> + panic::UnwindSafe,
{
    match panic::catch_unwind(f) {
        Ok(Ok(bytes)) => to_jbytes(env, &bytes),
        Ok(Err(msg)) => throw_and_null(env, &msg),
        Err(_) => throw_and_null(env, "Rust panic crossed JNI boundary"),
    }
}

fn to_jbytes(env: &mut JNIEnv, bytes: &[u8]) -> jbyteArray {
    match env.byte_array_from_slice(bytes) {
        Ok(arr) => arr.into_raw(),
        Err(_) => throw_and_null(env, "Failed to create byte array"),
    }
}

/// N-10: Logs a warning and returns empty vec on JNI conversion failure
/// (caller validates length inside run_jni, which will produce a clear error).
fn get_bytes(env: &mut JNIEnv, arr: &JByteArray) -> Vec<u8> {
    match env.convert_byte_array(arr) {
        Ok(bytes) => bytes,
        Err(_) => {
            // Return empty vec; downstream length checks in run_jni will
            // produce a descriptive error like "field element must be 32 bytes, got 0"
            Vec::new()
        }
    }
}

fn parse_fr(bytes: &[u8]) -> Result<Fr, String> {
    if bytes.len() != FR_BYTES {
        return Err(format!(
            "field element must be {} bytes, got {}",
            FR_BYTES,
            bytes.len()
        ));
    }
    let array: [u8; FR_BYTES] = bytes
        .try_into()
        .map_err(|_| "field element conversion failed".to_string())?;
    bytes_be_to_field_checked::<Fr>(&array)
}

/// Parse a secret key as a field element, allowing non-canonical values.
/// See `read_fr_secret_key` in ffi.rs for rationale.
fn parse_fr_secret_key(bytes: &[u8]) -> Result<Fr, String> {
    if bytes.len() != FR_BYTES {
        return Err(format!(
            "secret key must be {} bytes, got {}",
            FR_BYTES,
            bytes.len()
        ));
    }
    Ok(Fr::from_be_bytes_mod_order(bytes))
}

fn parse_members(
    public_key_bytes: &[u8],
    leaf_hash_bytes: &[u8],
) -> Result<Vec<CanonicalMember<Fr>>, String> {
    if public_key_bytes.len() % COMPRESSED_G1_PUBLIC_KEY_LEN != 0 {
        return Err(format!(
            "public key buffer length must be a multiple of {}",
            COMPRESSED_G1_PUBLIC_KEY_LEN
        ));
    }
    if leaf_hash_bytes.len() % FR_BYTES != 0 {
        return Err("leaf hash buffer length must be a multiple of 32".to_string());
    }

    let public_keys: Vec<[u8; COMPRESSED_G1_PUBLIC_KEY_LEN]> = public_key_bytes
        .chunks_exact(COMPRESSED_G1_PUBLIC_KEY_LEN)
        .map(|c| {
            c.try_into()
                .map_err(|_| "public key conversion failed".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    let leaf_hashes: Vec<Fr> = leaf_hash_bytes
        .chunks_exact(FR_BYTES)
        .map(parse_fr)
        .collect::<Result<Vec<_>, _>>()?;

    if public_keys.len() != leaf_hashes.len() {
        return Err(format!(
            "public key count ({}) != leaf hash count ({})",
            public_keys.len(),
            leaf_hashes.len()
        ));
    }

    Ok(public_keys
        .into_iter()
        .zip(leaf_hashes)
        .map(|(pk, lh)| CanonicalMember {
            public_key_bytes: pk,
            leaf_hash: lh,
        })
        .collect())
}

fn pack_two(a: &[u8], b: &[u8]) -> Vec<u8> {
    let mut packed = Vec::with_capacity(8 + a.len() + b.len());
    packed.extend_from_slice(&(a.len() as u32).to_be_bytes());
    packed.extend_from_slice(a);
    packed.extend_from_slice(&(b.len() as u32).to_be_bytes());
    packed.extend_from_slice(b);
    packed
}

fn pack_three(a: &[u8], b: &[u8], c: &[u8]) -> Vec<u8> {
    let mut packed = Vec::with_capacity(12 + a.len() + b.len() + c.len());
    packed.extend_from_slice(&(a.len() as u32).to_be_bytes());
    packed.extend_from_slice(a);
    packed.extend_from_slice(&(b.len() as u32).to_be_bytes());
    packed.extend_from_slice(b);
    packed.extend_from_slice(&(c.len() as u32).to_be_bytes());
    packed.extend_from_slice(c);
    packed
}

// -------- JNI exports --------
// Pattern: extract byte arrays from JNI env first, then do pure Rust logic
// inside run_jni() which catches panics to prevent UB at the JNI boundary.

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_computeLeafHash(
    mut env: JNIEnv,
    _class: JClass,
    secret_key: JByteArray,
) -> jbyteArray {
    let sk = get_bytes(&mut env, &secret_key);
    run_jni(&mut env, move || {
        let fr = parse_fr_secret_key(&sk)?;
        let leaf = prover::compute_leaf_hash(&fr);
        Ok(field_to_bytes_be(&leaf).to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_computePublicKey(
    mut env: JNIEnv,
    _class: JClass,
    secret_key: JByteArray,
) -> jbyteArray {
    let sk = get_bytes(&mut env, &secret_key);
    run_jni(&mut env, move || {
        let fr = parse_fr_secret_key(&sk)?;
        Ok(compressed_public_key_bytes(&fr).to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_computeMerkleRoot(
    mut env: JNIEnv,
    _class: JClass,
    member_public_keys: JByteArray,
    leaf_hashes: JByteArray,
    depth: i32,
) -> jbyteArray {
    let pk_bytes = get_bytes(&mut env, &member_public_keys);
    let lh_bytes = get_bytes(&mut env, &leaf_hashes);
    run_jni(&mut env, move || {
        let members = parse_members(&pk_bytes, &lh_bytes)?;
        let config = poseidon_config::<Fr>();
        let tree = PoseidonMerkleTree::build_from_members(&config, &members, depth as usize)
            .map_err(|e| e.to_string())?;
        Ok(field_to_bytes_be(&tree.root()).to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_nostrDerivePublicKey(
    mut env: JNIEnv,
    _class: JClass,
    secret_key: JByteArray,
) -> jbyteArray {
    let sk = get_bytes(&mut env, &secret_key);
    run_jni(&mut env, move || {
        if sk.len() != 32 {
            return Err(format!("secret key must be 32 bytes, got {}", sk.len()));
        }
        let arr: [u8; 32] = sk.try_into().unwrap();
        let signing_key = SigningKey::from_bytes(&arr).map_err(|e| e.to_string())?;
        Ok(signing_key.verifying_key().to_bytes().to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_nostrSignEventId(
    mut env: JNIEnv,
    _class: JClass,
    secret_key: JByteArray,
    event_id: JByteArray,
) -> jbyteArray {
    let sk = get_bytes(&mut env, &secret_key);
    let eid = get_bytes(&mut env, &event_id);
    run_jni(&mut env, move || {
        if sk.len() != 32 {
            return Err(format!("secret key must be 32 bytes, got {}", sk.len()));
        }
        if eid.len() != 32 {
            return Err(format!("event id must be 32 bytes, got {}", eid.len()));
        }
        let sk_arr: [u8; 32] = sk.try_into().unwrap();
        let eid_arr: [u8; 32] = eid.try_into().unwrap();
        let signing_key = SigningKey::from_bytes(&sk_arr).map_err(|e| e.to_string())?;
        // N-25: Use sign_prehash to sign the raw event ID bytes directly.
        // The event ID is already SHA-256(canonical_json) per NIP-01.
        let sig: Signature = signing_key
            .sign_prehash(&eid_arr)
            .map_err(|e| e.to_string())?;
        Ok(sig.to_bytes().to_vec())
    })
}

/// N-7/N-25: Verify a Nostr Schnorr signature over a pre-hashed event ID.
///
/// Returns a 1-byte array `[1]` on success, or throws a RuntimeException on failure.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_nostrVerifyEventSignature(
    mut env: JNIEnv,
    _class: JClass,
    public_key: JByteArray,
    event_id: JByteArray,
    signature: JByteArray,
) -> jbyteArray {
    let pk = get_bytes(&mut env, &public_key);
    let eid = get_bytes(&mut env, &event_id);
    let sig = get_bytes(&mut env, &signature);
    run_jni(&mut env, move || {
        if pk.len() != 32 {
            return Err(format!("public key must be 32 bytes, got {}", pk.len()));
        }
        if eid.len() != 32 {
            return Err(format!("event id must be 32 bytes, got {}", eid.len()));
        }
        if sig.len() != 64 {
            return Err(format!("signature must be 64 bytes, got {}", sig.len()));
        }
        let pk_arr: [u8; 32] = pk.try_into().unwrap();
        let eid_arr: [u8; 32] = eid.try_into().unwrap();
        let sig_arr: [u8; 64] = sig.try_into().unwrap();

        let verifying_key =
            VerifyingKey::from_bytes(&pk_arr).map_err(|e| e.to_string())?;
        let signature = Signature::try_from(sig_arr.as_slice()).map_err(|e| e.to_string())?;
        verifying_key
            .verify_prehash(&eid_arr, &signature)
            .map_err(|e| e.to_string())?;
        Ok(vec![1u8])
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_computeSha256Commitment(
    mut env: JNIEnv,
    _class: JClass,
    poseidon_root: JByteArray,
    epoch: i64,
    salt: JByteArray,
) -> jbyteArray {
    let root_bytes = get_bytes(&mut env, &poseidon_root);
    let salt_bytes = get_bytes(&mut env, &salt);
    run_jni(&mut env, move || {
        let root = parse_fr(&root_bytes)?;
        if salt_bytes.len() != SALT_LEN {
            return Err(format!("salt must be {} bytes", SALT_LEN));
        }
        let salt_arr: Salt = salt_bytes
            .try_into()
            .map_err(|_| "salt conversion failed".to_string())?;
        Ok(compute_commitment(&root, epoch as u64, &salt_arr).to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_computePoseidonCommitment(
    mut env: JNIEnv,
    _class: JClass,
    poseidon_root: JByteArray,
    epoch: i64,
    salt: JByteArray,
) -> jbyteArray {
    let root_bytes = get_bytes(&mut env, &poseidon_root);
    let salt_bytes = get_bytes(&mut env, &salt);
    run_jni(&mut env, move || {
        let root = parse_fr(&root_bytes)?;
        if salt_bytes.len() != SALT_LEN {
            return Err(format!("salt must be {} bytes", SALT_LEN));
        }
        let salt_arr: Salt = salt_bytes
            .try_into()
            .map_err(|_| "salt conversion failed".to_string())?;
        let config = poseidon_config::<Fr>();
        let commitment = compute_poseidon_commitment(&config, &root, epoch as u64, &salt_arr);
        Ok(field_to_bytes_be(&commitment).to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_generateTestingProvingKey(
    mut env: JNIEnv,
    _class: JClass,
    depth: i32,
    seed: i64,
) -> jbyteArray {
    run_jni(&mut env, move || {
        let mut rng = ChaCha20Rng::seed_from_u64(seed as u64);
        let setup = prover::setup(depth as usize, &mut rng).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        setup
            .proving_key
            .serialize_compressed(&mut bytes)
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    })
}

/// Returns packed: [4-byte proof_len BE][proof][4-byte commitment_len BE][commitment]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_generateMembershipProof(
    mut env: JNIEnv,
    _class: JClass,
    proving_key: JByteArray,
    member_public_keys: JByteArray,
    leaf_hashes: JByteArray,
    secret_key: JByteArray,
    epoch: i64,
    salt: JByteArray,
    depth: i32,
) -> jbyteArray {
    let pk_key_bytes = get_bytes(&mut env, &proving_key);
    let pk_bytes = get_bytes(&mut env, &member_public_keys);
    let lh_bytes = get_bytes(&mut env, &leaf_hashes);
    let sk_bytes = get_bytes(&mut env, &secret_key);
    let salt_bytes = get_bytes(&mut env, &salt);
    run_jni(&mut env, move || {
        let proving_key_obj = ProvingKey::<Bls12_381>::deserialize_compressed(&pk_key_bytes[..])
            .map_err(|e| e.to_string())?;
        let members = parse_members(&pk_bytes, &lh_bytes)?;
        let secret_key_fr = parse_fr_secret_key(&sk_bytes)?;
        if salt_bytes.len() != SALT_LEN {
            return Err(format!("salt must be {} bytes", SALT_LEN));
        }
        let salt_arr: Salt = salt_bytes
            .try_into()
            .map_err(|_| "salt conversion".to_string())?;
        let input = ProverInput {
            members,
            secret_key: secret_key_fr,
            epoch: epoch as u64,
            salt: salt_arr,
            depth: depth as usize,
        };
        let mut rng = OsRng;
        let (proof, public_inputs) =
            prover::prove(&proving_key_obj, &input, &mut rng).map_err(|e| e.to_string())?;
        let proof_bytes = prover::proof_to_bytes(&proof);
        let commitment_bytes = field_to_bytes_be(&public_inputs.commitment).to_vec();
        Ok(pack_two(&proof_bytes, &commitment_bytes))
    })
}

/// Returns packed: [4-byte a_len][a][4-byte b_len][b][4-byte c_len][c]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_proofToContractFormat(
    mut env: JNIEnv,
    _class: JClass,
    compressed_proof: JByteArray,
) -> jbyteArray {
    let compressed = get_bytes(&mut env, &compressed_proof);
    run_jni(&mut env, move || {
        let proof = prover::proof_from_bytes(&compressed).map_err(|e| e.to_string())?;
        let (a, b, c) = prover::proof_to_uncompressed_components(&proof);
        Ok(pack_three(&a, &b, &c))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_generateTestingUpdateProvingKey(
    mut env: JNIEnv,
    _class: JClass,
    depth: i32,
    seed: i64,
) -> jbyteArray {
    run_jni(&mut env, move || {
        let mut rng = ChaCha20Rng::seed_from_u64(seed as u64);
        let setup = prover::setup_update(depth as usize, &mut rng).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        setup
            .proving_key
            .serialize_compressed(&mut bytes)
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    })
}

/// Returns packed: [4-byte proof_len BE][proof][4-byte public_inputs_len BE][public_inputs]
/// where `public_inputs` is the 73-byte `UpdatePublicInputs` wire format.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_generateUpdateProof(
    mut env: JNIEnv,
    _class: JClass,
    proving_key: JByteArray,
    old_member_public_keys: JByteArray,
    old_leaf_hashes: JByteArray,
    new_member_public_keys: JByteArray,
    new_leaf_hashes: JByteArray,
    secret_key: JByteArray,
    epoch_old: i64,
    salt_old: JByteArray,
    salt_new: JByteArray,
    depth: i32,
) -> jbyteArray {
    let pk_key_bytes = get_bytes(&mut env, &proving_key);
    let old_pk_bytes = get_bytes(&mut env, &old_member_public_keys);
    let old_lh_bytes = get_bytes(&mut env, &old_leaf_hashes);
    let new_pk_bytes = get_bytes(&mut env, &new_member_public_keys);
    let new_lh_bytes = get_bytes(&mut env, &new_leaf_hashes);
    let sk_bytes = get_bytes(&mut env, &secret_key);
    let salt_old_bytes = get_bytes(&mut env, &salt_old);
    let salt_new_bytes = get_bytes(&mut env, &salt_new);
    run_jni(&mut env, move || {
        let proving_key_obj = ProvingKey::<Bls12_381>::deserialize_compressed(&pk_key_bytes[..])
            .map_err(|e| e.to_string())?;
        let members_old = parse_members(&old_pk_bytes, &old_lh_bytes)?;
        let members_new = parse_members(&new_pk_bytes, &new_lh_bytes)?;
        let secret_key_fr = parse_fr_secret_key(&sk_bytes)?;
        if salt_old_bytes.len() != SALT_LEN {
            return Err(format!("old salt must be {} bytes", SALT_LEN));
        }
        if salt_new_bytes.len() != SALT_LEN {
            return Err(format!("new salt must be {} bytes", SALT_LEN));
        }
        let salt_old_arr: Salt = salt_old_bytes
            .try_into()
            .map_err(|_| "old salt conversion".to_string())?;
        let salt_new_arr: Salt = salt_new_bytes
            .try_into()
            .map_err(|_| "new salt conversion".to_string())?;
        let input = UpdateProverInput {
            members_old,
            members_new,
            secret_key: secret_key_fr,
            epoch_old: epoch_old as u64,
            salt_old: salt_old_arr,
            salt_new: salt_new_arr,
            depth: depth as usize,
        };
        let mut rng = OsRng;
        let (proof, public_inputs) =
            prover::prove_update(&proving_key_obj, &input, &mut rng).map_err(|e| e.to_string())?;
        let proof_bytes = prover::proof_to_bytes(&proof);
        let pi_bytes = public_inputs.serialize().to_vec();
        Ok(pack_two(&proof_bytes, &pi_bytes))
    })
}

/// Parse the 73-byte update public-inputs wire format.
/// Returns packed: [4-byte c_old_len][c_old][4-byte epoch_old_len][epoch_old][4-byte c_new_len][c_new]
/// Fails loudly on version mismatch or length mismatch.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_parseUpdatePublicInputs(
    mut env: JNIEnv,
    _class: JClass,
    public_inputs: JByteArray,
) -> jbyteArray {
    let bytes = get_bytes(&mut env, &public_inputs);
    run_jni(&mut env, move || {
        let pi = UpdatePublicInputs::deserialize(&bytes)?;
        let c_old = field_to_bytes_be(&pi.c_old).to_vec();
        // Bytes 33..41 of the wire format contain the u64 epoch_old big-endian.
        let epoch_old_be = bytes[33..41].to_vec();
        let c_new = field_to_bytes_be(&pi.c_new).to_vec();
        Ok(pack_three(&c_old, &epoch_old_be, &c_new))
    })
}
