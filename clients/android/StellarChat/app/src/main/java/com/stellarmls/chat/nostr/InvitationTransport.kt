package com.stellarmls.chat.nostr

import com.stellarmls.chat.crypto.GroupCrypto
import com.stellarmls.chat.crypto.KeyManager
import com.stellarmls.chat.crypto.NostrEvent
import com.stellarmls.chat.crypto.NostrEventBuilder
import com.stellarmls.chat.model.BootstrapPayload
import com.stellarmls.chat.model.PendingInvitation
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import org.bouncycastle.crypto.params.X25519PrivateKeyParameters
import org.json.JSONArray
import org.json.JSONObject
import java.util.Date
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

/**
 * Kind 24113 invitation send/receive over Nostr relays.
 * Uses X25519 ECDH + AES-256-GCM for invitation encryption.
 */
class InvitationTransport(private val keyManager: KeyManager) {
    private val connections = mutableListOf<NostrRelayConnection>()
    private val subscriptionJobs = ConcurrentHashMap<String, Job>()
    private val scope = CoroutineScope(Dispatchers.IO)

    var onInvitation: ((PendingInvitation) -> Unit)? = null
    var onError: ((String) -> Unit)? = null

    fun connect(relayURLs: List<String>) {
        for (url in relayURLs) {
            val conn = NostrRelayConnection(url)
            conn.connect()
            connections.add(conn)
        }
    }

    fun disconnect() {
        subscriptionJobs.values.forEach { it.cancel() }
        subscriptionJobs.clear()
        connections.forEach { it.disconnect() }
        connections.clear()
    }

    /** Subscribe to inbox for incoming invitations. */
    fun subscribeToInbox(inboxTag: String, privateKey: X25519PrivateKeyParameters) {
        val subID = "inbox-${UUID.randomUUID().toString().take(8)}"

        val filter = JSONObject().apply {
            put("kinds", JSONArray().put(24113))
            put("#sep_inbox", JSONArray().put(inboxTag))
        }

        for (conn in connections) {
            val job = conn.subscribe(subID, filter)
                .onEach { event -> handleInvitationEvent(event, privateKey) }
                .launchIn(scope)
            subscriptionJobs["$subID-${conn.hashCode()}"] = job
        }
    }

    /** Send an invitation to a recipient via their inbox. */
    fun sendInvitation(
        payload: BootstrapPayload,
        recipientKeyAgreementPubkey: ByteArray,
        keyManager: KeyManager
    ) {
        val payloadJson = payload.toJson()
        val envelopeJson = GroupCrypto.encryptInvitation(
            payload = payloadJson.toByteArray(),
            recipientKeyAgreementPubkey = recipientKeyAgreementPubkey,
            senderKeyManager = keyManager
        )

        // Base64 encode the envelope
        val contentBase64 = android.util.Base64.encodeToString(
            envelopeJson.toByteArray(),
            android.util.Base64.NO_WRAP
        )

        val recipientInboxTag = GroupCrypto.hiddenInboxTag(recipientKeyAgreementPubkey)

        val tags = listOf(
            listOf("sep_inbox", recipientInboxTag),
            listOf("sep_version", "1")
        )

        val event = NostrEventBuilder.build(
            kind = 24113,
            tags = tags,
            content = contentBase64,
            keyManager = keyManager
        )

        for (conn in connections) {
            conn.publish(event)
        }
    }

    private fun handleInvitationEvent(event: NostrEvent, privateKey: X25519PrivateKeyParameters) {
        try {
            // Base64 decode the content to get the sealed envelope JSON
            val envelopeJson = String(
                android.util.Base64.decode(event.content, android.util.Base64.NO_WRAP)
            )

            // Decrypt using X25519 ECDH
            val payloadBytes = GroupCrypto.decryptInvitation(envelopeJson, privateKey)
            val payload = BootstrapPayload.fromJson(String(payloadBytes))

            val invitation = PendingInvitation(
                id = event.id,
                payload = payload,
                receivedAt = Date()
            )

            onInvitation?.invoke(invitation)
        } catch (_: Exception) {
            // Decryption failed — event not intended for us or corrupted
        }
    }
}
