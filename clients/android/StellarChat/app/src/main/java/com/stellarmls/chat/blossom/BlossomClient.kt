package com.stellarmls.chat.blossom

import com.stellarmls.chat.crypto.KeyManager
import com.stellarmls.chat.crypto.NostrEventBuilder
import com.stellarmls.chat.model.toHex
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.security.MessageDigest
import java.util.concurrent.TimeUnit

/**
 * Client for Blossom (BL-01) content-addressable blob storage.
 * Files are addressed by SHA-256 hash of their content.
 * Upload authorization uses Nostr-signed kind 24242 events.
 */
object BlossomClient {
    private val client = OkHttpClient.Builder()
        .connectTimeout(30, TimeUnit.SECONDS)
        .writeTimeout(60, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()

    /**
     * Upload an encrypted blob to Blossom server(s).
     * Returns the SHA-256 hex hash of the uploaded blob.
     */
    suspend fun upload(
        data: ByteArray,
        servers: List<String>,
        keyManager: KeyManager
    ): String {
        val hash = MessageDigest.getInstance("SHA-256").digest(data)
        val blobHash = hash.toHex()

        val authEvent = buildAuthEvent("upload", blobHash, keyManager)
        val authJson = authEvent.toJson().toString()
        val authToken = "Nostr ${android.util.Base64.encodeToString(
            authJson.toByteArray(), android.util.Base64.NO_WRAP)}"

        var lastError: Exception? = null
        for (server in servers) {
            try {
                val url = "${server.trimEnd('/')}/upload"
                val request = Request.Builder()
                    .url(url)
                    .put(data.toRequestBody("application/octet-stream".toMediaType()))
                    .header("Authorization", authToken)
                    .build()

                client.newCall(request).execute().use { response ->
                    if (response.isSuccessful) {
                        return blobHash
                    }
                    val reason = response.header("x-reason") ?: ""
                    val body = response.body?.string() ?: ""
                    val detail = reason.ifEmpty { body }
                    lastError = RuntimeException(
                        "Blossom upload to $server failed: HTTP ${response.code} $detail"
                    )
                }
            } catch (e: Exception) {
                lastError = e
            }
        }

        throw lastError ?: RuntimeException("Failed to upload to any Blossom server")
    }

    /**
     * Download an encrypted blob from Blossom server(s) by its SHA-256 hash.
     */
    suspend fun download(
        blobHash: String,
        servers: List<String>
    ): ByteArray {
        var lastError: Exception? = null
        for (server in servers) {
            try {
                val url = "${server.trimEnd('/')}/$blobHash"
                val request = Request.Builder().url(url).get().build()

                client.newCall(request).execute().use { response ->
                    if (response.isSuccessful) {
                        val data = response.body?.bytes()
                            ?: throw RuntimeException("Empty response body")
                        // Verify content hash matches
                        val downloadHash = MessageDigest.getInstance("SHA-256").digest(data)
                        val downloadHashHex = downloadHash.toHex()
                        if (downloadHashHex != blobHash) {
                            throw RuntimeException("Downloaded blob hash mismatch")
                        }
                        return data
                    }
                }
            } catch (e: Exception) {
                lastError = e
            }
        }

        throw lastError ?: RuntimeException("Failed to download from any Blossom server")
    }

    /**
     * Build a BL-01 authorization event (kind 24242).
     */
    private fun buildAuthEvent(
        action: String,
        blobHash: String,
        keyManager: KeyManager
    ): com.stellarmls.chat.crypto.NostrEvent {
        val expiration = (System.currentTimeMillis() / 1000) + 300 // 5 minutes

        val tags = listOf(
            listOf("t", action),
            listOf("x", blobHash),
            listOf("expiration", expiration.toString())
        )

        return NostrEventBuilder.build(
            kind = 24242,
            tags = tags,
            content = "Upload encrypted media",
            keyManager = keyManager
        )
    }
}
