package com.stellarmls.chat.nostr

import com.stellarmls.chat.crypto.GroupCrypto
import com.stellarmls.chat.crypto.KeyManager
import com.stellarmls.chat.crypto.NostrEvent
import com.stellarmls.chat.crypto.NostrEventBuilder
import com.stellarmls.chat.model.ChatGroup
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

class NostrMessageTransport(
    private val keyManager: KeyManager,
    private val relayURLs: List<String> = listOf(
        "wss://relay.damus.io",
        "wss://nos.lol",
        "wss://relay.nostr.band",
        "wss://relay.snort.social",
        "wss://nostr.wine"
    )
) {
    private val connections = mutableListOf<NostrRelayConnection>()
    private val subscriptionJobs = ConcurrentHashMap<String, Job>()
    private val scope = CoroutineScope(Dispatchers.IO)

    var onMessage: ((groupID: String, senderPubkey: String, text: String, eventID: String, timestamp: Long) -> Unit)? = null
    /** Called when a decrypted message is a protocol message (state update, salt request/response). */
    var onProtocolMessage: ((groupID: String, json: String, eventID: String, senderPubkey: String) -> Unit)? = null

    fun connect() {
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

    fun subscribe(group: ChatGroup) {
        val subID = "grp-${group.id.take(8)}-${UUID.randomUUID().toString().take(8)}"

        // Use a "since" timestamp to prevent historical event flood on subscription.
        // Default to 5 minutes ago to catch recent messages while avoiding full replay.
        val sinceTimestamp = (System.currentTimeMillis() / 1000) - 300

        val filter = JSONObject().apply {
            put("kinds", JSONArray().put(24114))
            put("#t", JSONArray().put(group.topicTag))
            put("since", sinceTimestamp)
        }

        for (conn in connections) {
            val job = conn.subscribe(subID, filter)
                .onEach { event -> handleEvent(event, group) }
                .launchIn(scope)
            subscriptionJobs["$subID-${conn.hashCode()}"] = job
        }
    }

    fun send(group: ChatGroup, text: String) {
        val key = group.encryptionKey
        // Wrap text with sender BLS pubkey for receiver-side membership verification (H-4)
        val authenticatedText = try {
            val wrapper = JSONObject().apply {
                put("text", text)
                put("senderBlsPubkey", android.util.Base64.encodeToString(
                    keyManager.blsPublicKey(), android.util.Base64.NO_WRAP))
            }
            wrapper.toString()
        } catch (_: Exception) { text }
        val envelope = GroupCrypto.encrypt(authenticatedText, key)

        val tags = listOf(
            listOf("t", group.topicTag)
        )

        val event = NostrEventBuilder.build(
            kind = 24114,
            tags = tags,
            content = envelope,
            keyManager = keyManager
        )

        for (conn in connections) {
            conn.publish(event)
        }
    }

    /** Send a protocol message (state update, salt request/response) to a group. */
    fun sendProtocolMessage(group: ChatGroup, json: String) {
        val key = group.encryptionKey
        val envelope = GroupCrypto.encrypt(json, key)

        val tags = listOf(
            listOf("t", group.topicTag)
        )

        val event = NostrEventBuilder.build(
            kind = 24114,
            tags = tags,
            content = envelope,
            keyManager = keyManager
        )

        for (conn in connections) {
            conn.publish(event)
        }
    }

    private fun handleEvent(event: NostrEvent, group: ChatGroup) {
        try {
            val key = group.encryptionKey
            val plaintext = GroupCrypto.decrypt(event.content, key)

            // Distinguish protocol messages from plain-text chat
            if (isProtocolMessage(plaintext)) {
                onProtocolMessage?.invoke(group.id, plaintext, event.id, event.pubkey)
            } else {
                // Sender authentication (H-4): verify BLS pubkey is in member list
                try {
                    val obj = JSONObject(plaintext)
                    if (obj.has("text") && obj.has("senderBlsPubkey")) {
                        val blsPubkey = android.util.Base64.decode(
                            obj.getString("senderBlsPubkey"), android.util.Base64.NO_WRAP)
                        val isMember = group.members.any { it.publicKeyCompressed.contentEquals(blsPubkey) }
                        if (isMember) {
                            onMessage?.invoke(group.id, event.pubkey, obj.getString("text"),
                                event.id, event.createdAt)
                        }
                        if (!isMember) {
                            com.stellarmls.chat.SecurityLog.nonMemberMessageRejected(group.id)
                        }
                        // Non-member messages are silently dropped
                        return
                    }
                } catch (_: Exception) {
                    // N-6: Reject messages without BLS sender authentication.
                    // Legacy unverified messages are no longer accepted to prevent
                    // bypass of H-4 sender authentication.
                    com.stellarmls.chat.SecurityLog.nonMemberMessageRejected(group.id)
                    return
                }
            }
        } catch (_: Exception) {
            com.stellarmls.chat.SecurityLog.decryptionFailed("group message")
        }
    }

    /** Check if decrypted text is a protocol message (has a "type" field). */
    private fun isProtocolMessage(text: String): Boolean {
        return try {
            val obj = JSONObject(text)
            obj.has("type")
        } catch (_: Exception) {
            false
        }
    }
}
