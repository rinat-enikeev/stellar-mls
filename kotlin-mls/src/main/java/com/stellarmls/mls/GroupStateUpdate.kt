package com.stellarmls.mls

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
    val senderAttestation: SEPKeyAttestationPayload? = null
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
    val member: SEPGroupMemberLeaf
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
