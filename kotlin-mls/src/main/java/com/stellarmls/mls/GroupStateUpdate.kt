package com.stellarmls.mls

// MARK: - Protocol Message Type Registry
//
// CHECKLIST: When adding a new protocol message type:
// 1. Add it to SEPMessageType below
// 2. Define the data class in this file
// 3. Add to isProtocolMessage on both platforms (iOS NostrMessageTransport, Android NostrMessageTransport)
// 4. If it modifies membership: add to applyMemberChanges on BOTH platforms
// 5. Add dispatch case in the main protocol message handler on both platforms
// 6. Add cross-platform test vector in docs/cross-platform-test-vectors.json

/**
 * Exhaustive registry of all SEP protocol message types.
 * Every MESSAGE_TYPE constant in this file MUST have a corresponding entry here.
 */
enum class SEPMessageType(val wireValue: String, val modifiesMembership: Boolean) {
    STATE_UPDATE("sep_state_update", modifiesMembership = true),
    MEMBER_JOINED("sep_member_joined", modifiesMembership = true),
    SALT_REQUEST("sep_salt_request", modifiesMembership = false),
    SALT_RESPONSE("sep_salt_response", modifiesMembership = false),
    GROUP_RENAMED("sep_group_renamed", modifiesMembership = false),
    REKEY("sep_rekey", modifiesMembership = false),
    MESSAGE_ACK("sep_message_ack", modifiesMembership = false),
    REMOVAL_NOTICE("sep_removal_notice", modifiesMembership = true),
    REKEY_ENVELOPE("sep_rekey_envelope", modifiesMembership = false),
    REKEY_ACK("sep_rekey_ack", modifiesMembership = false),
    REKEY_RESEND_REQUEST("sep_rekey_resend_request", modifiesMembership = false);

    companion object {
        private val byWireValue = entries.associateBy { it.wireValue }
        fun fromWireValue(value: String): SEPMessageType? = byWireValue[value]

        /** All message types that require applyMemberChanges handling. */
        val membershipModifying: Set<SEPMessageType> = entries.filter { it.modifiesMembership }.toSet()
    }
}

// MARK: - Group State Update Protocol Messages

/**
 * Distributed via the encrypted group channel (kind 24114) after a membership change.
 * Contains the new epoch, salt, and member delta so all members can update their local state.
 */
data class SEPGroupStateUpdate(
    val type: String = MESSAGE_TYPE,
    val epoch: Long,
    val salt: ByteArray,
    val addedMembers: List<SEPGroupMemberLeaf> = emptyList(),
    val removedMemberKeys: List<ByteArray> = emptyList(),
    val commitment: ByteArray? = null,
    val senderAttestation: SEPKeyAttestationPayload? = null,
    val senderTransportBundle: SEPMemberTransportBundle? = null,
    /** All known transport bundles for group members. Enables secure rekey
     *  by distributing bundles to members who joined after others. */
    val memberBundles: List<SEPMemberTransportBundle> = emptyList()
) {
    companion object {
        const val MESSAGE_TYPE = "sep_state_update"
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SEPGroupStateUpdate) return false
        return epoch == other.epoch && salt.contentEquals(other.salt)
    }

    override fun hashCode(): Int = 31 * epoch.hashCode() + salt.contentHashCode()
}

/**
 * Broadcast by a new joiner after joining via invite code.
 * Existing members receive this, add the joiner to their member list,
 * and respond with an [SEPGroupStateUpdate] containing the updated state.
 */
data class SEPMemberJoined(
    val type: String = MESSAGE_TYPE,
    val member: SEPGroupMemberLeaf,
    val transportBundle: SEPMemberTransportBundle? = null
) {
    companion object {
        const val MESSAGE_TYPE = "sep_member_joined"
    }
}

/**
 * Request a salt for a specific epoch from other online group members.
 * Sent via the encrypted group channel when a member discovers it missed an epoch.
 */
data class SEPSaltRequest(
    val type: String = MESSAGE_TYPE,
    val epoch: Long
) {
    companion object {
        const val MESSAGE_TYPE = "sep_salt_request"
    }
}

