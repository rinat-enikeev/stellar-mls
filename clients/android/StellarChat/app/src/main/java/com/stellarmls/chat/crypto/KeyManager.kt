package com.stellarmls.chat.crypto

import android.content.Context
import com.stellarmls.chat.model.toHex
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

class KeyManager(context: Context) {
    val secretKey: ByteArray
    val publicKey: ByteArray
    val publicKeyHex: String

    init {
        val prefs = context.getSharedPreferences("stellar_keys", Context.MODE_PRIVATE)
        val stored = prefs.getString("nostr_secret_key", null)

        secretKey = if (stored != null) {
            stored.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        } else {
            val key = ByteArray(32)
            SecureRandom().nextBytes(key)
            prefs.edit().putString("nostr_secret_key", key.toHex()).apply()
            key
        }

        publicKey = derivePublicKey(secretKey)
        publicKeyHex = publicKey.toHex()
    }

    /** Sign a 32-byte event ID. Returns 64-byte signature. */
    fun signEventID(eventID: ByteArray): ByteArray {
        // Simplified HMAC-based signature for demo.
        // Production would use secp256k1 Schnorr via fr.acinq.secp256k1.
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(secretKey, "HmacSHA256"))
        val hmac = mac.doFinal(eventID)
        return hmac + ByteArray(32) // pad to 64 bytes
    }

    companion object {
        /** Derive public key from secret key (SHA-256 for demo). */
        private fun derivePublicKey(secretKey: ByteArray): ByteArray {
            return MessageDigest.getInstance("SHA-256").digest(secretKey)
        }
    }
}
