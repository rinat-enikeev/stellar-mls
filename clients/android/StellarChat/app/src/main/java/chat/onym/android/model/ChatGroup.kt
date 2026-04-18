package chat.onym.android.model

import chat.onym.android.BuildConfig
import chat.onym.android.crypto.GroupCrypto
import com.stellarmls.mls.SEPCommitmentBuilder
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPGroupType
import com.stellarmls.mls.SEPTier
import org.json.JSONArray
import org.json.JSONObject
import java.util.Date

data class ChatGroup(
    val id: String,           // hex-encoded 32-byte group ID
    val name: String,
    val groupSecret: ByteArray, // 32-byte shared secret
    val createdAt: Date = Date(),
    val relayHints: List<String> = buildList {
        if (BuildConfig.DEFAULT_NOSTR_RELAY.isNotEmpty()) add(BuildConfig.DEFAULT_NOSTR_RELAY)
        addAll(listOf(
            "wss://relay.damus.io",
            "wss://nos.lol",
            "wss://relay.nostr.band",
            "wss://relay.snort.social",
            "wss://nostr.wine"
        ))
    },
    // SEP membership state
    var members: MutableList<SEPGroupMemberLeaf> = mutableListOf(),
    var epoch: Long = 0,
    var salt: ByteArray = SEPCommitmentBuilder.generateSalt(),
    var commitment: ByteArray? = null,
    var tier: SEPTier = SEPTier.LARGE,
    /** Governance type selected at creation. Existing persisted groups
     *  default to [SEPGroupType.ANARCHY] (the historical behavior). */
    var groupType: SEPGroupType = SEPGroupType.ANARCHY,
    /** Hex-encoded BLS pubkeys (lowercase) of members with admin privileges.
     *  Only meaningful for [SEPGroupType.OLIGARCHY] groups; seeded with the creator
     *  at group creation. Existing persisted groups decode this as empty. */
    var adminPubkeys: MutableSet<String> = mutableSetOf(),
    var isPublishedOnChain: Boolean = false,
    /** Unix timestamp of the last received event, used for offline catch-up. */
    var lastEventTimestamp: Long = 0,
    /** When non-null, the user is pinned to this historical epoch for viewing/sending. */
    var pinnedEpoch: Long? = null,
    /** If this group was forked from another group, stores the original group's ID. */
    var forkedFromGroupID: String? = null,
    /** The epoch of the original group at which this fork was created. */
    var forkedAtEpoch: Long? = null,
    /** BLS pubkey hex of the member who removed us from this group (null if not removed). */
    var removedByPubkeyHex: String? = null,
    /** Whether push notifications are enabled for this group (opt-in, defaults to off). */
    var pushNotificationsEnabled: Boolean = false,
    /** Timestamp (ms since epoch) of the latest message in this group. Drives chat-list
     *  ordering (most recent first). 0 = no messages yet, falls back to [createdAt]. */
    var lastMessageAt: Long = 0,
    /** Whether this chat is pinned to the top of the chat list. */
    var isPinned: Boolean = false
) {
    /** Group ID as raw bytes (hex string → ByteArray). */
    val groupIDData: ByteArray get() = id.hexToBytes()
    /** Nostr subscription topic tag for this group.
     *  Derivation (SEP-XXXX §3.1): `topicTag = hex(SHA-256("sep-topic-v1" || groupSecret)[0..8])`
     *  Both platforms MUST use this same derivation to ensure cross-platform message visibility. */
    val topicTag: String get() = GroupCrypto.hiddenGroupTopic(groupSecret)
    val encryptionKey: ByteArray get() = GroupCrypto.deriveMessageKey(groupSecret, epoch, salt)

    /** Recompute Merkle root and Poseidon commitment from current member list. */
    fun recomputeCommitment() {
        val root = SEPCommitmentBuilder.computeMerkleRoot(members, tier)
        commitment = SEPCommitmentBuilder.computePoseidonCommitment(root, epoch, salt)
    }

    /** Add a member and recompute the commitment.
     *  Members are sorted by compressed G1 public key per SEP-XXXX §2.1.
     *  Salt is derived deterministically from the previous salt and the new member's key. */
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
        salt = SEPCommitmentBuilder.deriveSalt(salt, leaf.publicKeyCompressed)
        recomputeCommitment()
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is ChatGroup) return false
        return id == other.id && epoch == other.epoch && members.size == other.members.size && name == other.name
    }

    override fun hashCode(): Int = 31 * id.hashCode() + epoch.hashCode()
}

enum class MessageStatus {
    SENDING, SENT, DELIVERED, FAILED
}

data class ChatMessage(
    val id: String,
    val groupID: String,
    val senderPubkey: String,
    val text: String,
    val timestamp: Date,
    val isMine: Boolean,
    val status: MessageStatus = MessageStatus.SENT,
    val mediaAttachment: MediaAttachment? = null,
    val isSystemMessage: Boolean = false,
    /** The epoch under which this message was encrypted (from the event's epoch tag). */
    val epoch: Long? = null,
    /** NIP-01 event ID of the parent message this is a reply to, or null. */
    val replyToID: String? = null
)