/**
 * Response to a salt request, providing the salt for the requested epoch.
 */
data class SEPSaltResponse(
    val type: String = MESSAGE_TYPE,
    val epoch: Long,
    val salt: ByteArray
) {
    companion object {
        const val MESSAGE_TYPE = "sep_salt_response"
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SEPSaltResponse) return false
        return epoch == other.epoch && salt.contentEquals(other.salt)
    }

    override fun hashCode(): Int = 31 * epoch.hashCode() + salt.contentHashCode()
}

/**
 * A key attestation payload suitable for wire transmission.
 * Binds a BLS12-381 group membership key to a Stellar Ed25519 account key.
 */
data class SEPKeyAttestationPayload(
    val blsPubkey: ByteArray,       // 48 bytes, compressed G1 point
    val ed25519Pubkey: ByteArray,   // 32 bytes, Stellar Ed25519 public key
    val signature: ByteArray        // 64 bytes, Ed25519 signature
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SEPKeyAttestationPayload) return false
        return blsPubkey.contentEquals(other.blsPubkey) &&
                ed25519Pubkey.contentEquals(other.ed25519Pubkey) &&
                signature.contentEquals(other.signature)
    }

    override fun hashCode(): Int {
        var result = blsPubkey.contentHashCode()
        result = 31 * result + ed25519Pubkey.contentHashCode()
        result = 31 * result + signature.contentHashCode()
        return result
    }
}

/**
 * Post-removal re-key message. Sent immediately after a member removal state update.
 * Contains the real salt (the state update carries a poisoned/placeholder salt so the
 * removed member cannot derive the new encryption key). Encrypted with the key derived
 * from the poisoned salt — only members still subscribed will receive and process it.
 */
data class SEPRekey(
    val type: String = MESSAGE_TYPE,
    val epoch: Long,
    val salt: ByteArray
) {
    companion object {
        const val MESSAGE_TYPE = "sep_rekey"
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SEPRekey) return false
        return epoch == other.epoch && salt.contentEquals(other.salt)
    }

    override fun hashCode(): Int = 31 * epoch.hashCode() + salt.contentHashCode()
}

/**
 * Delivery acknowledgment sent when a member receives and decrypts a message.
 * Each device sends at most one ACK per original event ID.
 */
data class SEPMessageAck(
    val type: String = MESSAGE_TYPE,
    val eventID: String
) {
    companion object {
        const val MESSAGE_TYPE = "sep_message_ack"
    }
}

/**
 * Broadcast when a member renames the group.
 */
data class SEPGroupRenamed(
    val type: String = MESSAGE_TYPE,
    val name: String
) {
    companion object {
        const val MESSAGE_TYPE = "sep_group_renamed"
    }
}

// MARK: - Authenticated Member Transport Bundle

/**
 * Authenticated mapping from a group member to their X25519 inbox encryption key.
 * Enables per-member asymmetric delivery of rekey material during removals.
 * The signature covers a domain-separated hash of all fields, signed by the member's
 * Stellar Ed25519 key, ensuring the inbox key cannot be spoofed.
 */
data class SEPMemberTransportBundle(
    val blsPubkey: ByteArray,            // 48 bytes, compressed G1
    val stellarEd25519Pubkey: ByteArray, // 32 bytes
    val x25519InboxPubkey: ByteArray,    // 32 bytes
    val version: Int = CURRENT_VERSION,  // u16
    val signature: ByteArray             // 64 bytes, Ed25519
) {
    companion object {
        const val CURRENT_VERSION = 1
    }

    val hasValidStructure: Boolean
        get() = blsPubkey.size == 48 &&
                stellarEd25519Pubkey.size == 32 &&
                x25519InboxPubkey.size == 32 &&
                signature.size == 64

    /** Domain-separated binding message:
     *  SHA-256("SEP-XXXX:member-transport-v1" || bls || stellar || x25519 || version) */
    fun computeBindingMessage(): ByteArray {
        val digest = java.security.MessageDigest.getInstance("SHA-256")
        digest.update("SEP-XXXX:member-transport-v1".toByteArray())
        digest.update(blsPubkey)
        digest.update(stellarEd25519Pubkey)
        digest.update(x25519InboxPubkey)
        digest.update(byteArrayOf((version shr 8).toByte(), version.toByte()))
        return digest.digest()
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SEPMemberTransportBundle) return false
        return blsPubkey.contentEquals(other.blsPubkey) &&
                stellarEd25519Pubkey.contentEquals(other.stellarEd25519Pubkey) &&
                x25519InboxPubkey.contentEquals(other.x25519InboxPubkey) &&
                version == other.version &&
                signature.contentEquals(other.signature)
    }

    override fun hashCode(): Int {
        var result = blsPubkey.contentHashCode()
        result = 31 * result + stellarEd25519Pubkey.contentHashCode()
        result = 31 * result + x25519InboxPubkey.contentHashCode()
        result = 31 * result + version
        result = 31 * result + signature.contentHashCode()
        return result
    }
}

