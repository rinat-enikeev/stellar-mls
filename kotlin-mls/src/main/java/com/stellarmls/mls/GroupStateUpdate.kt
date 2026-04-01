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
 * Configuration for fee-decoupled transaction submission via a relayer.
 *
 * The relayer receives the same contract invocation payload but wraps it
 * in a Stellar transaction signed by its own key, paying the network fee.
 */
data class SEPRelayerConfig(
    val relayerURL: String,
    val authToken: String? = null
)
