package com.stellarmls.chat.crypto

import android.content.Context
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import com.stellarmls.chat.model.toHex
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

/**
 * Field-level AES-256-GCM encryption for persistent storage.
 * Root secret stored in EncryptedSharedPreferences, storage key
 * derived via HKDF-SHA256.
 */
object StorageEncryption {
    private const val PREFS_NAME = "stellar_storage_root"
    private const val ROOT_KEY_PREF = "root_secret"

    @Volatile
    private var storageKeySpec: SecretKeySpec? = null

    /**
     * Current derivation version. If the derivation path changes, increment this
     * and handle migration of existing encrypted data (M-16).
     */
    private const val DERIVATION_VERSION = 1

    /** Initialize with application context. Must be called before encrypt/decrypt.
     *  N-3: Synchronized to prevent race condition on concurrent first-launch init. */
    @Synchronized
    fun init(context: Context) {
        if (storageKeySpec != null) return
        val rootSecret = loadOrCreateRootSecret(context)
        // M-16: Version number included in HKDF info to enable future derivation changes.
        val storageKeyBytes = GroupCrypto.hkdf(
            ikm = rootSecret,
            salt = "com.stellarmls.chat.storage".toByteArray(),
            info = "local-storage-v${DERIVATION_VERSION}".toByteArray(),
            length = 32
        )
        storageKeySpec = SecretKeySpec(storageKeyBytes, "AES")
    }

    /** Encrypt bytes. Returns nonce(12) || ciphertext || tag(16). */
    fun encrypt(plaintext: ByteArray): ByteArray {
        val keySpec = storageKeySpec ?: throw IllegalStateException("StorageEncryption not initialized")
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, keySpec)
        val iv = cipher.iv // 12 bytes
        val ciphertextWithTag = cipher.doFinal(plaintext)
        // Combined: iv || ciphertext || tag (GCM appends tag to ciphertext)
        return iv + ciphertextWithTag
    }

    /** Encrypt a UTF-8 string. */
    fun encrypt(string: String): ByteArray = encrypt(string.toByteArray())

    /** Decrypt combined format: nonce(12) || ciphertext || tag(16). */
    fun decrypt(combined: ByteArray): ByteArray {
        val keySpec = storageKeySpec ?: throw IllegalStateException("StorageEncryption not initialized")
        val iv = combined.copyOfRange(0, 12)
        val ciphertextWithTag = combined.copyOfRange(12, combined.size)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, keySpec, GCMParameterSpec(128, iv))
        return cipher.doFinal(ciphertextWithTag)
    }

    /** Decrypt combined format and return as UTF-8 string. */
    fun decryptString(combined: ByteArray): String = String(decrypt(combined))

    private fun loadOrCreateRootSecret(context: Context): ByteArray {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        val prefs = EncryptedSharedPreferences.create(
            context,
            PREFS_NAME,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )
        val stored = prefs.getString(ROOT_KEY_PREF, null)
        return if (stored != null) {
            stored.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        } else {
            val secret = ByteArray(32)
            SecureRandom().nextBytes(secret)
            prefs.edit().putString(ROOT_KEY_PREF, secret.toHex()).apply()
            secret
        }
    }
}
