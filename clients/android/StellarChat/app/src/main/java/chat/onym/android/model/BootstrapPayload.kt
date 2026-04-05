package chat.onym.android.model

import chat.onym.android.crypto.KeyAttestation
import com.stellarmls.mls.SEPCommitmentBuilder
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPMemberTransportBundle
import com.stellarmls.mls.SEPTier
import org.json.JSONArray
import org.json.JSONObject
import java.util.Date

/**
 * NIP-XX bootstrap payload — the decrypted content of a SEP invitation event
 * (kind 34113 in the current transport, with legacy 24113 receive support).
 * Contains everything a recipient needs to join the group.
 */
data class BootstrapPayload(
    val groupID: ByteArray,           // 32 bytes
    val groupSecret: ByteArray,       // 32 bytes
    val name: String,
    val epoch: Long,
    val members: List<SEPGroupMemberLeaf>,
    val relayHints: List<String>,
    val salt: ByteArray,              // 32 bytes
    val tierRawValue: Int,
    val commitment: ByteArray?,
    val senderNostrPubkey: String,
    val senderAttestation: KeyAttestation? = null,
    val senderTransportBundle: SEPMemberTransportBundle? = null
) {
    /** Convert to a ChatGroup for local storage.
     *  Members are re-sorted by compressed G1 key per SEP-XXXX §2.1
     *  to guard against out-of-order payloads. */
    fun toChatGroup(): ChatGroup {
        val groupIDHex = groupID.toHex()
        val sortedMembers = members.sortedWith { a, b ->
            val aKey = a.publicKeyCompressed
            val bKey = b.publicKeyCompressed
            for (i in 0 until minOf(aKey.size, bKey.size)) {
                val cmp = (aKey[i].toInt() and 0xFF) - (bKey[i].toInt() and 0xFF)
                if (cmp != 0) return@sortedWith cmp
            }
            aKey.size - bKey.size
        }
        val tier = SEPTier.entries.find { it.id == tierRawValue } ?: SEPTier.LARGE
        return ChatGroup(
            id = groupIDHex,
            name = name,
            groupSecret = groupSecret,
            createdAt = Date(),
            relayHints = relayHints,
            members = sortedMembers.toMutableList(),
            epoch = epoch,
            salt = salt,
            commitment = commitment,
            tier = tier
        )
    }

    fun toJson(): String {
        val obj = JSONObject()
        obj.put("groupID", android.util.Base64.encodeToString(groupID, android.util.Base64.NO_WRAP))
        obj.put("groupSecret", android.util.Base64.encodeToString(groupSecret, android.util.Base64.NO_WRAP))
        obj.put("name", name)
        obj.put("epoch", epoch)
        obj.put("relayHints", JSONArray(relayHints))
        obj.put("salt", android.util.Base64.encodeToString(salt, android.util.Base64.NO_WRAP))
        obj.put("tierRawValue", tierRawValue)
        obj.put("senderNostrPubkey", senderNostrPubkey)
        if (commitment != null) {
            obj.put("commitment", android.util.Base64.encodeToString(commitment, android.util.Base64.NO_WRAP))
        }
        val membersArr = JSONArray()
        for (m in members) {
            val mObj = JSONObject()
            mObj.put("publicKeyCompressed", android.util.Base64.encodeToString(m.publicKeyCompressed, android.util.Base64.NO_WRAP))
            mObj.put("leafHash", android.util.Base64.encodeToString(m.leafHash, android.util.Base64.NO_WRAP))
            membersArr.put(mObj)
        }
        obj.put("members", membersArr)
        if (senderAttestation != null) {
            val attObj = JSONObject()
            attObj.put("blsPubkey", android.util.Base64.encodeToString(senderAttestation.blsPubkey, android.util.Base64.NO_WRAP))
            attObj.put("ed25519Pubkey", android.util.Base64.encodeToString(senderAttestation.ed25519Pubkey, android.util.Base64.NO_WRAP))
            attObj.put("signature", android.util.Base64.encodeToString(senderAttestation.signature, android.util.Base64.NO_WRAP))
            obj.put("senderAttestation", attObj)
        }
        if (senderTransportBundle != null) {
            obj.put("senderTransportBundle", serializeTransportBundle(senderTransportBundle))
        }
        return obj.toString()
    }

    companion object {
        /** Build a bootstrap payload from a ChatGroup, optionally including attestation and transport bundle. */
        fun from(
            group: ChatGroup,
            senderPubkey: String,
            attestation: KeyAttestation? = null,
            transportBundle: SEPMemberTransportBundle? = null
        ): BootstrapPayload {
            return BootstrapPayload(
                groupID = group.groupIDData,
                groupSecret = group.groupSecret,
                name = group.name,
                epoch = group.epoch,
                members = group.members.toList(),
                relayHints = group.relayHints,
                salt = group.salt,
                tierRawValue = group.tier.id,
                commitment = group.commitment,
                senderNostrPubkey = senderPubkey,
                senderAttestation = attestation,
                senderTransportBundle = transportBundle
            )
        }

        fun serializeTransportBundle(bundle: SEPMemberTransportBundle): JSONObject {
            return JSONObject().apply {
                put("blsPubkey", android.util.Base64.encodeToString(bundle.blsPubkey, android.util.Base64.NO_WRAP))
                put("stellarEd25519Pubkey", android.util.Base64.encodeToString(bundle.stellarEd25519Pubkey, android.util.Base64.NO_WRAP))
                put("x25519InboxPubkey", android.util.Base64.encodeToString(bundle.x25519InboxPubkey, android.util.Base64.NO_WRAP))
                put("version", bundle.version)
                put("signature", android.util.Base64.encodeToString(bundle.signature, android.util.Base64.NO_WRAP))
            }
        }

        fun deserializeTransportBundle(obj: JSONObject): SEPMemberTransportBundle {
            return SEPMemberTransportBundle(
                blsPubkey = android.util.Base64.decode(obj.getString("blsPubkey"), android.util.Base64.NO_WRAP),
                stellarEd25519Pubkey = android.util.Base64.decode(obj.getString("stellarEd25519Pubkey"), android.util.Base64.NO_WRAP),
                x25519InboxPubkey = android.util.Base64.decode(obj.getString("x25519InboxPubkey"), android.util.Base64.NO_WRAP),
                version = obj.getInt("version"),
                signature = android.util.Base64.decode(obj.getString("signature"), android.util.Base64.NO_WRAP)
            )
        }

        fun fromJson(json: String): BootstrapPayload {
            val obj = JSONObject(json)
            val membersArr = obj.getJSONArray("members")
            val members = (0 until membersArr.length()).map { i ->
                val mObj = membersArr.getJSONObject(i)
                SEPGroupMemberLeaf(
                    publicKeyCompressed = android.util.Base64.decode(mObj.getString("publicKeyCompressed"), android.util.Base64.NO_WRAP),
                    leafHash = android.util.Base64.decode(mObj.getString("leafHash"), android.util.Base64.NO_WRAP)
                )
            }
            val relayArr = obj.getJSONArray("relayHints")
            val relayHints = (0 until relayArr.length()).map { relayArr.getString(it) }
            val commitmentStr = obj.optString("commitment", "")
            val commitment = if (commitmentStr.isNotEmpty()) {
                android.util.Base64.decode(commitmentStr, android.util.Base64.NO_WRAP)
            } else null

            val attestation = if (obj.has("senderAttestation")) {
                val attObj = obj.getJSONObject("senderAttestation")
                KeyAttestation(
                    blsPubkey = android.util.Base64.decode(attObj.getString("blsPubkey"), android.util.Base64.NO_WRAP),
                    ed25519Pubkey = android.util.Base64.decode(attObj.getString("ed25519Pubkey"), android.util.Base64.NO_WRAP),
                    signature = android.util.Base64.decode(attObj.getString("signature"), android.util.Base64.NO_WRAP)
                )
            } else null

            val transportBundle = if (obj.has("senderTransportBundle")) {
                deserializeTransportBundle(obj.getJSONObject("senderTransportBundle"))
            } else null

            return BootstrapPayload(
                groupID = android.util.Base64.decode(obj.getString("groupID"), android.util.Base64.NO_WRAP),
                groupSecret = android.util.Base64.decode(obj.getString("groupSecret"), android.util.Base64.NO_WRAP),
                name = obj.getString("name"),
                epoch = obj.getLong("epoch"),
                members = members,
                relayHints = relayHints,
                salt = android.util.Base64.decode(obj.getString("salt"), android.util.Base64.NO_WRAP),
                tierRawValue = obj.getInt("tierRawValue"),
                commitment = commitment,
                senderNostrPubkey = obj.getString("senderNostrPubkey"),
                senderAttestation = attestation,
                senderTransportBundle = transportBundle
            )
        }
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is BootstrapPayload) return false
        return groupID.contentEquals(other.groupID) && name == other.name
    }

    override fun hashCode(): Int = groupID.contentHashCode()
}

/** Represents a received invitation before the user accepts it. */
data class PendingInvitation(
    val id: String,                  // Nostr event ID
    val payload: BootstrapPayload,
    val receivedAt: Date
)
