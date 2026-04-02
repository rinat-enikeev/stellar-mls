package com.stellarmls.chat.crypto

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import java.io.ByteArrayOutputStream
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

/**
 * Per-file AES-256-GCM encryption for media attachments.
 * Each file gets a fresh random key to avoid reusing the group key
 * for large binary payloads, minimizing AES-GCM nonce-collision risk.
 */
object MediaCrypto {
    /** Maximum compressed image size in bytes (2 MB). */
    const val MAX_IMAGE_BYTES = 2_000_000
    /** Maximum thumbnail dimension in pixels. */
    const val THUMBNAIL_MAX_DIMENSION = 200
    /** Thumbnail JPEG compression quality (0-100). */
    const val THUMBNAIL_QUALITY = 40

    private const val NONCE_SIZE = 12
    private const val TAG_SIZE = 16

    // -- Encrypt / Decrypt --

    /**
     * Encrypt data with a fresh random AES-256-GCM key.
     * Returns Pair(encryptedBlob, key) where encryptedBlob = nonce(12) || ciphertext || tag(16).
     */
    fun encryptMedia(data: ByteArray): Pair<ByteArray, ByteArray> {
        val key = ByteArray(32)
        SecureRandom().nextBytes(key)
        val encrypted = encryptMedia(data, key)
        return Pair(encrypted, key)
    }

    /**
     * Encrypt data with a provided AES-256-GCM key.
     * Returns combined format: nonce(12) || ciphertext || tag(16).
     */
    fun encryptMedia(data: ByteArray, key: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        val keySpec = SecretKeySpec(key, "AES")
        cipher.init(Cipher.ENCRYPT_MODE, keySpec)
        val nonce = cipher.iv // 12 bytes generated automatically
        val ciphertextWithTag = cipher.doFinal(data)

        // Combined format: nonce || ciphertext || tag
        // Java GCM appends tag to ciphertext, so ciphertextWithTag = ct + tag already
        return nonce + ciphertextWithTag
    }

    /**
     * Decrypt combined-format blob: nonce(12) || ciphertext || tag(16).
     */
    fun decryptMedia(encryptedBlob: ByteArray, key: ByteArray): ByteArray {
        require(encryptedBlob.size > NONCE_SIZE + TAG_SIZE) { "Encrypted blob too short" }

        val nonce = encryptedBlob.copyOfRange(0, NONCE_SIZE)
        val ciphertextWithTag = encryptedBlob.copyOfRange(NONCE_SIZE, encryptedBlob.size)

        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        val keySpec = SecretKeySpec(key, "AES")
        val gcmSpec = GCMParameterSpec(TAG_SIZE * 8, nonce)
        cipher.init(Cipher.DECRYPT_MODE, keySpec, gcmSpec)
        return cipher.doFinal(ciphertextWithTag)
    }

    // -- Image Processing --

    /**
     * Compress an image to JPEG within [maxBytes].
     * Progressively reduces quality until the size limit is met.
     */
    fun compressImage(imageData: ByteArray, maxBytes: Int = MAX_IMAGE_BYTES): ByteArray? {
        val bitmap = BitmapFactory.decodeByteArray(imageData, 0, imageData.size) ?: return null
        var quality = 70
        while (quality >= 10) {
            val output = ByteArrayOutputStream()
            bitmap.compress(Bitmap.CompressFormat.JPEG, quality, output)
            val result = output.toByteArray()
            if (result.size <= maxBytes) {
                bitmap.recycle()
                return result
            }
            quality -= 10
        }
        // Last resort: lowest quality
        val output = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.JPEG, 10, output)
        bitmap.recycle()
        return output.toByteArray()
    }

    /**
     * Generate a small JPEG thumbnail.
     */
    fun generateThumbnail(
        imageData: ByteArray,
        maxDimension: Int = THUMBNAIL_MAX_DIMENSION
    ): ByteArray? {
        val bitmap = BitmapFactory.decodeByteArray(imageData, 0, imageData.size) ?: return null
        val scale: Float = if (bitmap.width > bitmap.height) {
            maxDimension.toFloat() / bitmap.width
        } else {
            maxDimension.toFloat() / bitmap.height
        }
        val newWidth = (bitmap.width * scale).toInt().coerceAtLeast(1)
        val newHeight = (bitmap.height * scale).toInt().coerceAtLeast(1)
        val thumbnail = Bitmap.createScaledBitmap(bitmap, newWidth, newHeight, true)
        val output = ByteArrayOutputStream()
        thumbnail.compress(Bitmap.CompressFormat.JPEG, THUMBNAIL_QUALITY, output)
        if (thumbnail != bitmap) thumbnail.recycle()
        bitmap.recycle()
        return output.toByteArray()
    }

    /**
     * Get pixel dimensions of an image.
     */
    fun imageDimensions(imageData: ByteArray): Pair<Int, Int>? {
        val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(imageData, 0, imageData.size, options)
        if (options.outWidth <= 0 || options.outHeight <= 0) return null
        return Pair(options.outWidth, options.outHeight)
    }
}
