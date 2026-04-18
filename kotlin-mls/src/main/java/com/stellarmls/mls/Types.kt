package com.stellarmls.mls

/** Circuit tier — determines max members and Merkle tree depth. */
enum class SEPTier(val maxMembers: Int, val depth: Int, val id: Int) {
    SMALL(32, 5, 0),
    MEDIUM(256, 8, 1),
    LARGE(2048, 11, 2);

    companion object {
        fun fromId(id: Int): SEPTier = when (id) {
            0 -> SMALL
            1 -> MEDIUM
            2 -> LARGE
            else -> throw IllegalArgumentException("unknown SEPTier id: $id")
        }
    }
}

/** Governance type selected at group creation. Mirrors the `group_type`
 *  u32 persisted in the contract's `CommitmentEntryV2`. See
 *  `docs/group-governance-types-design.md`. */
enum class SEPGroupType(val id: Int) {
    ANARCHY(0),
    ONE_ON_ONE(1),
    DEMOCRACY(2),
    OLIGARCHY(3);

    companion object {
        fun fromId(id: Int): SEPGroupType = when (id) {
            0 -> ANARCHY
            1 -> ONE_ON_ONE
            2 -> DEMOCRACY
            3 -> OLIGARCHY
            else -> throw IllegalArgumentException("unknown SEPGroupType id: $id")
        }
    }
}

/** A group member's BLS12-381 compressed public key + Poseidon leaf hash. */
data class SEPGroupMemberLeaf(
    val publicKeyCompressed: ByteArray, // 48 bytes
    val leafHash: ByteArray             // 32 bytes
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SEPGroupMemberLeaf) return false
        return publicKeyCompressed.contentEquals(other.publicKeyCompressed)
    }
    override fun hashCode() = publicKeyCompressed.contentHashCode()
}

/** Public inputs for a ZK proof — commitment + epoch. */
data class SEPPublicInputs(
    val commitment: ByteArray,
    val epoch: Long
)

/** Proof bundle: compressed proof + public inputs. */
data class SEPMembershipProofBundle(
    val proof: ByteArray,
    val publicInputs: SEPPublicInputs
)

/** Uncompressed proof components for on-chain verification. */
data class SEPContractProofComponents(
    val proofA: ByteArray,  // 96 bytes (G1 uncompressed)
    val proofB: ByteArray,  // 192 bytes (G2 uncompressed)
    val proofC: ByteArray   // 96 bytes (G1 uncompressed)
)

/**
 * Public inputs for the UpdateCircuit (#59 fix). The contract binds `cNew`
 * cryptographically via the proof, so the relayer no longer accepts a
 * client-supplied `new_commitment`.
 */
data class SEPUpdatePublicInputs(
    val cOld: ByteArray,   // 32 bytes
    val epochOld: Long,
    val cNew: ByteArray    // 32 bytes
)

/** UpdateCircuit proof bundle: compressed proof + update public inputs. */
data class SEPUpdateProofBundle(
    val proof: ByteArray,
    val publicInputs: SEPUpdatePublicInputs
)

/** On-chain state returned by `get_state_v2` — extends the V1 entry with
 *  governance-type metadata. Legacy groups are projected to `ANARCHY`
 *  / `memberCount = 0` by the contract. */
data class SEPCommitmentEntryV2(
    val commitment: ByteArray,
    val epoch: Long,
    val timestamp: Long,
    val tier: Int,
    val active: Boolean,
    val groupType: SEPGroupType,
    val memberCount: Int
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SEPCommitmentEntryV2) return false
        return commitment.contentEquals(other.commitment) && epoch == other.epoch &&
            timestamp == other.timestamp && tier == other.tier && active == other.active &&
            groupType == other.groupType && memberCount == other.memberCount
    }
    override fun hashCode(): Int {
        var h = commitment.contentHashCode()
        h = 31 * h + epoch.hashCode()
        h = 31 * h + tier
        h = 31 * h + groupType.hashCode()
        h = 31 * h + memberCount
        return h
    }
}
