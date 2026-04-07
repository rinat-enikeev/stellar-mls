use std::ffi::{c_char, CString};
use std::mem;
use std::ptr;
use std::slice;

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
use crate::prover::{self, ProverInput};

const FR_BYTES: usize = 32;

#[repr(C)]
pub struct SepByteBuffer {
    pub ptr: *mut u8,
    pub len: usize,
}

fn run_ffi<F>(out_error: *mut *mut c_char, f: F) -> bool
where
    F: FnOnce() -> Result<(), String>,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(())) => {
            if !out_error.is_null() {
                unsafe { *out_error = ptr::null_mut() };
            }
            true
        }
        Ok(Err(message)) => {
            write_error(out_error, &message);
            false
        }
        Err(_) => {
            write_error(out_error, "Rust panic crossed FFI boundary");
            false
        }
    }
}

fn write_error(out_error: *mut *mut c_char, message: &str) {
    if out_error.is_null() {
        return;
    }

    let sanitized = message.replace('\0', " ");
    let c_string = CString::new(sanitized).expect("CString::new should succeed after sanitizing");
    unsafe {
        *out_error = c_string.into_raw();
    }
}

fn buffer_from_vec(mut bytes: Vec<u8>) -> SepByteBuffer {
    let buffer = SepByteBuffer {
        ptr: bytes.as_mut_ptr(),
        len: bytes.len(),
    };
    mem::forget(bytes);
    buffer
}

fn write_buffer(out: *mut SepByteBuffer, bytes: Vec<u8>) -> Result<(), String> {
    if out.is_null() {
        return Err("output buffer pointer was null".to_string());
    }

    unsafe {
        *out = buffer_from_vec(bytes);
    }
    Ok(())
}

fn require_len(name: &str, actual: usize, expected: usize) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{name} must be {expected} bytes, got {actual}"))
    }
}

fn read_bytes<'a>(ptr: *const u8, len: usize, label: &str) -> Result<&'a [u8], String> {
    if ptr.is_null() {
        return Err(format!("{label} pointer was null"));
    }
    unsafe { Ok(slice::from_raw_parts(ptr, len)) }
}

fn read_fr(bytes: &[u8]) -> Result<Fr, String> {
    require_len("field element", bytes.len(), FR_BYTES)?;
    let array: [u8; FR_BYTES] = bytes
        .try_into()
        .map_err(|_| "failed to convert field element bytes".to_string())?;
    bytes_be_to_field_checked::<Fr>(&array)
}

/// Read a secret key as a field element, allowing non-canonical values.
///
/// Secret keys are private witnesses — any valid scalar works. Unlike public
/// inputs (roots, commitments), there is no security benefit to rejecting
/// non-canonical secret keys. This avoids failures when the client generates
/// random 32-byte keys that happen to be >= the field modulus (~50% chance).
fn read_fr_secret_key(bytes: &[u8]) -> Result<Fr, String> {
    require_len("secret key", bytes.len(), FR_BYTES)?;
    Ok(Fr::from_be_bytes_mod_order(bytes))
}

fn read_array_32(bytes: &[u8], label: &str) -> Result<[u8; 32], String> {
    require_len(label, bytes.len(), 32)?;
    bytes
        .try_into()
        .map_err(|_| format!("failed to convert {label} bytes"))
}

fn read_salt(bytes: &[u8]) -> Result<Salt, String> {
    require_len("salt", bytes.len(), SALT_LEN)?;
    bytes
        .try_into()
        .map_err(|_| "failed to convert salt bytes".to_string())
}

fn read_field_vector(bytes: &[u8]) -> Result<Vec<Fr>, String> {
    if bytes.len() % FR_BYTES != 0 {
        return Err("leaf hash byte buffer length must be a multiple of 32".to_string());
    }

    bytes
        .chunks_exact(FR_BYTES)
        .map(read_fr)
        .collect::<Result<Vec<_>, _>>()
}

fn read_public_key_vector(
    bytes: &[u8],
) -> Result<Vec<[u8; COMPRESSED_G1_PUBLIC_KEY_LEN]>, String> {
    if bytes.len() % COMPRESSED_G1_PUBLIC_KEY_LEN != 0 {
        return Err(format!(
            "public key byte buffer length must be a multiple of {}",
            COMPRESSED_G1_PUBLIC_KEY_LEN
        ));
    }

    bytes
        .chunks_exact(COMPRESSED_G1_PUBLIC_KEY_LEN)
        .map(|chunk| {
            chunk
                .try_into()
                .map_err(|_| "failed to convert public key bytes".to_string())
        })
        .collect::<Result<Vec<_>, _>>()
}

