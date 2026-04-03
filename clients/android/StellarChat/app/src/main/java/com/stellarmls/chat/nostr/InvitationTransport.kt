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
 * Kind 34113 invitation send/receive over Nostr relays.
 * Still listens for legacy kind 24113 events during migration.
 * Uses X25519 ECDH + AES-256-GCM for invitation encryption.
 */
class InvitationTransport(private val keyManager: KeyManager) {
    companion object {
        private const val PRIMARY_KIND = 34113
        private const val LEGACY_KIND = 24113
        private const val PRIMARY_INBOX_FILTER_KEY = "#d"
        private const val PRIMARY_INBOX_EVENT_TAG_KEY = "d"
        private const val SECONDARY_INBOX_EVENT_TAG_KEY = "t"
        private const val LEGACY_INBOX_EVENT_TAG_KEY = "sep_inbox"
        private const val PRIMARY_INBOX_TAG_PREFIX = "sep-inbox:"

        fun subscriptionFilters(inboxTag: String): List<JSONObject> = listOf(
            JSONObject().apply {
                put("kinds", JSONArray().put(PRIMARY_KIND))
                put(PRIMARY_INBOX_FILTER_KEY, JSONArray().put(PRIMARY_INBOX_TAG_PREFIX + inboxTag))
            },
            JSONObject().apply {
                put("kinds", JSONArray().put(PRIMARY_KIND))
                put("#t", JSONArray().put(inboxTag))
            },
            JSONObject().apply {
                put("kinds", JSONArray().put(LEGACY_KIND))
                put("#t", JSONArray().put(inboxTag))
            }
        )

        fun eventTags(recipientInboxTag: String): List<List<String>> = listOf(
            // Use a parameterized-replaceable `d` tag on an addressable kind so
            // relays can retain and query invitations across reconnects.
            listOf(PRIMARY_INBOX_EVENT_TAG_KEY, PRIMARY_INBOX_TAG_PREFIX + recipientInboxTag),
            listOf(SECONDARY_INBOX_EVENT_TAG_KEY, recipientInboxTag),
            listOf(LEGACY_INBOX_EVENT_TAG_KEY, recipientInboxTag),
            listOf("sep_version", "1")
        )
    }

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
        if (com.stellarmls.chat.BuildConfig.DEBUG) {
            android.util.Log.d("InvitationTransport", "connect relays=$relayURLs")
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

        val filters = subscriptionFilters(inboxTag)
        if (com.stellarmls.chat.BuildConfig.DEBUG) {
            android.util.Log.d("InvitationTransport", "subscribeToInbox inboxTag=$inboxTag filters=$filters")
        }

        for (conn in connections) {
            filters.forEachIndexed { index, filter ->
                val filterSubID = "$subID-$index"
                val job = conn.subscribe(filterSubID, filter)
                    .onEach { event -> handleInvitationEvent(event, privateKey) }
                    .launchIn(scope)
                subscriptionJobs["$filterSubID-${conn.hashCode()}"] = job
            }
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

        val tags = eventTags(recipientInboxTag)

        val event = NostrEventBuilder.build(
            kind = PRIMARY_KIND,
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
            if (com.stellarmls.chat.BuildConfig.DEBUG) {
                android.util.Log.d(
                    "InvitationTransport",
                    "Received event id=${event.id.take(12)} kind=${event.kind} tags=${event.tags}"
                )
            }
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
                receivedAt = Date(event.displayMilliseconds)
            )

            onInvitation?.invoke(invitation)
        } catch (e: Exception) {
            if (com.stellarmls.chat.BuildConfig.DEBUG) {
                android.util.Log.d(
                    "InvitationTransport",
                    "Ignored event ${event.id.take(12)}: ${e.message}"
                )
            }
        }
    }
}
