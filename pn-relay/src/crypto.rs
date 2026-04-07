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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = PathBuf::from(format!(
            "/tmp/pn_relay_crypto_test_{}_{}_{id}",
            std::process::id(),
            prefix
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ---------------------------------------------------------------
    // Key management
    // ---------------------------------------------------------------

    #[test]
    fn test_load_or_create_keypair_generates_new() {
        let dir = unique_temp_dir("keypair_new");
        let result = load_or_create_keypair(&dir);
        assert!(result.is_ok());
        let (secret, public) = result.unwrap();
        // Public key should be derivable from the secret
        assert_eq!(PublicKey::from(&secret).as_bytes(), public.as_bytes());
        // Key file should exist
        assert!(dir.join("x25519_private.key").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_or_create_keypair_loads_existing() {
        let dir = unique_temp_dir("keypair_existing");
        let (secret1, public1) = load_or_create_keypair(&dir).unwrap();
        let (secret2, public2) = load_or_create_keypair(&dir).unwrap();
        // Same keypair should be returned on second call
        assert_eq!(public1.as_bytes(), public2.as_bytes());
        // StaticSecret doesn't implement PartialEq, so compare via derived public keys
        assert_eq!(
            PublicKey::from(&secret1).as_bytes(),
            PublicKey::from(&secret2).as_bytes()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_or_create_storage_key_generates_new() {
        let dir = unique_temp_dir("storagekey_new");
        let result = load_or_create_storage_key(&dir);
        assert!(result.is_ok());
        let key = result.unwrap();
        assert_eq!(key.len(), 32);
        assert!(dir.join("storage.key").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_or_create_storage_key_loads_existing() {
        let dir = unique_temp_dir("storagekey_existing");
        let key1 = load_or_create_storage_key(&dir).unwrap();
        let key2 = load_or_create_storage_key(&dir).unwrap();
        assert_eq!(key1, key2);
        std::fs::remove_dir_all(&dir).ok();
    }

    // ---------------------------------------------------------------
    // Storage encryption
    // ---------------------------------------------------------------

    #[test]
    fn test_storage_encrypt_decrypt_roundtrip() {
        let key = [42u8; 32];
        let plaintext = b"hello world storage encryption";
        let encrypted = encrypt_for_storage(plaintext, &key).unwrap();
        let decrypted = decrypt_from_storage(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_storage_decrypt_wrong_key_fails() {
        let key_a = [1u8; 32];
        let key_b = [2u8; 32];
        let plaintext = b"secret data";
        let encrypted = encrypt_for_storage(plaintext, &key_a).unwrap();
        let result = decrypt_from_storage(&encrypted, &key_b);
        assert!(result.is_err());
    }

    #[test]
    fn test_storage_decrypt_too_short_input() {
        let key = [0u8; 32];
        // 27 bytes is less than the minimum 28 (12 nonce + 16 tag)
        let short_data = vec![0u8; 27];
        let result = decrypt_from_storage(&short_data, &key);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too short"));
    }

    #[test]
    fn test_storage_encrypt_output_format() {
        let key = [7u8; 32];
        let plaintext = b"test data here";
        let encrypted = encrypt_for_storage(plaintext, &key).unwrap();
        // Output format: nonce(12) || ciphertext(same len as plaintext) || tag(16)
        assert_eq!(encrypted.len(), 12 + plaintext.len() + 16);
    }

    // ---------------------------------------------------------------
    // Notification encryption
    // ---------------------------------------------------------------

    #[test]
    fn test_encrypt_notification_roundtrip() {
        let key = [99u8; 32];
        let content = b"notification payload";
        let event_id = "event123";

        let (ciphertext, nonce, tag) = encrypt_notification(content, &key, event_id).unwrap();

        // Manually decrypt to verify
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let nonce_obj = Nonce::from_slice(&nonce);
        let mut combined = ciphertext;
        combined.extend_from_slice(&tag);

        use aes_gcm::aead::Payload;
        let payload = Payload {
            msg: combined.as_ref(),
            aad: event_id.as_bytes(),
        };
        let decrypted = cipher.decrypt(nonce_obj, payload).unwrap();
        assert_eq!(decrypted, content);
    }

    #[test]
    fn test_encrypt_notification_different_nonces() {
        let key = [88u8; 32];
        let content = b"same content";
        let event_id = "same_event";

        let (_, nonce1, _) = encrypt_notification(content, &key, event_id).unwrap();
        let (_, nonce2, _) = encrypt_notification(content, &key, event_id).unwrap();

        // Random nonces should differ (probability of collision is negligible)
        assert_ne!(nonce1, nonce2);
    }

    #[test]
    fn test_encrypt_notification_wrong_key_fails() {
        let key_a = [10u8; 32];
        let key_b = [20u8; 32];
        let content = b"secret notification";
        let event_id = "evt1";

        let (ciphertext, nonce, tag) = encrypt_notification(content, &key_a, event_id).unwrap();

        // Try to decrypt with wrong key
        let cipher = Aes256Gcm::new_from_slice(&key_b).unwrap();
        let nonce_obj = Nonce::from_slice(&nonce);
        let mut combined = ciphertext;
        combined.extend_from_slice(&tag);

        use aes_gcm::aead::Payload;
        let payload = Payload {
            msg: combined.as_ref(),
            aad: event_id.as_bytes(),
        };
        let result = cipher.decrypt(nonce_obj, payload);
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypt_notification_uses_event_id_as_aad() {
        let key = [55u8; 32];
        let content = b"same content for both";

        let (_, _, _tag1) = encrypt_notification(content, &key, "event_a").unwrap();
        let (_, _, _tag2) = encrypt_notification(content, &key, "event_b").unwrap();

        // Different event IDs (AAD) produce different tags even with same content
        // (Nonces also differ, but the AAD difference guarantees auth tag difference)
        // We mainly verify that decryption with wrong AAD fails
        let (ciphertext, nonce, tag) = encrypt_notification(content, &key, "event_correct").unwrap();

        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let nonce_obj = Nonce::from_slice(&nonce);
        let mut combined = ciphertext;
        combined.extend_from_slice(&tag);

        use aes_gcm::aead::Payload;
        let wrong_aad_payload = Payload {
            msg: combined.as_ref(),
            aad: b"event_wrong",
        };
        let result = cipher.decrypt(nonce_obj, wrong_aad_payload);
        assert!(result.is_err(), "decryption with wrong AAD should fail");
    }

    // ---------------------------------------------------------------
    // Registration envelope (X25519 ECDH)
    // ---------------------------------------------------------------

    /// Helper: construct a valid encrypted envelope for testing.
    fn build_test_envelope(
        server_public: &PublicKey,
        payload: &serde_json::Value,
    ) -> (StaticSecret, serde_json::Value) {
        let b64 = base64::engine::general_purpose::STANDARD;

        // 1. Generate ephemeral X25519 keypair
        let mut rng = rand::thread_rng();
        let mut eph_bytes = [0u8; 32];
        rng.fill_bytes(&mut eph_bytes);
        let ephemeral_secret = StaticSecret::from(eph_bytes);
        let ephemeral_public = PublicKey::from(&ephemeral_secret);

        // 2. Compute shared secret
        let shared_secret = ephemeral_secret.diffie_hellman(server_public);

        // 3. Derive AES key via HKDF-SHA256
        let hkdf = Hkdf::<Sha256>::new(Some(b"sep-invitation-v1"), shared_secret.as_bytes());
        let mut aes_key = [0u8; 32];
        hkdf.expand(b"aes-256-gcm", &mut aes_key).unwrap();

        // 4. Encrypt payload with AES-256-GCM
        let cipher = Aes256Gcm::new_from_slice(&aes_key).unwrap();
        let mut nonce_bytes = [0u8; 12];
        rng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let plaintext = serde_json::to_vec(payload).unwrap();
        let encrypted = cipher.encrypt(nonce, plaintext.as_ref()).unwrap();

        // Split into ciphertext and tag
        let tag_start = encrypted.len() - 16;
        let ciphertext = &encrypted[..tag_start];
        let tag = &encrypted[tag_start..];

        // 5. Build envelope JSON
        let envelope = serde_json::json!({
            "ephemeral_public_key": b64.encode(ephemeral_public.as_bytes()),
            "nonce": b64.encode(&nonce_bytes),
            "ciphertext": b64.encode(ciphertext),
            "authentication_tag": b64.encode(tag),
        });

        (ephemeral_secret, envelope)
    }

    #[test]
    fn test_decrypt_registration_envelope_roundtrip() {
        let dir = unique_temp_dir("envelope_rt");
        let (server_secret, server_public) = load_or_create_keypair(&dir).unwrap();

        let inner_payload = serde_json::json!({
            "action": "subscribe",
            "subscription_id": "test-123",
            "platform": "ios",
            "token": "device-token-abc",
        });

        let (_, envelope) = build_test_envelope(&server_public, &inner_payload);

        let decrypted = decrypt_registration_envelope(&envelope, &server_secret).unwrap();
        assert_eq!(decrypted["action"], "subscribe");
        assert_eq!(decrypted["subscription_id"], "test-123");
        assert_eq!(decrypted["platform"], "ios");
        assert_eq!(decrypted["token"], "device-token-abc");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_decrypt_envelope_wrong_key_fails() {
        let dir_a = unique_temp_dir("envelope_wrong_a");
        let dir_b = unique_temp_dir("envelope_wrong_b");
        let (_secret_a, public_a) = load_or_create_keypair(&dir_a).unwrap();
        let (secret_b, _public_b) = load_or_create_keypair(&dir_b).unwrap();

        let payload = serde_json::json!({"action": "subscribe"});
        let (_, envelope) = build_test_envelope(&public_a, &payload);

        // Decrypt with wrong server key
        let result = decrypt_registration_envelope(&envelope, &secret_b);
        assert!(result.is_err());

        std::fs::remove_dir_all(&dir_a).ok();
        std::fs::remove_dir_all(&dir_b).ok();
    }

    #[test]
    fn test_decrypt_envelope_missing_fields() {
        let dir = unique_temp_dir("envelope_missing");
        let (server_secret, _) = load_or_create_keypair(&dir).unwrap();

        // Missing ephemeral_public_key
        let envelope = serde_json::json!({
            "nonce": "AAAAAAAAAAAAAAAA",
            "ciphertext": "AAAA",
            "authentication_tag": "AAAAAAAAAAAAAAAAAAAAAA==",
        });
        let result = decrypt_registration_envelope(&envelope, &server_secret);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ephemeral_public_key"));

        // Missing nonce
        let envelope2 = serde_json::json!({
            "ephemeral_public_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "ciphertext": "AAAA",
            "authentication_tag": "AAAAAAAAAAAAAAAAAAAAAA==",
        });
        let result2 = decrypt_registration_envelope(&envelope2, &server_secret);
        assert!(result2.is_err());
        assert!(result2.unwrap_err().contains("nonce"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
