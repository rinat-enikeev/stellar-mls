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
        "wss://nos.lol"
    )
) {
    private val connections = mutableListOf<NostrRelayConnection>()
    private val subscriptionJobs = ConcurrentHashMap<String, Job>()
    private val scope = CoroutineScope(Dispatchers.IO)

    var onMessage: ((groupID: String, senderPubkey: String, text: String, eventID: String, timestamp: Long) -> Unit)? = null

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

        val filter = JSONObject().apply {
            put("kinds", JSONArray().put(24114))
            put("#t", JSONArray().put(group.topicTag))
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
        val envelope = GroupCrypto.encrypt(text, key)

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
            onMessage?.invoke(
                group.id,
                event.pubkey,
                plaintext,
                event.id,
                event.createdAt
            )
        } catch (_: Exception) {
            // Decryption failed — event not for this group or corrupted
        }
    }
}
