package com.stellarmls.chat.model

import com.stellarmls.chat.crypto.GroupCrypto
import com.stellarmls.mls.SEPCommitmentBuilder
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPTier
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
    ),
    // SEP membership state
    var members: MutableList<SEPGroupMemberLeaf> = mutableListOf(),
    var epoch: Long = 0,
    var salt: ByteArray = SEPCommitmentBuilder.generateSalt(),
    var commitment: ByteArray? = null,
    var tier: SEPTier = SEPTier.SMALL,
    var isPublishedOnChain: Boolean = false
) {
    /** Group ID as raw bytes (hex string → ByteArray). */
    val groupIDData: ByteArray get() = id.hexToBytes()
    val topicTag: String get() = GroupCrypto.hiddenGroupTopic(groupSecret)
    val encryptionKey: ByteArray get() = GroupCrypto.deriveMessageKey(groupSecret, epoch, salt)

    /** Recompute Merkle root and commitment from current member list. */
    fun recomputeCommitment() {
        val root = SEPCommitmentBuilder.computeMerkleRoot(members, tier)
        commitment = SEPCommitmentBuilder.computeSHA256Commitment(root, epoch, salt)
    }

    /** Add a member and recompute the commitment.
     *  Members are sorted by compressed G1 public key per SEP-XXXX §2.1. */
    fun addMember(leaf: SEPGroupMemberLeaf) {
        if (members.size >= tier.maxMembers) return
        members.add(leaf)
        members.sortWith { a, b ->
            val aKey = a.publicKeyCompressed
            val bKey = b.publicKeyCompressed
            for (i in 0 until minOf(aKey.size, bKey.size)) {
                val cmp = (aKey[i].toInt() and 0xFF) - (bKey[i].toInt() and 0xFF)
                if (cmp != 0) return@sortWith cmp
            }
            aKey.size - bKey.size
        }
        epoch++
        salt = SEPCommitmentBuilder.generateSalt()
        recomputeCommitment()
    }

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
    /** Encode to a Base64 string using the canonical JSON format (base64 for binary fields).
     *  Compatible with iOS Codable serialization of Data fields. */
    fun encode(): String {
        val json = JSONObject().apply {
            put("groupID", android.util.Base64.encodeToString(groupID, android.util.Base64.NO_WRAP))
            put("groupSecret", android.util.Base64.encodeToString(groupSecret, android.util.Base64.NO_WRAP))
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
            val groupIDStr = json.getString("groupID")
            val groupSecretStr = json.getString("groupSecret")
            return InviteCode(
                groupID = decodeFlexible(groupIDStr),
                groupSecret = decodeFlexible(groupSecretStr),
                name = json.getString("name"),
                relayHints = (0 until json.getJSONArray("relayHints").length()).map {
                    json.getJSONArray("relayHints").getString(it)
                }
            )
        }

        /** Decode a string that may be base64 or hex-encoded (for backward compatibility). */
        private fun decodeFlexible(value: String): ByteArray {
            // Base64-encoded 32 bytes → 44 chars; hex-encoded 32 bytes → 64 chars.
            // Hex strings are always even-length and contain only [0-9a-fA-F].
            return if (value.length == 64 && value.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }) {
                value.hexToBytes()
            } else {
                android.util.Base64.decode(value, android.util.Base64.NO_WRAP)
            }
        }
    }
}

fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }

fun String.hexToBytes(): ByteArray {
    check(length % 2 == 0)
    return chunked(2).map { it.toInt(16).toByte() }.toByteArray()
}
