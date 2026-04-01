package com.stellarmls.chat.crypto

import com.stellarmls.chat.model.toHex
import org.bouncycastle.crypto.agreement.X25519Agreement
import org.bouncycastle.crypto.params.X25519PrivateKeyParameters
import org.bouncycastle.crypto.params.X25519PublicKeyParameters
import org.json.JSONObject
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.Mac
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

// M-7: This file uses org.json.JSONObject for JSON serialization, which is untyped
// and error-prone. A future migration to kotlinx.serialization with @Serializable
// data classes would provide compile-time type safety and eliminate runtime parsing
// errors from missing or mistyped fields. See audit finding M-7 for details.
object GroupCrypto {
    /**
     * Derive hidden group topic (SEP-XXXX §3.1).
     * Canonical derivation: `topicTag = hex(SHA-256("sep-topic-v1" || groupSecret)[0..8])`
     * Result: 16-character hex string used as the `t` tag value in Nostr kind 24114 events.
     * This derivation MUST be identical on all platforms for cross-platform interoperability.
     */
    fun hiddenGroupTopic(groupSecret: ByteArray): String {
        val digest = MessageDigest.getInstance("SHA-256")
        digest.update("sep-topic-v1".toByteArray())
        digest.update(groupSecret)
        return digest.digest().take(8).toByteArray().toHex()
    }

    /** Derive hidden inbox tag for a recipient. */
    fun hiddenInboxTag(recipientPublicKey: ByteArray): String {
        val digest = MessageDigest.getInstance("SHA-256")
        digest.update("sep-inbox-v1".toByteArray())
        digest.update(recipientPublicKey)
        return digest.digest().take(8).toByteArray().toHex()
    }

    /** Derive AES-256-GCM key bound to current epoch and salt via HKDF-SHA256.
     *  Key rotates on every membership change, ensuring removed members
     *  cannot decrypt messages sent after their removal. */
    fun deriveMessageKey(groupSecret: ByteArray, epoch: Long, salt: ByteArray): ByteArray {
        // IKM = groupSecret || epoch (big-endian) || salt
        val epochBytes = ByteArray(8)
        for (i in 0 until 8) epochBytes[i] = (epoch shr (56 - i * 8) and 0xFF).toByte()
        val ikm = groupSecret + epochBytes + salt
        return hkdf(ikm, "sep-msg-key-v1".toByteArray(), "traffic".toByteArray(), 32)
    }

    // -- Group message encryption (AES-256-GCM, kind 24114) --

    /** Encrypt plaintext using AES-256-GCM. Returns a sealed envelope JSON string. */
    fun encrypt(plaintext: String, key: ByteArray): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        val keySpec = SecretKeySpec(key, "AES")
        cipher.init(Cipher.ENCRYPT_MODE, keySpec)
        val iv = cipher.iv // 12 bytes generated automatically
        val ciphertext = cipher.doFinal(plaintext.toByteArray())

        // GCM appends auth tag to ciphertext; split them
        val tagLen = 16
        val ct = ciphertext.copyOfRange(0, ciphertext.size - tagLen)
        val tag = ciphertext.copyOfRange(ciphertext.size - tagLen, ciphertext.size)

