package com.stellarmls.chat.crypto

import com.stellarmls.chat.model.toHex
import org.json.JSONObject
import java.security.MessageDigest
import javax.crypto.Cipher
import javax.crypto.Mac
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

object GroupCrypto {
    /** Derive hidden group topic: first 16 hex chars of SHA256("sep-topic-v1" || groupSecret). */
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

    /** Derive AES-256-GCM key from group secret via HKDF-SHA256. */
    fun deriveMessageKey(groupSecret: ByteArray): ByteArray {
        val salt = "sep-msg-key-v1".toByteArray()
        val info = "traffic".toByteArray()
        return hkdf(groupSecret, salt, info, 32)
    }

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

    /** Simple HKDF-SHA256 (extract + expand). */
    private fun hkdf(ikm: ByteArray, salt: ByteArray, info: ByteArray, length: Int): ByteArray {
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
