package com.stellarmls.chat.crypto

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.MediaMetadataRetriever
import android.net.Uri
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
    /** Maximum video size in bytes before encryption (50 MB). */
    const val MAX_VIDEO_BYTES = 50_000_000
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

    // -- Video Processing --

    /**
     * Extract video metadata: width, height, duration (seconds).
     */
    fun videoMetadata(context: Context, uri: Uri): Triple<Int, Int, Int>? {
        val retriever = MediaMetadataRetriever()
        return try {
            retriever.setDataSource(context, uri)
            val width = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH)?.toIntOrNull() ?: 0
            val height = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT)?.toIntOrNull() ?: 0
            val durationMs = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull() ?: 0
            val rotation = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_ROTATION)?.toIntOrNull() ?: 0
            // Swap width/height for 90/270 rotation
            val (w, h) = if (rotation == 90 || rotation == 270) Pair(height, width) else Pair(width, height)
            Triple(w, h, (durationMs / 1000).toInt())
        } catch (_: Exception) {
            null
        } finally {
            retriever.release()
        }
    }

    /**
     * Generate a thumbnail from the first frame of a video.
     */
    fun generateVideoThumbnail(
        context: Context,
        uri: Uri,
        maxDimension: Int = THUMBNAIL_MAX_DIMENSION
    ): ByteArray? {
        val retriever = MediaMetadataRetriever()
        return try {
            retriever.setDataSource(context, uri)
            val frame = retriever.getFrameAtTime(0, MediaMetadataRetriever.OPTION_CLOSEST_SYNC)
                ?: return null
            val scale: Float = if (frame.width > frame.height) {
                maxDimension.toFloat() / frame.width
            } else {
                maxDimension.toFloat() / frame.height
            }
            val newWidth = (frame.width * scale).toInt().coerceAtLeast(1)
            val newHeight = (frame.height * scale).toInt().coerceAtLeast(1)
            val thumbnail = Bitmap.createScaledBitmap(frame, newWidth, newHeight, true)
            val output = ByteArrayOutputStream()
            thumbnail.compress(Bitmap.CompressFormat.JPEG, THUMBNAIL_QUALITY, output)
            if (thumbnail != frame) thumbnail.recycle()
            frame.recycle()
            output.toByteArray()
        } catch (_: Exception) {
            null
        } finally {
            retriever.release()
        }
    }
}
