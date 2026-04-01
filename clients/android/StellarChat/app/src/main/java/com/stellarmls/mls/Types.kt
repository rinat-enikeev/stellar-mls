package com.stellarmls.mls

/** Circuit tier — determines max members and Merkle tree depth. */
enum class SEPTier(val maxMembers: Int, val depth: Int, val id: Int) {
    SMALL(32, 5, 0),
    MEDIUM(256, 8, 1),
    LARGE(2048, 11, 2)
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