/**
 * Non-secret notice broadcast on the old group channel during a secure removal.
 * Does NOT contain new groupSecret or salt — only metadata.
 */
data class SEPRemovalNotice(
    val type: String = MESSAGE_TYPE,
    val groupID: ByteArray,
    val epoch: Long,
    val removedMemberKeys: List<ByteArray>,
    val commitment: ByteArray? = null
) {
    companion object {
        const val MESSAGE_TYPE = "sep_removal_notice"
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SEPRemovalNotice) return false
        return groupID.contentEquals(other.groupID) && epoch == other.epoch
    }

    override fun hashCode(): Int = 31 * groupID.contentHashCode() + epoch.hashCode()
}

/**
 * Per-member rekey envelope delivered via X25519 inbox transport.
 * Contains fresh groupSecret and salt that the removed member never receives.
 */
data class SEPRekeyEnvelope(
    val type: String = MESSAGE_TYPE,
    val groupID: ByteArray,           // 32 bytes
    val epoch: Long,
    val groupSecret: ByteArray,       // 32 bytes — the NEW group secret
    val salt: ByteArray,              // 32 bytes — the NEW salt
    val commitment: ByteArray? = null,
    val memberBundles: List<SEPMemberTransportBundle>,
    val removedMemberKeys: List<ByteArray>,
    val relayHints: List<String>,
    val senderBundle: SEPMemberTransportBundle
) {
    companion object {
        const val MESSAGE_TYPE = "sep_rekey_envelope"
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SEPRekeyEnvelope) return false
        return groupID.contentEquals(other.groupID) && epoch == other.epoch
    }

    override fun hashCode(): Int = 31 * groupID.contentHashCode() + epoch.hashCode()
}

/**
 * Acknowledgment that a member received and installed a rekey envelope.
 */
data class SEPRekeyAck(
    val type: String = MESSAGE_TYPE,
    val groupID: ByteArray,
    val epoch: Long,
    val senderBlsPubkey: ByteArray
) {
    companion object {
        const val MESSAGE_TYPE = "sep_rekey_ack"
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SEPRekeyAck) return false
        return groupID.contentEquals(other.groupID) && epoch == other.epoch
    }

    override fun hashCode(): Int = 31 * groupID.contentHashCode() + epoch.hashCode()
}

/**
 * Request for a rekey envelope resend (sent via inbox when a member misses the original).
 */
data class SEPRekeyResendRequest(
    val type: String = MESSAGE_TYPE,
    val groupID: ByteArray,
    val epoch: Long,
    val requesterBundle: SEPMemberTransportBundle
) {
    companion object {
        const val MESSAGE_TYPE = "sep_rekey_resend_request"
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SEPRekeyResendRequest) return false
        return groupID.contentEquals(other.groupID) && epoch == other.epoch
    }

    override fun hashCode(): Int = 31 * groupID.contentHashCode() + epoch.hashCode()
}

/**
 * Configuration for fee-decoupled transaction submission via a relayer.
 *
 * The relayer receives the same contract invocation payload but wraps it
 * in a Stellar transaction signed by its own key, paying the network fee.
 */
data class SEPRelayerConfig(
    val relayerURL: String,
    val authToken: String? = null
)
