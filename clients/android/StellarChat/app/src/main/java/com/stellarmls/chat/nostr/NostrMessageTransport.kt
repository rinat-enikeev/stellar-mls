package com.stellarmls.chat.nostr

import com.stellarmls.chat.crypto.GroupCrypto
import com.stellarmls.chat.crypto.KeyManager
import com.stellarmls.chat.crypto.NostrEvent
import com.stellarmls.chat.crypto.NostrEventBuilder
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPGroupRenamed
import com.stellarmls.mls.SEPGroupStateUpdate
import com.stellarmls.mls.SEPMemberJoined
import com.stellarmls.mls.SEPMessageAck
import com.stellarmls.mls.SEPSaltRequest
import com.stellarmls.mls.SEPSaltResponse
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.launchIn
import kotlinx.coroutines.flow.onEach
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.CopyOnWriteArrayList

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

    /** Whether at least one relay connection is active. */
    val isAnyRelayConnected: Boolean
        get() = connections.any { it.isConnected }

    var onMessage: ((groupID: String, senderPubkey: String, text: String, eventID: String, timestamp: Long) -> Unit)? = null
    /** Called when a decrypted message is a protocol message (state update, salt request/response). */
    var onProtocolMessage: ((groupID: String, json: String, eventID: String, senderPubkey: String) -> Unit)? = null
    /** Called when a decrypted message is an image message with media attachment. */
    var onImageMessage: ((groupID: String, text: String, media: com.stellarmls.chat.model.MediaAttachment, eventID: String, senderPubkey: String, timestamp: Long) -> Unit)? = null
    /** Callback for relay OK responses: (eventID, accepted). Used for delivery status. */
    var onOK: ((String, Boolean) -> Unit)? = null

    /** Current group members used for sender authentication (H-4). */
    val currentMembers = CopyOnWriteArrayList<SEPGroupMemberLeaf>()

    fun connect() {
        for (url in relayURLs) {
            val conn = NostrRelayConnection(url)
            conn.onOK = { eventID, accepted ->
                onOK?.invoke(eventID, accepted)
            }
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

    fun subscribe(group: ChatGroup, sinceTimestamp: Long? = null) {
        val topicKey = "chat-${group.topicTag}"

        // Cancel existing subscriptions for this topic
        val toRemove = subscriptionJobs.keys.filter { it.startsWith(topicKey) }
        for (key in toRemove) {
            subscriptionJobs.remove(key)?.cancel()
        }

        val subID = "grp-${group.id.take(8)}-${UUID.randomUUID().toString().take(8)}"

        // Use an overlap window of 60 seconds to handle clock skew across relays
        val since = sinceTimestamp?.let { maxOf(0, it - 60) } ?: ((System.currentTimeMillis() / 1000) - 300)

        val filter = JSONObject().apply {
            put("kinds", JSONArray().put(44114).put(24114))
            put("#t", JSONArray().put(group.topicTag))
            put("since", since)
        }

        val key = group.encryptionKey
        val groupID = group.id

        // Subscribe to ALL connections concurrently (each gets its own coroutine)
        for (conn in connections) {
            val job = conn.subscribe(subID, filter)
                .onEach { event -> handleIncomingEvent(event, groupID, key) }
                .launchIn(scope)
            subscriptionJobs["$topicKey-${conn.hashCode()}"] = job
        }
    }

    /** Send an encrypted chat message. Returns the published NostrEvent (with deterministic NIP-01 ID). */
    fun send(group: ChatGroup, text: String): NostrEvent {
        val key = group.encryptionKey
        // v2 wrapper: includes version, type discriminator, and sender timestamp
        val wrapper = JSONObject().apply {
            put("v", 2)
            put("type", "chat")
            put("text", text)
            put("senderBlsPubkey", android.util.Base64.encodeToString(
                keyManager.blsPublicKey(), android.util.Base64.NO_WRAP))
            put("ts", System.currentTimeMillis() / 1000)
        }
        val authenticatedText = wrapper.toString()
        val envelopeJson = GroupCrypto.encrypt(authenticatedText, key)
        val content = android.util.Base64.encodeToString(
            envelopeJson.toByteArray(), android.util.Base64.NO_WRAP)

        val tags = listOf(
            listOf("t", group.topicTag)
        )

        val event = NostrEventBuilder.build(
            kind = 44114,
            tags = tags,
            content = content,
            keyManager = keyManager
        )

        for (conn in connections) {
            conn.publish(event)
        }

        return event
    }

    /** Send a protocol message (state update, salt request/response) to a group. */
    fun sendProtocolMessage(group: ChatGroup, json: String, overrideKey: ByteArray? = null) {
        val key = overrideKey ?: group.encryptionKey
        // Wrap with BLS auth like chat messages — receiver unwraps all messages uniformly
        val wrapper = JSONObject().apply {
            put("text", json)
            put("senderBlsPubkey", android.util.Base64.encodeToString(
                keyManager.blsPublicKey(), android.util.Base64.NO_WRAP))
        }
        val envelopeJson = GroupCrypto.encrypt(wrapper.toString(), key)
        val content = android.util.Base64.encodeToString(
            envelopeJson.toByteArray(), android.util.Base64.NO_WRAP)

        val tags = listOf(
            listOf("t", group.topicTag)
        )

        val event = NostrEventBuilder.build(
            kind = 44114,
            tags = tags,
            content = content,
            keyManager = keyManager
        )

        if (com.stellarmls.chat.BuildConfig.DEBUG) android.util.Log.d("MsgTransport", "sendProtocol to ${connections.size} relays")
        for (conn in connections) {
            conn.publish(event)
        }
    }

    /** Publish a pre-built event to all connected relays. */
    fun publish(event: NostrEvent) {
        for (conn in connections) {
            conn.publish(event)
        }
    }

    /** Process a single incoming Nostr event: decrypt, unwrap BLS, route to chat or protocol handler. */
    private fun handleIncomingEvent(event: NostrEvent, groupID: String, key: ByteArray) {
        try {
            val envelopeJson = String(
                android.util.Base64.decode(event.content, android.util.Base64.NO_WRAP))
            val plaintext = GroupCrypto.decrypt(envelopeJson, key)

            // All messages are BLS-wrapped: {"text":"...", "senderBlsPubkey":"..."}
            val wrapper = try { JSONObject(plaintext) } catch (_: Exception) { null }
            val innerText = wrapper?.optString("text")
            val blsPubkeyB64 = wrapper?.optString("senderBlsPubkey")

            if (innerText.isNullOrEmpty() || blsPubkeyB64.isNullOrEmpty()) {
                // N-6: Reject messages without BLS sender authentication.
                if (com.stellarmls.chat.BuildConfig.DEBUG) android.util.Log.w("MsgTransport", "Rejected: missing BLS auth.")
                com.stellarmls.chat.SecurityLog.nonMemberMessageRejected(groupID)
                return
            }

            val wrapperType = wrapper?.optString("type", "chat") ?: "chat"

            if (com.stellarmls.chat.BuildConfig.DEBUG) android.util.Log.d("MsgTransport", "Decrypted OK group=${groupID.take(8)} wrapperType=$wrapperType members=${currentMembers.size}")

            if (wrapperType == "image") {
                // Image message — verify BLS pubkey is in member list (H-4)
                val blsPubkey = android.util.Base64.decode(blsPubkeyB64, android.util.Base64.NO_WRAP)
                val isMember = currentMembers.any { it.publicKeyCompressed.contentEquals(blsPubkey) }
                if (!isMember) {
                    if (com.stellarmls.chat.BuildConfig.DEBUG) android.util.Log.w("MsgTransport", "BLS rejected image: members=${currentMembers.size}")
                    com.stellarmls.chat.SecurityLog.nonMemberMessageRejected(groupID)
                    return
                }
                val mediaObj = wrapper.optJSONObject("media") ?: return
                val servers = mediaObj.optJSONArray("blossomServers")?.let { arr ->
                    (0 until arr.length()).map { arr.getString(it) }
                } ?: emptyList()
                val media = com.stellarmls.chat.model.MediaAttachment(
                    blobHash = mediaObj.getString("blobHash"),
                    fileKey = android.util.Base64.decode(mediaObj.getString("fileKey"), android.util.Base64.NO_WRAP),
                    mimeType = mediaObj.optString("mimeType", "image/jpeg"),
                    width = mediaObj.optInt("width", 0),
                    height = mediaObj.optInt("height", 0),
                    size = mediaObj.optInt("size", 0),
                    blossomServers = servers,
                    encryptedThumbnail = mediaObj.optString("thumbnail", "").takeIf { it.isNotEmpty() }
                        ?.let { android.util.Base64.decode(it, android.util.Base64.NO_WRAP) }
                )
                onImageMessage?.invoke(groupID, innerText, media, event.id, event.pubkey, event.createdAt)
            } else {
                // Check inner text for protocol messages (state updates, salt, etc.)
                if (isProtocolMessage(innerText)) {
                    // Update currentMembers SYNCHRONOUSLY before processing the next event,
                    // so that chat messages arriving right after are not rejected.
                    applyMemberChanges(innerText)
                    onProtocolMessage?.invoke(groupID, innerText, event.id, event.pubkey)
                } else {
                    // Chat message — verify BLS pubkey is in member list (H-4)
                    val blsPubkey = android.util.Base64.decode(blsPubkeyB64, android.util.Base64.NO_WRAP)
                    val isMember = currentMembers.any { it.publicKeyCompressed.contentEquals(blsPubkey) }
                    if (isMember) {
                        onMessage?.invoke(groupID, event.pubkey, innerText,
                            event.id, event.createdAt)
                    } else {
                        if (com.stellarmls.chat.BuildConfig.DEBUG) android.util.Log.w("MsgTransport", "BLS rejected: members=${currentMembers.size}")
                        com.stellarmls.chat.SecurityLog.nonMemberMessageRejected(groupID)
                    }
                }
            }
        } catch (e: Exception) {
            if (com.stellarmls.chat.BuildConfig.DEBUG) android.util.Log.e("MsgTransport", "Decrypt failed group=${groupID.take(8)} err=${e.message}")
            com.stellarmls.chat.SecurityLog.decryptionFailed("group message")
        }
    }

    /** Synchronously update currentMembers from protocol messages so the BLS check
     *  for subsequent chat messages uses the latest member list. */
    private fun applyMemberChanges(json: String) {
        try {
            val obj = JSONObject(json)
            when (obj.optString("type")) {
                SEPMemberJoined.MESSAGE_TYPE -> {
                    val memberObj = obj.optJSONObject("member") ?: return
                    val pubKey = android.util.Base64.decode(
                        memberObj.getString("publicKeyCompressed"), android.util.Base64.NO_WRAP)
                    if (currentMembers.none { it.publicKeyCompressed.contentEquals(pubKey) }) {
                        val leafHash = android.util.Base64.decode(
                            memberObj.getString("leafHash"), android.util.Base64.NO_WRAP)
                        currentMembers.add(SEPGroupMemberLeaf(pubKey, leafHash))
                    }
                }
                SEPGroupStateUpdate.MESSAGE_TYPE -> {
                    val removedArr = obj.optJSONArray("removedMemberKeys")
                    if (removedArr != null) {
                        for (i in 0 until removedArr.length()) {
                            val removed = android.util.Base64.decode(removedArr.getString(i), android.util.Base64.NO_WRAP)
                            currentMembers.removeAll { it.publicKeyCompressed.contentEquals(removed) }
                        }
                    }
                    val addedArr = obj.optJSONArray("addedMembers")
                    if (addedArr != null) {
                        for (i in 0 until addedArr.length()) {
                            val m = addedArr.getJSONObject(i)
                            val pubKey = android.util.Base64.decode(
                                m.getString("publicKeyCompressed"), android.util.Base64.NO_WRAP)
                            if (currentMembers.none { it.publicKeyCompressed.contentEquals(pubKey) }) {
                                val leafHash = android.util.Base64.decode(
                                    m.getString("leafHash"), android.util.Base64.NO_WRAP)
                                currentMembers.add(SEPGroupMemberLeaf(pubKey, leafHash))
                            }
                        }
                    }
                }
            }
        } catch (_: Exception) { }
    }

    /** Check if text is a protocol message (matches a known protocol type). */
    private fun isProtocolMessage(text: String): Boolean {
        return try {
            val obj = JSONObject(text)
            val type = obj.optString("type", "")
            type in setOf(
                SEPMemberJoined.MESSAGE_TYPE,
                SEPGroupStateUpdate.MESSAGE_TYPE,
                SEPSaltRequest.MESSAGE_TYPE,
                SEPSaltResponse.MESSAGE_TYPE,
                SEPGroupRenamed.MESSAGE_TYPE,
                SEPMessageAck.MESSAGE_TYPE
            )
        } catch (_: Exception) {
            false
        }
    }
}