        val envelope = JSONObject().apply {
            put("version", 1)
            put("scheme", "aes-256-gcm-v1")
            put("ephemeral_public_key", JSONObject.NULL)
            put("nonce", android.util.Base64.encodeToString(iv, android.util.Base64.NO_WRAP))
            put("ciphertext", android.util.Base64.encodeToString(ct, android.util.Base64.NO_WRAP))
            put("authentication_tag", android.util.Base64.encodeToString(tag, android.util.Base64.NO_WRAP))
        }
        return envelope.toString()
    }

    /** Decrypt a sealed envelope JSON string using AES-256-GCM. */
    fun decrypt(envelopeJson: String, key: ByteArray): String {
        val envelope = JSONObject(envelopeJson)
        require(envelope.getString("scheme") == "aes-256-gcm-v1")

        val nonce = android.util.Base64.decode(envelope.getString("nonce"), android.util.Base64.NO_WRAP)
        val ct = android.util.Base64.decode(envelope.getString("ciphertext"), android.util.Base64.NO_WRAP)
        val tag = android.util.Base64.decode(envelope.getString("authentication_tag"), android.util.Base64.NO_WRAP)

        // GCM expects ciphertext + tag concatenated
        val combined = ct + tag

        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        val keySpec = SecretKeySpec(key, "AES")
        cipher.init(Cipher.DECRYPT_MODE, keySpec, GCMParameterSpec(128, nonce))
        return String(cipher.doFinal(combined))
    }

    // -- Invitation encryption (X25519 ECDH + AES-256-GCM, kind 24113) --

    /**
     * Encrypt an invitation payload using ephemeral X25519 ECDH + AES-256-GCM.
     * Returns a sealed envelope JSON string with scheme "x25519-aes-256-gcm-v1".
     *
     * M-5: The ephemeral public key is signed with the sender's Ed25519 identity key
     * to prevent MITM substitution of the ephemeral key.
     */
    fun encryptInvitation(
        payload: ByteArray,
        recipientKeyAgreementPubkey: ByteArray,
        senderKeyManager: KeyManager? = null
    ): String {
        // Generate ephemeral X25519 key pair
        val ephemeralPrivate = X25519PrivateKeyParameters(SecureRandom())
        val ephemeralPublic = ephemeralPrivate.generatePublicKey()

        // ECDH shared secret
        val recipientPublicParams = X25519PublicKeyParameters(recipientKeyAgreementPubkey, 0)
        val agreement = X25519Agreement()
        agreement.init(ephemeralPrivate)
        val sharedSecret = ByteArray(agreement.agreementSize)
        agreement.calculateAgreement(recipientPublicParams, sharedSecret, 0)

        // Derive encryption key via HKDF
        val encKey = hkdf(
            ikm = sharedSecret,
            salt = "sep-invitation-v1".toByteArray(),
            info = "aes-256-gcm".toByteArray(),
            length = 32
        )

        // Encrypt with AES-256-GCM
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(encKey, "AES"))
        val iv = cipher.iv
        val ciphertext = cipher.doFinal(payload)

        val tagLen = 16
        val ct = ciphertext.copyOfRange(0, ciphertext.size - tagLen)
        val tag = ciphertext.copyOfRange(ciphertext.size - tagLen, ciphertext.size)

        // M-5: Sign ephemeral public key with sender's Ed25519 identity key
        val ephKeySignature = senderKeyManager?.stellarSign(ephemeralPublic.encoded)

        val envelope = JSONObject().apply {
            put("version", 1)
            put("scheme", "x25519-aes-256-gcm-v1")
            put("ephemeral_public_key", android.util.Base64.encodeToString(ephemeralPublic.encoded, android.util.Base64.NO_WRAP))
            if (ephKeySignature != null) {
                put("ephemeral_key_signature", android.util.Base64.encodeToString(ephKeySignature, android.util.Base64.NO_WRAP))
            }
            put("nonce", android.util.Base64.encodeToString(iv, android.util.Base64.NO_WRAP))
            put("ciphertext", android.util.Base64.encodeToString(ct, android.util.Base64.NO_WRAP))
            put("authentication_tag", android.util.Base64.encodeToString(tag, android.util.Base64.NO_WRAP))
        }
        return envelope.toString()
    }

    /**
     * Decrypt an invitation sealed envelope using the recipient's X25519 private key.
     * N-1: If `senderEd25519Pubkey` is provided and the envelope has an ephemeral key signature,
     * verifies the signature to prevent MITM substitution.
     */
    fun decryptInvitation(
        envelopeJson: String,
        privateKey: X25519PrivateKeyParameters,
        senderEd25519Pubkey: ByteArray? = null
    ): ByteArray {
        val envelope = JSONObject(envelopeJson)
        require(envelope.getString("scheme") == "x25519-aes-256-gcm-v1")

        val ephemeralPubBytes = android.util.Base64.decode(envelope.getString("ephemeral_public_key"), android.util.Base64.NO_WRAP)
        val nonce = android.util.Base64.decode(envelope.getString("nonce"), android.util.Base64.NO_WRAP)
        val ct = android.util.Base64.decode(envelope.getString("ciphertext"), android.util.Base64.NO_WRAP)
        val tag = android.util.Base64.decode(envelope.getString("authentication_tag"), android.util.Base64.NO_WRAP)

        // N-1: Verify ephemeral key signature if present
        if (envelope.has("ephemeral_key_signature")) {
            val sigBytes = android.util.Base64.decode(
                envelope.getString("ephemeral_key_signature"), android.util.Base64.NO_WRAP)
            requireNotNull(senderEd25519Pubkey) {
                "Ephemeral key signature present but no sender pubkey to verify against"
            }
            val verifier = org.bouncycastle.crypto.signers.Ed25519Signer()
            verifier.init(false, org.bouncycastle.crypto.params.Ed25519PublicKeyParameters(senderEd25519Pubkey, 0))
            verifier.update(ephemeralPubBytes, 0, ephemeralPubBytes.size)
            require(verifier.verifySignature(sigBytes)) {
                "Ephemeral key signature verification failed"
            }
        }

        // ECDH shared secret
        val ephemeralPubParams = X25519PublicKeyParameters(ephemeralPubBytes, 0)
        val agreement = X25519Agreement()
        agreement.init(privateKey)
        val sharedSecret = ByteArray(agreement.agreementSize)
        agreement.calculateAgreement(ephemeralPubParams, sharedSecret, 0)

        // Derive decryption key via HKDF
        val decKey = hkdf(
            ikm = sharedSecret,
            salt = "sep-invitation-v1".toByteArray(),
            info = "aes-256-gcm".toByteArray(),
            length = 32
        )

        // Decrypt with AES-256-GCM
        val combined = ct + tag
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(decKey, "AES"), GCMParameterSpec(128, nonce))
        return cipher.doFinal(combined)
    }

    /** HKDF-SHA256 (extract + expand). */
    fun hkdf(ikm: ByteArray, salt: ByteArray, info: ByteArray, length: Int): ByteArray {
        // Extract
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(salt, "HmacSHA256"))
        val prk = mac.doFinal(ikm)

        // Expand
        val expandMac = Mac.getInstance("HmacSHA256")
        expandMac.init(SecretKeySpec(prk, "HmacSHA256"))
        expandMac.update(info)
        expandMac.update(byteArrayOf(1))
        return expandMac.doFinal().copyOfRange(0, length)
    }
}
