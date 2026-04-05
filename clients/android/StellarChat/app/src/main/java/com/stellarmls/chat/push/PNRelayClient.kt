package com.stellarmls.chat.push

import com.stellarmls.chat.crypto.GroupCrypto
import com.stellarmls.chat.crypto.KeyManager
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.bouncycastle.crypto.params.X25519PublicKeyParameters
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.TimeUnit

class PNRelayClient(private val relayURL: String) {
    private val client = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(15, TimeUnit.SECONDS)
        .build()

    private var relayPublicKey: ByteArray? = null

    /// Fetch PN relay's X25519 public key.
    suspend fun fetchInfo(): ByteArray = withContext(Dispatchers.IO) {
        val request = Request.Builder()
            .url("$relayURL/v1/info")
            .get()
            .build()
        val response = client.newCall(request).execute()
        val body = response.body?.string() ?: throw Exception("Empty info response")
        val json = JSONObject(body)
        val pubkeyB64 = json.getString("x25519_public_key")
        val pubkey = android.util.Base64.decode(pubkeyB64, android.util.Base64.DEFAULT)
        relayPublicKey = pubkey
        pubkey
    }

    private suspend fun getRelayPublicKey(): ByteArray {
        return relayPublicKey ?: fetchInfo()
    }

    /// Register a push subscription for a group topic.
    suspend fun subscribe(
        subscriptionID: String,
        filter: JSONObject,
        pushToken: String,
        notificationKey: ByteArray,
        platform: String
    ) {
        val relayPubkey = getRelayPublicKey()
        val payload = JSONObject().apply {
            put("action", "subscribe")
            put("subscription_id", subscriptionID)
            put("filter", filter)
            put("token", pushToken)
            put("notification_key", android.util.Base64.encodeToString(notificationKey, android.util.Base64.NO_WRAP))
            put("platform", platform)
        }
        sendEncryptedEnvelope(payload, relayPubkey)
    }

    /// Update an existing subscription's filter.
    suspend fun update(subscriptionID: String, filter: JSONObject) {
        val relayPubkey = getRelayPublicKey()
        val payload = JSONObject().apply {
            put("action", "update")
            put("subscription_id", subscriptionID)
            put("filter", filter)
        }
        sendEncryptedEnvelope(payload, relayPubkey)
    }

    /// Unsubscribe.
    suspend fun unsubscribe(subscriptionID: String) {
        val relayPubkey = getRelayPublicKey()
        val payload = JSONObject().apply {
            put("action", "unsubscribe")
            put("subscription_id", subscriptionID)
        }
        sendEncryptedEnvelope(payload, relayPubkey)
    }

    /// Encrypt payload using ephemeral X25519 + AES-256-GCM and POST to /v1/subscription.
    private suspend fun sendEncryptedEnvelope(payload: JSONObject, relayPubkey: ByteArray) = withContext(Dispatchers.IO) {
        val payloadBytes = payload.toString().toByteArray()
        // encryptInvitation uses ephemeral X25519 ECDH — anonymous, no signing
        val envelopeJSON = GroupCrypto.encryptInvitation(
            payload = payloadBytes,
            recipientKeyAgreementPubkey = relayPubkey,
            senderKeyManager = null  // No signing — anonymous registration
        )

        val request = Request.Builder()
            .url("$relayURL/v1/subscription")
            .post(envelopeJSON.toRequestBody("application/json".toMediaType()))
            .build()

        val response = client.newCall(request).execute()
        if (!response.isSuccessful) {
            val body = response.body?.string() ?: ""
            throw Exception("PN relay error ${response.code}: $body")
        }
    }
}
