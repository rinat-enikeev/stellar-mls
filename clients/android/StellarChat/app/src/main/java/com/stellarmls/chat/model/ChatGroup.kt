package com.stellarmls.chat.model

import com.stellarmls.chat.crypto.GroupCrypto
import org.json.JSONArray
import org.json.JSONObject
import java.util.Date

data class ChatGroup(
    val id: String,           // hex-encoded 32-byte group ID
    val name: String,
    val groupSecret: ByteArray, // 32-byte shared secret
    val createdAt: Date = Date(),
    val relayHints: List<String> = listOf(
        "wss://relay.damus.io",
        "wss://nos.lol"
    )
) {
    val topicTag: String get() = GroupCrypto.hiddenGroupTopic(groupSecret)
    val encryptionKey: ByteArray get() = GroupCrypto.deriveMessageKey(groupSecret)

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is ChatGroup) return false
        return id == other.id
    }

    override fun hashCode(): Int = id.hashCode()
}

data class ChatMessage(
    val id: String,
    val groupID: String,
    val senderPubkey: String,
    val text: String,
    val timestamp: Date,
    val isMine: Boolean
)

data class InviteCode(
    val groupID: ByteArray,
    val groupSecret: ByteArray,
    val name: String,
    val relayHints: List<String>
) {
    fun encode(): String {
        val json = JSONObject().apply {
            put("groupID", groupID.toHex())
            put("groupSecret", groupSecret.toHex())
            put("name", name)
            put("relayHints", JSONArray(relayHints))
        }
        return android.util.Base64.encodeToString(
            json.toString().toByteArray(),
            android.util.Base64.NO_WRAP
        )
    }

    companion object {
        fun decode(encoded: String): InviteCode {
            val json = JSONObject(
                String(android.util.Base64.decode(encoded, android.util.Base64.NO_WRAP))
            )
            return InviteCode(
                groupID = json.getString("groupID").hexToBytes(),
                groupSecret = json.getString("groupSecret").hexToBytes(),
                name = json.getString("name"),
                relayHints = (0 until json.getJSONArray("relayHints").length()).map {
                    json.getJSONArray("relayHints").getString(it)
                }
            )
        }
    }
}

fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }

fun String.hexToBytes(): ByteArray {
    check(length % 2 == 0)
    return chunked(2).map { it.toInt(16).toByte() }.toByteArray()
}
