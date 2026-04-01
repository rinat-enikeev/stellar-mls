//! JNI bindings for Android. Each function corresponds to a native method
//! in `com.stellarmls.mls.RustBridge`.

use jni::objects::{JByteArray, JClass};
use jni::sys::jbyteArray;
use jni::JNIEnv;

use ark_bls12_381::{Bls12_381, Fr};
use ark_groth16::ProvingKey;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use k256::schnorr::{signature::Signer, Signature, SigningKey};
use rand::rngs::OsRng;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

use crate::commitment::{
    bytes_be_to_field, compute_commitment, compute_poseidon_commitment, field_to_bytes_be, Salt,
    SALT_LEN,
};
use crate::merkle::{
    CanonicalMember, PoseidonMerkleTree, COMPRESSED_G1_PUBLIC_KEY_LEN, compressed_public_key_bytes,
};
use crate::poseidon::poseidon_config;
use crate::prover::{self, ProverInput};

const FR_BYTES: usize = 32;

fn throw_and_null(env: &mut JNIEnv, msg: &str) -> jbyteArray {
    let _ = env.throw_new("java/lang/RuntimeException", msg);
    JByteArray::default().into_raw()
}

fn to_jbytes(env: &mut JNIEnv, bytes: &[u8]) -> jbyteArray {
    match env.byte_array_from_slice(bytes) {
        Ok(arr) => arr.into_raw(),
        Err(_) => throw_and_null(env, "Failed to create byte array"),
    }
}

fn get_bytes(env: &mut JNIEnv, arr: &JByteArray) -> Vec<u8> {
    env.convert_byte_array(arr).unwrap_or_default()
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
    Ok(bytes_be_to_field::<Fr>(&array))
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
// Pattern: extract byte arrays from JNI env first, then do pure Rust logic.

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_computeLeafHash(
    mut env: JNIEnv,
    _class: JClass,
    secret_key: JByteArray,
) -> jbyteArray {
    let sk = get_bytes(&mut env, &secret_key);
    match parse_fr(&sk).map(|fr| prover::compute_leaf_hash(&fr)) {
        Ok(leaf) => to_jbytes(&mut env, &field_to_bytes_be(&leaf)),
        Err(msg) => throw_and_null(&mut env, &msg),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_computePublicKey(
    mut env: JNIEnv,
    _class: JClass,
    secret_key: JByteArray,
) -> jbyteArray {
    let sk = get_bytes(&mut env, &secret_key);
    match parse_fr(&sk).map(|fr| compressed_public_key_bytes(&fr)) {
        Ok(pk) => to_jbytes(&mut env, &pk),
        Err(msg) => throw_and_null(&mut env, &msg),
    }
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
    let result = (|| -> Result<Vec<u8>, String> {
        let members = parse_members(&pk_bytes, &lh_bytes)?;
        let config = poseidon_config::<Fr>();
        let tree = PoseidonMerkleTree::build_from_members(&config, &members, depth as usize)
            .map_err(|e| e.to_string())?;
        Ok(field_to_bytes_be(&tree.root()).to_vec())
    })();
    match result {
        Ok(bytes) => to_jbytes(&mut env, &bytes),
        Err(msg) => throw_and_null(&mut env, &msg),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_nostrDerivePublicKey(
    mut env: JNIEnv,
    _class: JClass,
    secret_key: JByteArray,
) -> jbyteArray {
    let sk = get_bytes(&mut env, &secret_key);
    let result = (|| -> Result<Vec<u8>, String> {
        if sk.len() != 32 {
            return Err(format!("secret key must be 32 bytes, got {}", sk.len()));
        }
        let arr: [u8; 32] = sk.try_into().unwrap();
        let signing_key = SigningKey::from_bytes(&arr).map_err(|e| e.to_string())?;
        Ok(signing_key.verifying_key().to_bytes().to_vec())
    })();
    match result {
        Ok(bytes) => to_jbytes(&mut env, &bytes),
        Err(msg) => throw_and_null(&mut env, &msg),
    }
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
    let result = (|| -> Result<Vec<u8>, String> {
        if sk.len() != 32 {
            return Err(format!("secret key must be 32 bytes, got {}", sk.len()));
        }
        if eid.len() != 32 {
            return Err(format!("event id must be 32 bytes, got {}", eid.len()));
        }
        let sk_arr: [u8; 32] = sk.try_into().unwrap();
        let eid_arr: [u8; 32] = eid.try_into().unwrap();
        let signing_key = SigningKey::from_bytes(&sk_arr).map_err(|e| e.to_string())?;
        let sig: Signature = signing_key.sign(&eid_arr);
        Ok(sig.to_bytes().to_vec())
    })();
    match result {
        Ok(bytes) => to_jbytes(&mut env, &bytes),
        Err(msg) => throw_and_null(&mut env, &msg),
    }
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
    let result = (|| -> Result<Vec<u8>, String> {
        let root = parse_fr(&root_bytes)?;
        if salt_bytes.len() != SALT_LEN {
            return Err(format!("salt must be {} bytes", SALT_LEN));
        }
        let salt_arr: Salt = salt_bytes
            .try_into()
            .map_err(|_| "salt conversion failed".to_string())?;
        Ok(compute_commitment(&root, epoch as u64, &salt_arr).to_vec())
    })();
    match result {
        Ok(bytes) => to_jbytes(&mut env, &bytes),
        Err(msg) => throw_and_null(&mut env, &msg),
    }
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
    let result = (|| -> Result<Vec<u8>, String> {
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
    })();
    match result {
        Ok(bytes) => to_jbytes(&mut env, &bytes),
        Err(msg) => throw_and_null(&mut env, &msg),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_generateTestingProvingKey(
    mut env: JNIEnv,
    _class: JClass,
    depth: i32,
    seed: i64,
) -> jbyteArray {
    let result = (|| -> Result<Vec<u8>, String> {
        let mut rng = ChaCha20Rng::seed_from_u64(seed as u64);
        let setup = prover::setup(depth as usize, &mut rng).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        setup
            .proving_key
            .serialize_compressed(&mut bytes)
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    })();
    match result {
        Ok(bytes) => to_jbytes(&mut env, &bytes),
        Err(msg) => throw_and_null(&mut env, &msg),
    }
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
    let result = (|| -> Result<Vec<u8>, String> {
        let proving_key_obj = ProvingKey::<Bls12_381>::deserialize_compressed(&pk_key_bytes[..])
            .map_err(|e| e.to_string())?;
        let members = parse_members(&pk_bytes, &lh_bytes)?;
        let secret_key_fr = parse_fr(&sk_bytes)?;
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
    })();
    match result {
        Ok(bytes) => to_jbytes(&mut env, &bytes),
        Err(msg) => throw_and_null(&mut env, &msg),
    }
}

/// Returns packed: [4-byte a_len][a][4-byte b_len][b][4-byte c_len][c]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_stellarmls_mls_RustBridge_proofToContractFormat(
    mut env: JNIEnv,
    _class: JClass,
    compressed_proof: JByteArray,
) -> jbyteArray {
    let compressed = get_bytes(&mut env, &compressed_proof);
    let result = (|| -> Result<Vec<u8>, String> {
        let proof = prover::proof_from_bytes(&compressed).map_err(|e| e.to_string())?;
        let (a, b, c) = prover::proof_to_uncompressed_components(&proof);
        Ok(pack_three(&a, &b, &c))
    })();
    match result {
        Ok(bytes) => to_jbytes(&mut env, &bytes),
        Err(msg) => throw_and_null(&mut env, &msg),
    }
}