/** Encrypted media attachment metadata, embedded inside E2EE message envelopes. */
data class MediaAttachment(
    val blobHash: String,           // SHA-256 hex of encrypted blob on Blossom
    val fileKey: ByteArray,         // 32-byte AES-256-GCM per-file key
    val mimeType: String,           // e.g. "image/jpeg" or "video/mp4"
    val width: Int,
    val height: Int,
    val size: Int,                  // encrypted blob size in bytes
    val blossomServers: List<String>, // server base URLs for retrieval
    val encryptedThumbnail: ByteArray?, // combined-format encrypted thumbnail
    val duration: Int? = null       // video duration in seconds, null for images
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is MediaAttachment) return false
        return blobHash == other.blobHash
    }
    override fun hashCode(): Int = blobHash.hashCode()
}

/** Captures group state at a specific epoch for epoch branching.
 *  Users can pin to a historical epoch to continue communicating with removed members. */
data class EpochSnapshot(
    val epoch: Long,
    val members: List<SEPGroupMemberLeaf>,
    val salt: ByteArray,
    val groupSecret: ByteArray,
    val changeDescription: String
) {
    val topicTag: String get() = GroupCrypto.hiddenGroupTopic(groupSecret)
    fun encryptionKey(): ByteArray = GroupCrypto.deriveMessageKey(groupSecret, epoch, salt)

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is EpochSnapshot) return false
        return epoch == other.epoch && groupSecret.contentEquals(other.groupSecret)
    }

    override fun hashCode(): Int = 31 * epoch.hashCode() + groupSecret.contentHashCode()
}

data class InviteCode(
    val groupID: ByteArray,
    val groupSecret: ByteArray,
    val name: String,
    val relayHints: List<String>,
    /** Full group state — required for joiners to share the same encryption key and member list. */
    val members: List<SEPGroupMemberLeaf>,
    val epoch: Long,
    val salt: ByteArray,
    val commitment: ByteArray?,
    val tierRawValue: Int = SEPTier.LARGE.id,
    /** Governance type of the invited group. Defaults to Anarchy (0) for
     *  backward compatibility with older invite codes. */
    val groupTypeRawValue: Int = SEPGroupType.ANARCHY.id
) {
    /** Encode to a Base64 string using the canonical JSON format (base64 for binary fields).
     *  Compatible with iOS Codable serialization of Data fields. */
    fun encode(): String {
        val membersArray = JSONArray().apply {
            for (m in members) {
                put(JSONObject().apply {
                    put("publicKeyCompressed", android.util.Base64.encodeToString(
                        m.publicKeyCompressed, android.util.Base64.NO_WRAP))
                    put("leafHash", android.util.Base64.encodeToString(
                        m.leafHash, android.util.Base64.NO_WRAP))
                })
            }
        }
        val json = JSONObject().apply {
            put("groupID", android.util.Base64.encodeToString(groupID, android.util.Base64.NO_WRAP))
            put("groupSecret", android.util.Base64.encodeToString(groupSecret, android.util.Base64.NO_WRAP))
            put("name", name)
            put("relayHints", JSONArray(relayHints))
            put("members", membersArray)
            put("epoch", epoch)
            put("salt", android.util.Base64.encodeToString(salt, android.util.Base64.NO_WRAP))
            if (commitment != null) {
                put("commitment", android.util.Base64.encodeToString(commitment, android.util.Base64.NO_WRAP))
            }
            put("tierRawValue", tierRawValue)
            put("groupTypeRawValue", groupTypeRawValue)
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

            val members = mutableListOf<SEPGroupMemberLeaf>()
            val membersArray = json.optJSONArray("members")
            if (membersArray != null) {
                for (i in 0 until membersArray.length()) {
                    val mObj = membersArray.getJSONObject(i)
                    members.add(SEPGroupMemberLeaf(
                        publicKeyCompressed = android.util.Base64.decode(
                            mObj.getString("publicKeyCompressed"), android.util.Base64.NO_WRAP),
                        leafHash = android.util.Base64.decode(
                            mObj.getString("leafHash"), android.util.Base64.NO_WRAP)
                    ))
                }
            }

            val saltStr = json.optString("salt", "")
            val commitmentStr = json.optString("commitment", "")

            return InviteCode(
                groupID = decodeFlexible(groupIDStr),
                groupSecret = decodeFlexible(groupSecretStr),
                name = json.getString("name"),
                relayHints = (0 until json.getJSONArray("relayHints").length()).map {
                    json.getJSONArray("relayHints").getString(it)
                },
                members = members,
                epoch = json.optLong("epoch", 0),
                salt = if (saltStr.isNotEmpty()) android.util.Base64.decode(saltStr, android.util.Base64.NO_WRAP)
                       else SEPCommitmentBuilder.generateSalt(),
                commitment = if (commitmentStr.isNotEmpty()) android.util.Base64.decode(commitmentStr, android.util.Base64.NO_WRAP)
                            else null,
                tierRawValue = json.optInt("tierRawValue", SEPTier.LARGE.id),
                groupTypeRawValue = json.optInt("groupTypeRawValue", SEPGroupType.ANARCHY.id)
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
