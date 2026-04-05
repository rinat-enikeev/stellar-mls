use std::fs;
use std::path::Path;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::Engine;
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

/// Load an existing X25519 keypair from disk, or generate a new one and save it.
pub fn load_or_create_keypair(data_dir: &Path) -> Result<(StaticSecret, PublicKey), String> {
    let key_path = data_dir.join("x25519_private.key");

    if key_path.exists() {
        let bytes = fs::read(&key_path)
            .map_err(|e| format!("failed to read X25519 private key: {e}"))?;
        if bytes.len() != 32 {
            return Err(format!(
                "X25519 private key file has invalid length: {} (expected 32)",
                bytes.len()
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        let secret = StaticSecret::from(arr);
        let public = PublicKey::from(&secret);
        eprintln!("Loaded X25519 keypair from {}", key_path.display());
        Ok((secret, public))
    } else {
        let mut rng = rand::thread_rng();
        let mut key_bytes = [0u8; 32];
        rng.fill_bytes(&mut key_bytes);
        let secret = StaticSecret::from(key_bytes);
        let public = PublicKey::from(&secret);

        fs::create_dir_all(data_dir)
            .map_err(|e| format!("failed to create data directory: {e}"))?;
        fs::write(&key_path, key_bytes)
            .map_err(|e| format!("failed to write X25519 private key: {e}"))?;
        eprintln!("Generated new X25519 keypair at {}", key_path.display());
        Ok((secret, public))
    }
}

/// Load or create a 32-byte storage key used to encrypt tokens and notification keys at rest.
pub fn load_or_create_storage_key(data_dir: &Path) -> Result<[u8; 32], String> {
    let key_path = data_dir.join("storage.key");

    if key_path.exists() {
        let bytes = fs::read(&key_path)
            .map_err(|e| format!("failed to read storage key: {e}"))?;
        if bytes.len() != 32 {
            return Err(format!(
                "storage key file has invalid length: {} (expected 32)",
                bytes.len()
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        eprintln!("Loaded storage key from {}", key_path.display());
        Ok(arr)
    } else {
        let mut rng = rand::thread_rng();
        let mut key = [0u8; 32];
        rng.fill_bytes(&mut key);

        fs::create_dir_all(data_dir)
            .map_err(|e| format!("failed to create data directory: {e}"))?;
        fs::write(&key_path, key)
            .map_err(|e| format!("failed to write storage key: {e}"))?;
        eprintln!("Generated new storage key at {}", key_path.display());
        Ok(key)
    }
}

/// Decrypt a registration envelope encrypted with the x25519-aes-256-gcm-v1 scheme.
///
/// The envelope JSON must contain: ephemeral_public_key, nonce, ciphertext, authentication_tag
/// (all base64-encoded).
pub fn decrypt_registration_envelope(
    envelope: &serde_json::Value,
    private_key: &StaticSecret,
) -> Result<serde_json::Value, String> {
    let b64 = base64::engine::general_purpose::STANDARD;

    let ephemeral_pub_b64 = envelope
        .get("ephemeral_public_key")
        .and_then(|v| v.as_str())
        .ok_or("missing ephemeral_public_key")?;
    let nonce_b64 = envelope
        .get("nonce")
        .and_then(|v| v.as_str())
        .ok_or("missing nonce")?;
    let ciphertext_b64 = envelope
        .get("ciphertext")
        .and_then(|v| v.as_str())
        .ok_or("missing ciphertext")?;
    let tag_b64 = envelope
        .get("authentication_tag")
        .and_then(|v| v.as_str())
        .ok_or("missing authentication_tag")?;

    let ephemeral_pub_bytes = b64
        .decode(ephemeral_pub_b64)
        .map_err(|e| format!("invalid base64 ephemeral_public_key: {e}"))?;
    let nonce_bytes = b64
        .decode(nonce_b64)
        .map_err(|e| format!("invalid base64 nonce: {e}"))?;
    let ciphertext_bytes = b64
        .decode(ciphertext_b64)
        .map_err(|e| format!("invalid base64 ciphertext: {e}"))?;
    let tag_bytes = b64
        .decode(tag_b64)
        .map_err(|e| format!("invalid base64 authentication_tag: {e}"))?;

    if ephemeral_pub_bytes.len() != 32 {
        return Err(format!(
            "ephemeral_public_key must be 32 bytes, got {}",
            ephemeral_pub_bytes.len()
        ));
    }
    if nonce_bytes.len() != 12 {
        return Err(format!(
            "nonce must be 12 bytes, got {}",
            nonce_bytes.len()
        ));
    }
    if tag_bytes.len() != 16 {
        return Err(format!(
            "authentication_tag must be 16 bytes, got {}",
            tag_bytes.len()
        ));
    }

    // Perform X25519 ECDH
    let mut ephemeral_pub_arr = [0u8; 32];
    ephemeral_pub_arr.copy_from_slice(&ephemeral_pub_bytes);
    let ephemeral_public = PublicKey::from(ephemeral_pub_arr);
    let shared_secret = private_key.diffie_hellman(&ephemeral_public);

    // Derive AES key using HKDF-SHA256
    let hkdf = Hkdf::<Sha256>::new(Some(b"sep-invitation-v1"), shared_secret.as_bytes());
    let mut aes_key = [0u8; 32];
    hkdf.expand(b"aes-256-gcm", &mut aes_key)
        .map_err(|e| format!("HKDF expand failed: {e}"))?;

    // Decrypt using AES-256-GCM
    let cipher = Aes256Gcm::new_from_slice(&aes_key)
        .map_err(|e| format!("failed to create AES cipher: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    // AES-GCM expects ciphertext || tag concatenated
    let mut combined = ciphertext_bytes;
    combined.extend_from_slice(&tag_bytes);

    let plaintext = cipher
        .decrypt(nonce, combined.as_ref())
        .map_err(|_| "decryption failed: invalid ciphertext or authentication tag".to_string())?;

    serde_json::from_slice(&plaintext)
        .map_err(|e| format!("decrypted content is not valid JSON: {e}"))
}

/// Encrypt event content under a subscription's notification key.
///
/// Returns (encrypted_ciphertext, nonce, tag). Uses event_id as AAD.
pub fn encrypt_notification(
    content: &[u8],
    notification_key: &[u8],
    event_id: &str,
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
    if notification_key.len() != 32 {
        return Err(format!(
            "notification_key must be 32 bytes, got {}",
            notification_key.len()
        ));
    }

    let cipher = Aes256Gcm::new_from_slice(notification_key)
        .map_err(|e| format!("failed to create AES cipher: {e}"))?;

    let mut rng = rand::thread_rng();
    let mut nonce_bytes = [0u8; 12];
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Use event_id as AAD
    use aes_gcm::aead::Payload;
    let payload = Payload {
        msg: content,
        aad: event_id.as_bytes(),
    };

    let encrypted = cipher
        .encrypt(nonce, payload)
        .map_err(|e| format!("encryption failed: {e}"))?;

    // AES-GCM output is ciphertext || tag (16 bytes)
    if encrypted.len() < 16 {
        return Err("encrypted output too short".to_string());
    }
    let tag_start = encrypted.len() - 16;
    let ciphertext = encrypted[..tag_start].to_vec();
    let tag = encrypted[tag_start..].to_vec();

    Ok((ciphertext, nonce_bytes.to_vec(), tag))
}

/// Encrypt plaintext for storage using AES-256-GCM.
///
/// Returns nonce(12) || ciphertext || tag(16).
pub fn encrypt_for_storage(plaintext: &[u8], storage_key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(storage_key)
        .map_err(|e| format!("failed to create AES cipher: {e}"))?;

    let mut rng = rand::thread_rng();
    let mut nonce_bytes = [0u8; 12];
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let encrypted = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("storage encryption failed: {e}"))?;

    let mut combined = Vec::with_capacity(12 + encrypted.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&encrypted);
    Ok(combined)
}

/// Decrypt data that was encrypted with `encrypt_for_storage`.
///
/// Input: nonce(12) || ciphertext || tag(16).
pub fn decrypt_from_storage(combined: &[u8], storage_key: &[u8; 32]) -> Result<Vec<u8>, String> {
    if combined.len() < 12 + 16 {
        return Err("encrypted data too short (must be at least 28 bytes)".to_string());
    }

    let nonce_bytes = &combined[..12];
    let encrypted = &combined[12..];

    let cipher = Aes256Gcm::new_from_slice(storage_key)
        .map_err(|e| format!("failed to create AES cipher: {e}"))?;
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, encrypted)
        .map_err(|_| "storage decryption failed".to_string())
}