fn build_members(
    public_keys: Vec<[u8; COMPRESSED_G1_PUBLIC_KEY_LEN]>,
    leaf_hashes: Vec<Fr>,
) -> Result<Vec<CanonicalMember<Fr>>, String> {
    if public_keys.len() != leaf_hashes.len() {
        return Err(format!(
            "member public key count ({}) does not match leaf hash count ({})",
            public_keys.len(),
            leaf_hashes.len()
        ));
    }

    Ok(public_keys
        .into_iter()
        .zip(leaf_hashes)
        .map(|(public_key_bytes, leaf_hash)| CanonicalMember {
            public_key_bytes,
            leaf_hash,
        })
        .collect())
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_byte_buffer_free(buffer: SepByteBuffer) {
    if !buffer.ptr.is_null() {
        unsafe {
            let _ = Vec::from_raw_parts(buffer.ptr, buffer.len, buffer.len);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_compute_leaf_hash(
    secret_key_ptr: *const u8,
    secret_key_len: usize,
    out_leaf_hash: *mut SepByteBuffer,
    out_error: *mut *mut c_char,
) -> bool {
    run_ffi(out_error, || {
        let secret_key_bytes = read_bytes(secret_key_ptr, secret_key_len, "secret key")?;
        let secret_key = read_fr_secret_key(secret_key_bytes)?;
        let leaf = prover::compute_leaf_hash(&secret_key);
        write_buffer(out_leaf_hash, field_to_bytes_be(&leaf).to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_compute_public_key(
    secret_key_ptr: *const u8,
    secret_key_len: usize,
    out_public_key: *mut SepByteBuffer,
    out_error: *mut *mut c_char,
) -> bool {
    run_ffi(out_error, || {
        let secret_key_bytes = read_bytes(secret_key_ptr, secret_key_len, "secret key")?;
        let secret_key = read_fr_secret_key(secret_key_bytes)?;
        let public_key = compressed_public_key_bytes(&secret_key);
        write_buffer(out_public_key, public_key.to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_compute_merkle_root(
    member_public_keys_ptr: *const u8,
    member_public_keys_len: usize,
    leaf_hashes_ptr: *const u8,
    leaf_hashes_len: usize,
    depth: usize,
    out_root: *mut SepByteBuffer,
    out_error: *mut *mut c_char,
) -> bool {
    run_ffi(out_error, || {
        let public_key_bytes = read_bytes(
            member_public_keys_ptr,
            member_public_keys_len,
            "member public keys",
        )?;
        let leaf_hash_bytes = read_bytes(leaf_hashes_ptr, leaf_hashes_len, "leaf hashes")?;
        let public_keys = read_public_key_vector(public_key_bytes)?;
        let leaf_hashes = read_field_vector(leaf_hash_bytes)?;
        let members = build_members(public_keys, leaf_hashes)?;
        let config = poseidon_config::<Fr>();
        let tree = PoseidonMerkleTree::build_from_members(&config, &members, depth)
            .map_err(|e| e.to_string())?;
        write_buffer(out_root, field_to_bytes_be(&tree.root()).to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_nostr_derive_public_key(
    secret_key_ptr: *const u8,
    secret_key_len: usize,
    out_public_key: *mut SepByteBuffer,
    out_error: *mut *mut c_char,
) -> bool {
    run_ffi(out_error, || {
        let secret_key_bytes = read_bytes(secret_key_ptr, secret_key_len, "Nostr secret key")?;
        let secret_key_bytes = read_array_32(secret_key_bytes, "Nostr secret key")?;
        let signing_key = SigningKey::from_bytes(&secret_key_bytes).map_err(|e| e.to_string())?;
        let public_key = signing_key.verifying_key().to_bytes();
        write_buffer(out_public_key, public_key.to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_nostr_sign_event_id(
    secret_key_ptr: *const u8,
    secret_key_len: usize,
    event_id_ptr: *const u8,
    event_id_len: usize,
    out_signature: *mut SepByteBuffer,
    out_error: *mut *mut c_char,
) -> bool {
    run_ffi(out_error, || {
        let secret_key_bytes = read_bytes(secret_key_ptr, secret_key_len, "Nostr secret key")?;
        let secret_key_bytes = read_array_32(secret_key_bytes, "Nostr secret key")?;
        let event_id_bytes = read_bytes(event_id_ptr, event_id_len, "Nostr event id")?;
        let event_id_bytes = read_array_32(event_id_bytes, "Nostr event id")?;

        let signing_key = SigningKey::from_bytes(&secret_key_bytes).map_err(|e| e.to_string())?;
        // N-25: Use sign_prehash to sign the raw event ID bytes directly.
        // The event ID is already SHA-256(canonical_json) per NIP-01.
        // The previous Signer::sign() would double-hash, producing invalid signatures.
        let signature: Signature = signing_key
            .sign_prehash(&event_id_bytes)
            .map_err(|e| e.to_string())?;
        write_buffer(out_signature, signature.to_bytes().to_vec())
    })
}

/// N-7/N-25: Verify a Nostr Schnorr signature over a pre-hashed event ID.
///
/// Returns true if the signature is valid, false otherwise.
/// On internal error (e.g., malformed key), writes the error message
/// and returns false.
#[unsafe(no_mangle)]
pub extern "C" fn sep_nostr_verify_event_signature(
    public_key_ptr: *const u8,
    public_key_len: usize,
    event_id_ptr: *const u8,
    event_id_len: usize,
    signature_ptr: *const u8,
    signature_len: usize,
    out_error: *mut *mut c_char,
) -> bool {
    run_ffi(out_error, || {
        let pubkey_bytes = read_bytes(public_key_ptr, public_key_len, "Nostr public key")?;
        let pubkey_bytes = read_array_32(pubkey_bytes, "Nostr public key")?;
        let event_id_bytes = read_bytes(event_id_ptr, event_id_len, "Nostr event id")?;
        let event_id_bytes = read_array_32(event_id_bytes, "Nostr event id")?;
        let sig_bytes = read_bytes(signature_ptr, signature_len, "Nostr signature")?;
        require_len("Nostr signature", sig_bytes.len(), 64)?;
        let sig_array: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| "failed to convert signature bytes".to_string())?;

        let verifying_key =
            VerifyingKey::from_bytes(&pubkey_bytes).map_err(|e| e.to_string())?;
        let signature = Signature::try_from(sig_array.as_slice()).map_err(|e| e.to_string())?;

        verifying_key
            .verify_prehash(&event_id_bytes, &signature)
            .map_err(|e| e.to_string())?;

        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_proof_to_contract_format(
    compressed_proof_ptr: *const u8,
    compressed_proof_len: usize,
    out_proof_a: *mut SepByteBuffer,
    out_proof_b: *mut SepByteBuffer,
    out_proof_c: *mut SepByteBuffer,
    out_error: *mut *mut c_char,
) -> bool {
    run_ffi(out_error, || {
        let compressed = read_bytes(compressed_proof_ptr, compressed_proof_len, "compressed proof")?;
        let proof = prover::proof_from_bytes(compressed).map_err(|e| e.to_string())?;
        let (a, b, c) = prover::proof_to_uncompressed_components(&proof);
        write_buffer(out_proof_a, a)?;
        write_buffer(out_proof_b, b)?;
        write_buffer(out_proof_c, c)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_compute_sha256_commitment(
    poseidon_root_ptr: *const u8,
    poseidon_root_len: usize,
    epoch: u64,
    salt_ptr: *const u8,
    salt_len: usize,
    out_commitment: *mut SepByteBuffer,
    out_error: *mut *mut c_char,
) -> bool {
    run_ffi(out_error, || {
        let root_bytes = read_bytes(poseidon_root_ptr, poseidon_root_len, "poseidon root")?;
        let salt_bytes = read_bytes(salt_ptr, salt_len, "salt")?;
        let root = read_fr(root_bytes)?;
        let salt = read_salt(salt_bytes)?;
        let commitment = compute_commitment(&root, epoch, &salt);
        write_buffer(out_commitment, commitment.to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_compute_poseidon_commitment(
    poseidon_root_ptr: *const u8,
    poseidon_root_len: usize,
    epoch: u64,
    salt_ptr: *const u8,
    salt_len: usize,
    out_commitment: *mut SepByteBuffer,
    out_error: *mut *mut c_char,
) -> bool {
    run_ffi(out_error, || {
        let root_bytes = read_bytes(poseidon_root_ptr, poseidon_root_len, "poseidon root")?;
        let salt_bytes = read_bytes(salt_ptr, salt_len, "salt")?;
        let root = read_fr(root_bytes)?;
        let salt = read_salt(salt_bytes)?;
        let config = poseidon_config::<Fr>();
        let commitment = compute_poseidon_commitment(&config, &root, epoch, &salt);
        write_buffer(out_commitment, field_to_bytes_be(&commitment).to_vec())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_generate_testing_proving_key(
    depth: usize,
    seed: u64,
    out_proving_key: *mut SepByteBuffer,
    out_error: *mut *mut c_char,
) -> bool {
    run_ffi(out_error, || {
        let mut rng = ChaCha20Rng::seed_from_u64(seed);
        let setup = prover::setup(depth, &mut rng).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        setup
            .proving_key
            .serialize_compressed(&mut bytes)
            .map_err(|e| e.to_string())?;
        write_buffer(out_proving_key, bytes)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn sep_generate_membership_proof(
    proving_key_ptr: *const u8,
    proving_key_len: usize,
    member_public_keys_ptr: *const u8,
    member_public_keys_len: usize,
    leaf_hashes_ptr: *const u8,
    leaf_hashes_len: usize,
    secret_key_ptr: *const u8,
    secret_key_len: usize,
    epoch: u64,
    salt_ptr: *const u8,
    salt_len: usize,
    depth: usize,
    out_proof: *mut SepByteBuffer,
    out_commitment: *mut SepByteBuffer,
    out_error: *mut *mut c_char,
) -> bool {
    run_ffi(out_error, || {
        let proving_key_bytes = read_bytes(proving_key_ptr, proving_key_len, "proving key")?;
        let public_key_bytes = read_bytes(
            member_public_keys_ptr,
            member_public_keys_len,
            "member public keys",
        )?;
        let leaf_hash_bytes = read_bytes(leaf_hashes_ptr, leaf_hashes_len, "leaf hashes")?;
        let secret_key_bytes = read_bytes(secret_key_ptr, secret_key_len, "secret key")?;
        let salt_bytes = read_bytes(salt_ptr, salt_len, "salt")?;

        let proving_key = ProvingKey::<Bls12_381>::deserialize_compressed(proving_key_bytes)
            .map_err(|e| e.to_string())?;
        let public_keys = read_public_key_vector(public_key_bytes)?;
        let leaf_hashes = read_field_vector(leaf_hash_bytes)?;
        let members = build_members(public_keys, leaf_hashes)?;
        let secret_key = read_fr_secret_key(secret_key_bytes)?;
        let salt = read_salt(salt_bytes)?;

        let input = ProverInput {
            members,
            secret_key,
            epoch,
            salt,
            depth,
        };

        let mut rng = OsRng;
        let (proof, public_inputs) =
            prover::prove(&proving_key, &input, &mut rng).map_err(|e| e.to_string())?;

        write_buffer(out_proof, prover::proof_to_bytes(&proof))?;
        write_buffer(
            out_commitment,
            field_to_bytes_be(&public_inputs.commitment).to_vec(),
        )?;
        Ok(())
    })
}

#[cfg(test)]
#[allow(unused_unsafe)]
mod tests {
    use super::*;
    use k256::schnorr::{
        signature::hazmat::{PrehashSigner, PrehashVerifier},
        SigningKey, VerifyingKey,
    };
    use sha2::{Digest, Sha256};
    use std::ffi::c_char;

    /// Read output bytes from a SepByteBuffer (does not free it).
    fn read_buffer(buf: &SepByteBuffer) -> Vec<u8> {
        if buf.ptr.is_null() || buf.len == 0 {
            return Vec::new();
        }
        unsafe { slice::from_raw_parts(buf.ptr, buf.len).to_vec() }
    }

    /// Call sep_compute_leaf_hash and return the result bytes.
    fn ffi_leaf_hash(sk: &[u8]) -> Vec<u8> {
        let mut out = SepByteBuffer {
            ptr: ptr::null_mut(),
            len: 0,
        };
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            sep_compute_leaf_hash(sk.as_ptr(), sk.len(), &mut out, &mut err)
        };
        assert!(ok, "sep_compute_leaf_hash failed");
        let result = read_buffer(&out);
        sep_byte_buffer_free(out);
        result
    }

    /// Call sep_compute_public_key and return the result bytes.
    fn ffi_public_key(sk: &[u8]) -> Vec<u8> {
        let mut out = SepByteBuffer {
            ptr: ptr::null_mut(),
            len: 0,
        };
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            sep_compute_public_key(sk.as_ptr(), sk.len(), &mut out, &mut err)
        };
        assert!(ok, "sep_compute_public_key failed");
        let result = read_buffer(&out);
        sep_byte_buffer_free(out);
        result
    }

    fn decode_hex(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Sign/verify round-trip: sign_prehash produces a signature that verify_prehash accepts.
    #[test]
    fn test_nostr_sign_verify_roundtrip() {
        let mut sk_bytes = [0u8; 32];
        sk_bytes[31] = 1;
        let signing_key = SigningKey::from_bytes(&sk_bytes).unwrap();
        let verifying_key = signing_key.verifying_key();

        // Simulate NIP-01 event id (SHA-256 of canonical JSON)
        let event_id: [u8; 32] = Sha256::digest(b"test-event-canonical-json").into();

        let signature = signing_key.sign_prehash(&event_id).unwrap();
        assert!(
            verifying_key.verify_prehash(&event_id, &signature).is_ok(),
            "sign_prehash signature must pass verify_prehash"
        );
    }

    /// Different event IDs produce different signatures that don't cross-verify.
    #[test]
    fn test_nostr_signature_binds_to_event_id() {
        let mut sk_bytes = [0u8; 32];
        sk_bytes[31] = 1;
        let signing_key = SigningKey::from_bytes(&sk_bytes).unwrap();
        let verifying_key = signing_key.verifying_key();

        let eid1: [u8; 32] = Sha256::digest(b"event-1").into();
        let eid2: [u8; 32] = Sha256::digest(b"event-2").into();

        let sig1 = signing_key.sign_prehash(&eid1).unwrap();
        assert!(
            verifying_key.verify_prehash(&eid2, &sig1).is_err(),
            "signature for event-1 must not verify against event-2"
        );
    }

    /// BIP-340 test vector 0:
    /// Secret key: 0x03, Public key: F9308A..., Message: 32 zero bytes
    #[test]
    fn test_bip340_test_vector_0() {
        let sk = decode_hex("0000000000000000000000000000000000000000000000000000000000000003");
        let expected_pubkey = decode_hex(
            "F9308A019258C31049344F85F89D5229B531C845836F99B08601F113BCE036F9",
        );
        let msg = [0u8; 32];
        let expected_sig = decode_hex(
            "E907831F80848D1069A5371B402410364BDF1C5F8307B0084C55F1CE2DCA821525F66A4A85EA8B71E482A74F382D2CE5EBEEE8FDB2172F477DF4900D310536C0",
        );

        let signing_key = SigningKey::from_bytes(sk.as_slice().try_into().unwrap()).unwrap();
        let verifying_key = signing_key.verifying_key();

        assert_eq!(
            verifying_key.to_bytes().as_slice(),
            expected_pubkey.as_slice(),
            "derived pubkey must match BIP-340 test vector"
        );

        let signature = signing_key.sign_prehash(&msg).unwrap();
        assert_eq!(
            signature.to_bytes().as_slice(),
            expected_sig.as_slice(),
            "signature must match BIP-340 test vector"
        );

        assert!(verifying_key.verify_prehash(&msg, &signature).is_ok());
    }

    /// BIP-340 verification-only test vector 4.
    #[test]
    fn test_bip340_test_vector_4_verify() {
        let pk = decode_hex("D69C3509BB99E412E68B0FE8544E72837DFA30746D8BE2AA65975F29D22DC7B9");
        let msg = decode_hex("4DF3C3F68FCC83B27E9D42C90431A72499F17875C81A599B566C9889B9696703");
        let sig = decode_hex(
            "00000000000000000000003B78CE563F89A0ED9414F5AA28AD0D96D6795F9C6376AFB1548AF603B3EB45C9F8207DEE1060CB71C04E80F593060B07D28308D7F4",
        );

        let pk_arr: [u8; 32] = pk.try_into().unwrap();
        let msg_arr: [u8; 32] = msg.try_into().unwrap();
        let sig_arr: [u8; 64] = sig.try_into().unwrap();

        let verifying_key = VerifyingKey::from_bytes(&pk_arr).unwrap();
        let signature = k256::schnorr::Signature::try_from(sig_arr.as_slice()).unwrap();

        assert!(
            verifying_key.verify_prehash(&msg_arr, &signature).is_ok(),
            "BIP-340 test vector 4 must verify"
        );
    }

    /// Negative test: old Signer::sign (double-hashes) must NOT pass verify_prehash.
    #[test]
    fn test_old_signer_double_hash_does_not_verify() {
        use k256::schnorr::signature::Signer;

        let mut sk_bytes = [0u8; 32];
        sk_bytes[31] = 1;
        let signing_key = SigningKey::from_bytes(&sk_bytes).unwrap();
        let verifying_key = signing_key.verifying_key();

        let event_id: [u8; 32] = Sha256::digest(b"double-hash-test").into();

        // Old behavior: Signer::sign internally SHA-256-hashes the input again
        let old_sig: k256::schnorr::Signature = signing_key.sign(&event_id);

        assert!(
            verifying_key.verify_prehash(&event_id, &old_sig).is_err(),
            "Signer::sign output must NOT pass PrehashVerifier::verify_prehash (double-hash bug)"
        );
    }

    // ================================================================
    // Internal Helper Tests
    // ================================================================

    #[test]
    fn test_read_fr_canonical_accepted() {
        // A small canonical field element (1)
        let mut bytes = [0u8; 32];
        bytes[31] = 1;
        assert!(read_fr(&bytes).is_ok());
    }

    #[test]
    fn test_read_fr_zero_accepted() {
        let bytes = [0u8; 32];
        assert!(read_fr(&bytes).is_ok());
    }

    #[test]
    fn test_read_fr_non_canonical_rejected() {
        // BLS12-381 scalar field modulus r is ~2^255, so all-0xFF is >= r
        let bytes = [0xFF; 32];
        assert!(read_fr(&bytes).is_err());
    }

    #[test]
    fn test_read_fr_wrong_length_rejected() {
        let bytes = [0u8; 16];
        assert!(read_fr(&bytes).is_err());
    }

    #[test]
    fn test_read_fr_secret_key_accepts_non_canonical() {
        // Non-canonical values should be accepted (reduced mod r)
        let bytes = [0xFF; 32];
        assert!(read_fr_secret_key(&bytes).is_ok());
    }

    #[test]
    fn test_read_fr_secret_key_accepts_canonical() {
        let mut bytes = [0u8; 32];
        bytes[31] = 42;
        assert!(read_fr_secret_key(&bytes).is_ok());
    }

    #[test]
    fn test_read_fr_secret_key_wrong_length_rejected() {
        let bytes = [0u8; 16];
        assert!(read_fr_secret_key(&bytes).is_err());
    }

    // ================================================================
    // FFI Leaf Hash Tests
    // ================================================================

    #[test]
    fn test_ffi_compute_leaf_hash_32_bytes() {
        let mut sk = [0u8; 32];
        sk[31] = 1;
        let result = ffi_leaf_hash(&sk);
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn test_ffi_compute_leaf_hash_deterministic() {
        let mut sk = [0u8; 32];
        sk[31] = 42;
        let r1 = ffi_leaf_hash(&sk);
        let r2 = ffi_leaf_hash(&sk);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_ffi_compute_leaf_hash_different_keys_differ() {
        let mut sk1 = [0u8; 32];
        sk1[31] = 1;
        let mut sk2 = [0u8; 32];
        sk2[31] = 2;
        assert_ne!(ffi_leaf_hash(&sk1), ffi_leaf_hash(&sk2));
    }

    #[test]
    fn test_ffi_compute_leaf_hash_null_pointer_error() {
        let mut out = SepByteBuffer {
            ptr: ptr::null_mut(),
            len: 0,
        };
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            sep_compute_leaf_hash(ptr::null(), 32, &mut out, &mut err)
        };
        assert!(!ok);
        assert!(!err.is_null());
        unsafe { sep_string_free(err) };
    }

    // ================================================================
    // FFI Public Key Tests
    // ================================================================

    #[test]
    fn test_ffi_compute_public_key_48_bytes() {
        let mut sk = [0u8; 32];
        sk[31] = 1;
        let result = ffi_public_key(&sk);
        assert_eq!(result.len(), 48);
    }

    #[test]
    fn test_ffi_compute_public_key_deterministic() {
        let mut sk = [0u8; 32];
        sk[31] = 99;
        let r1 = ffi_public_key(&sk);
        let r2 = ffi_public_key(&sk);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_ffi_compute_public_key_different_keys_differ() {
        let mut sk1 = [0u8; 32];
        sk1[31] = 1;
        let mut sk2 = [0u8; 32];
        sk2[31] = 2;
        assert_ne!(ffi_public_key(&sk1), ffi_public_key(&sk2));
    }

    // ================================================================
    // FFI Merkle Root Tests
    // ================================================================

    #[test]
    fn test_ffi_compute_merkle_root_single_member() {
        let mut sk = [0u8; 32];
        sk[31] = 1;
        let pk = ffi_public_key(&sk);
        let leaf = ffi_leaf_hash(&sk);

        let mut out = SepByteBuffer {
            ptr: ptr::null_mut(),
            len: 0,
        };
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            sep_compute_merkle_root(
                pk.as_ptr(), pk.len(),
                leaf.as_ptr(), leaf.len(),
                5, // depth
                &mut out, &mut err,
            )
        };
        assert!(ok);
        let root = read_buffer(&out);
        assert_eq!(root.len(), 32);
        sep_byte_buffer_free(out);
    }

    #[test]
    fn test_ffi_compute_merkle_root_order_independent() {
        // Create two members
        let mut sk1 = [0u8; 32];
        sk1[31] = 1;
        let mut sk2 = [0u8; 32];
        sk2[31] = 2;

        let pk1 = ffi_public_key(&sk1);
        let pk2 = ffi_public_key(&sk2);
        let leaf1 = ffi_leaf_hash(&sk1);
        let leaf2 = ffi_leaf_hash(&sk2);

        // Order A: [member1, member2]
        let mut pks_a = pk1.clone();
        pks_a.extend_from_slice(&pk2);
        let mut leaves_a = leaf1.clone();
        leaves_a.extend_from_slice(&leaf2);

        // Order B: [member2, member1]
        let mut pks_b = pk2.clone();
        pks_b.extend_from_slice(&pk1);
        let mut leaves_b = leaf2.clone();
        leaves_b.extend_from_slice(&leaf1);

        let mut out_a = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut out_b = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut err: *mut c_char = ptr::null_mut();

        let ok_a = unsafe {
            sep_compute_merkle_root(
                pks_a.as_ptr(), pks_a.len(),
                leaves_a.as_ptr(), leaves_a.len(),
                5, &mut out_a, &mut err,
            )
        };
        let ok_b = unsafe {
            sep_compute_merkle_root(
                pks_b.as_ptr(), pks_b.len(),
                leaves_b.as_ptr(), leaves_b.len(),
                5, &mut out_b, &mut err,
            )
        };

        assert!(ok_a);
        assert!(ok_b);
        let root_a = read_buffer(&out_a);
        let root_b = read_buffer(&out_b);
        assert_eq!(root_a, root_b, "Merkle root must be order-independent");

        sep_byte_buffer_free(out_a);
        sep_byte_buffer_free(out_b);
    }

    #[test]
    fn test_ffi_compute_merkle_root_mismatched_counts() {
        let mut sk = [0u8; 32];
        sk[31] = 1;
        let pk = ffi_public_key(&sk);
        // Provide 2 leaf hashes but only 1 public key
        let leaf = ffi_leaf_hash(&sk);
        let mut double_leaf = leaf.clone();
        double_leaf.extend_from_slice(&leaf);

        let mut out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            sep_compute_merkle_root(
                pk.as_ptr(), pk.len(),
                double_leaf.as_ptr(), double_leaf.len(),
                5, &mut out, &mut err,
            )
        };
        assert!(!ok);
        assert!(!err.is_null());
        unsafe { sep_string_free(err) };
    }

    // ================================================================
    // FFI Commitment Tests
    // ================================================================

    #[test]
    fn test_ffi_sha256_commitment_32_bytes() {
        let mut root = [0u8; 32];
        root[31] = 1;
        let salt = [0u8; 32];
        let mut out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut err: *mut c_char = ptr::null_mut();

        let ok = unsafe {
            sep_compute_sha256_commitment(
                root.as_ptr(), root.len(),
                0, // epoch
                salt.as_ptr(), salt.len(),
                &mut out, &mut err,
            )
        };
        assert!(ok);
        let result = read_buffer(&out);
        assert_eq!(result.len(), 32);
        sep_byte_buffer_free(out);
    }

    #[test]
    fn test_ffi_poseidon_commitment_32_bytes() {
        let mut root = [0u8; 32];
        root[31] = 1;
        let salt = [0u8; 32];
        let mut out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut err: *mut c_char = ptr::null_mut();

        let ok = unsafe {
            sep_compute_poseidon_commitment(
                root.as_ptr(), root.len(),
                0,
                salt.as_ptr(), salt.len(),
                &mut out, &mut err,
            )
        };
        assert!(ok);
        let result = read_buffer(&out);
        assert_eq!(result.len(), 32);
        sep_byte_buffer_free(out);
    }

    #[test]
    fn test_ffi_poseidon_and_sha256_commitment_differ() {
        let mut root = [0u8; 32];
        root[31] = 1;
        let salt = [0u8; 32];

        let mut out_sha = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut out_pos = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut err: *mut c_char = ptr::null_mut();

        unsafe {
            sep_compute_sha256_commitment(
                root.as_ptr(), root.len(), 0,
                salt.as_ptr(), salt.len(),
                &mut out_sha, &mut err,
            );
            sep_compute_poseidon_commitment(
                root.as_ptr(), root.len(), 0,
                salt.as_ptr(), salt.len(),
                &mut out_pos, &mut err,
            );
        }

        let sha_result = read_buffer(&out_sha);
        let pos_result = read_buffer(&out_pos);
        assert_ne!(sha_result, pos_result, "SHA-256 and Poseidon commitments must differ");

        sep_byte_buffer_free(out_sha);
        sep_byte_buffer_free(out_pos);
    }

    #[test]
    fn test_ffi_commitment_deterministic() {
        let mut root = [0u8; 32];
        root[31] = 5;
        let salt = [0xAB; 32];

        let compute = || {
            let mut out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
            let mut err: *mut c_char = ptr::null_mut();
            unsafe {
                sep_compute_poseidon_commitment(
                    root.as_ptr(), root.len(), 42,
                    salt.as_ptr(), salt.len(),
                    &mut out, &mut err,
                );
            }
            let result = read_buffer(&out);
            sep_byte_buffer_free(out);
            result
        };
        assert_eq!(compute(), compute());
    }

    // ================================================================
    // FFI Nostr Tests (via extern "C")
    // ================================================================

    #[test]
    fn test_ffi_nostr_derive_public_key_32_bytes() {
        let mut sk = [0u8; 32];
        sk[31] = 1;
        let mut out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut err: *mut c_char = ptr::null_mut();

        let ok = unsafe {
            sep_nostr_derive_public_key(
                sk.as_ptr(), sk.len(), &mut out, &mut err,
            )
        };
        assert!(ok);
        let result = read_buffer(&out);
        assert_eq!(result.len(), 32);
        sep_byte_buffer_free(out);
    }

    #[test]
    fn test_ffi_nostr_sign_verify_roundtrip() {
        let mut sk = [0u8; 32];
        sk[31] = 3; // BIP-340 test vector 0 secret key
        let event_id: [u8; 32] = Sha256::digest(b"ffi-roundtrip-test").into();

        // Derive public key
        let mut pk_out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            sep_nostr_derive_public_key(sk.as_ptr(), sk.len(), &mut pk_out, &mut err)
        };
        assert!(ok);
        let pk = read_buffer(&pk_out);

        // Sign
        let mut sig_out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let ok = unsafe {
            sep_nostr_sign_event_id(
                sk.as_ptr(), sk.len(),
                event_id.as_ptr(), event_id.len(),
                &mut sig_out, &mut err,
            )
        };
        assert!(ok);
        let sig = read_buffer(&sig_out);
        assert_eq!(sig.len(), 64);

        // Verify
        let ok = unsafe {
            sep_nostr_verify_event_signature(
                pk.as_ptr(), pk.len(),
                event_id.as_ptr(), event_id.len(),
                sig.as_ptr(), sig.len(),
                &mut err,
            )
        };
        assert!(ok, "valid signature must verify");

        sep_byte_buffer_free(pk_out);
        sep_byte_buffer_free(sig_out);
    }

    #[test]
    fn test_ffi_nostr_verify_wrong_event_id_fails() {
        let mut sk = [0u8; 32];
        sk[31] = 3;
        let event_id: [u8; 32] = Sha256::digest(b"correct-event").into();
        let wrong_id: [u8; 32] = Sha256::digest(b"wrong-event").into();

        let mut pk_out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut sig_out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut err: *mut c_char = ptr::null_mut();

        unsafe {
            sep_nostr_derive_public_key(sk.as_ptr(), sk.len(), &mut pk_out, &mut err);
            sep_nostr_sign_event_id(
                sk.as_ptr(), sk.len(),
                event_id.as_ptr(), event_id.len(),
                &mut sig_out, &mut err,
            );
        }
        let pk = read_buffer(&pk_out);
        let sig = read_buffer(&sig_out);

        // Verify against wrong event ID
        let ok = unsafe {
            sep_nostr_verify_event_signature(
                pk.as_ptr(), pk.len(),
                wrong_id.as_ptr(), wrong_id.len(),
                sig.as_ptr(), sig.len(),
                &mut err,
            )
        };
        assert!(!ok, "signature must not verify against wrong event ID");

        sep_byte_buffer_free(pk_out);
        sep_byte_buffer_free(sig_out);
        if !err.is_null() {
            unsafe { sep_string_free(err) };
        }
    }

    // ================================================================
    // FFI Memory Safety Tests
    // ================================================================

    #[test]
    fn test_byte_buffer_free_null_safe() {
        let buf = SepByteBuffer {
            ptr: ptr::null_mut(),
            len: 0,
        };
        sep_byte_buffer_free(buf); // should not crash
    }

    #[test]
    fn test_string_free_null_safe() {
        unsafe { sep_string_free(ptr::null_mut()) }; // should not crash
    }

    // ================================================================
    // FFI Proof Pipeline Tests
    // ================================================================

    #[test]
    fn test_ffi_generate_testing_proving_key() {
        let mut out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            sep_generate_testing_proving_key(5, 12345, &mut out, &mut err)
        };
        assert!(ok);
        assert!(out.len > 0, "proving key must be non-empty");
        sep_byte_buffer_free(out);
    }

    #[test]
    fn test_ffi_generate_pk_deterministic() {
        let gen = |seed: u64| {
            let mut out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
            let mut err: *mut c_char = ptr::null_mut();
            unsafe { sep_generate_testing_proving_key(5, seed, &mut out, &mut err) };
            let bytes = read_buffer(&out);
            sep_byte_buffer_free(out);
            bytes
        };
        assert_eq!(gen(42), gen(42), "same seed must produce same PK");
        assert_ne!(gen(42), gen(43), "different seeds must produce different PKs");
    }

    #[test]
    fn test_ffi_full_proof_pipeline() {
        // 1. Generate proving key
        let mut pk_out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut err: *mut c_char = ptr::null_mut();
        let ok = unsafe {
            sep_generate_testing_proving_key(5, 99, &mut pk_out, &mut err)
        };
        assert!(ok, "generating PK failed");
        let pk_bytes = read_buffer(&pk_out);

        // 2. Create a single member
        let mut sk = [0u8; 32];
        sk[31] = 7;
        let member_pk = ffi_public_key(&sk);
        let member_leaf = ffi_leaf_hash(&sk);
        let salt = [0xAA; 32];

        // 3. Generate membership proof
        let mut proof_out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut commitment_out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let ok = unsafe {
            sep_generate_membership_proof(
                pk_bytes.as_ptr(), pk_bytes.len(),
                member_pk.as_ptr(), member_pk.len(),
                member_leaf.as_ptr(), member_leaf.len(),
                sk.as_ptr(), sk.len(),
                0, // epoch
                salt.as_ptr(), salt.len(),
                5, // depth
                &mut proof_out, &mut commitment_out, &mut err,
            )
        };
        assert!(ok, "generating proof failed");

        let proof = read_buffer(&proof_out);
        let commitment = read_buffer(&commitment_out);
        assert_eq!(proof.len(), 192, "compressed proof must be 192 bytes");
        assert_eq!(commitment.len(), 32, "commitment must be 32 bytes");

        // 4. Decompress proof
        let mut a_out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut b_out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let mut c_out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        let ok = unsafe {
            sep_proof_to_contract_format(
                proof.as_ptr(), proof.len(),
                &mut a_out, &mut b_out, &mut c_out, &mut err,
            )
        };
        assert!(ok, "proof decompression failed");

        let a = read_buffer(&a_out);
        let b = read_buffer(&b_out);
        let c = read_buffer(&c_out);
        assert_eq!(a.len(), 96, "proof_a (G1) must be 96 bytes");
        assert_eq!(b.len(), 192, "proof_b (G2) must be 192 bytes");
        assert_eq!(c.len(), 96, "proof_c (G1) must be 96 bytes");

        // 5. Verify commitment matches separate computation
        let mut root_out = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        unsafe {
            sep_compute_merkle_root(
                member_pk.as_ptr(), member_pk.len(),
                member_leaf.as_ptr(), member_leaf.len(),
                5,
                &mut root_out, &mut err,
            );
        }
        let root = read_buffer(&root_out);

        let mut commit_check = SepByteBuffer { ptr: ptr::null_mut(), len: 0 };
        unsafe {
            sep_compute_poseidon_commitment(
                root.as_ptr(), root.len(),
                0,
                salt.as_ptr(), salt.len(),
                &mut commit_check, &mut err,
            );
        }
        let commit_check_bytes = read_buffer(&commit_check);
        assert_eq!(
            commitment, commit_check_bytes,
            "proof commitment must match separately computed Poseidon commitment"
        );

        // Free all buffers
        sep_byte_buffer_free(pk_out);
        sep_byte_buffer_free(proof_out);
        sep_byte_buffer_free(commitment_out);
        sep_byte_buffer_free(a_out);
        sep_byte_buffer_free(b_out);
        sep_byte_buffer_free(c_out);
        sep_byte_buffer_free(root_out);
        sep_byte_buffer_free(commit_check);
    }
}
