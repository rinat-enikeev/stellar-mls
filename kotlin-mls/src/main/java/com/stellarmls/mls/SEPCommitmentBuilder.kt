package com.stellarmls.mls

import java.security.SecureRandom

/** Merkle tree and commitment operations backed by the Rust FFI. */
object SEPCommitmentBuilder {

    /** Generate a random 32-byte salt. */
    fun generateSalt(): ByteArray {
        val salt = ByteArray(32)
        SecureRandom().nextBytes(salt)
        return salt
    }

    /** Deterministic salt derivation for member-join events.
     *  All members processing the same SEPMemberJoined will derive the same salt,
     *  preventing epoch/key divergence. */
    fun deriveSalt(previousSalt: ByteArray, memberKey: ByteArray): ByteArray {
        val digest = java.security.MessageDigest.getInstance("SHA-256")
        digest.update(previousSalt)
        digest.update(memberKey)
        return digest.digest()
    }

    /** Compute the Poseidon leaf hash for a 32-byte secret key. Returns 32 bytes. */
    fun computeLeafHash(secretKey: ByteArray): ByteArray {
        require(secretKey.size == 32) { "secret key must be 32 bytes" }
        return RustBridge.computeLeafHash(secretKey)
    }

    /** Compute BLS12-381 compressed public key from a 32-byte secret key. Returns 48 bytes. */
    fun computePublicKey(secretKey: ByteArray): ByteArray {
        require(secretKey.size == 32) { "secret key must be 32 bytes" }
        return RustBridge.computePublicKey(secretKey)
    }

    /** Compute the Poseidon Merkle root from a list of members. Returns 32 bytes. */
    fun computeMerkleRoot(members: List<SEPGroupMemberLeaf>, tier: SEPTier): ByteArray {
        val (flatPk, flatLh) = flattenMembers(members)
        return RustBridge.computeMerkleRoot(flatPk, flatLh, tier.depth)
    }

    /** SHA256(poseidon_root || epoch_be || salt). Returns 32 bytes. */
    fun computeSHA256Commitment(poseidonRoot: ByteArray, epoch: Long, salt: ByteArray): ByteArray {
        require(poseidonRoot.size == 32) { "poseidon root must be 32 bytes" }
        require(salt.size == 32) { "salt must be 32 bytes" }
        return RustBridge.computeSha256Commitment(poseidonRoot, epoch, salt)
    }

    /** Poseidon(poseidon_root, epoch, salt). Returns 32 bytes. */
    fun computePoseidonCommitment(poseidonRoot: ByteArray, epoch: Long, salt: ByteArray): ByteArray {
        require(poseidonRoot.size == 32) { "poseidon root must be 32 bytes" }
        require(salt.size == 32) { "salt must be 32 bytes" }
        return RustBridge.computePoseidonCommitment(poseidonRoot, epoch, salt)
    }

    /**
     * Seed for the Oligarchy admin tree at creation (§6.2.2):
     *   admin_commitment = Poseidon( Poseidon(admin_root, admin_epoch), admin_salt )
     * where `admin_root` is the Poseidon Merkle root over the admin leaves
     * and `admin_epoch = 0` at group birth. The admin tree is capped at 32
     * entries (small tier, depth = 5).
     */
    fun computeAdminCommitment(admins: List<SEPGroupMemberLeaf>, salt: ByteArray): ByteArray {
        val adminRoot = computeMerkleRoot(admins, SEPTier.SMALL)
        return computePoseidonCommitment(adminRoot, 0L, salt)
    }

    /** Flatten member list into concatenated byte buffers for FFI. */
    internal fun flattenMembers(members: List<SEPGroupMemberLeaf>): Pair<ByteArray, ByteArray> {
        val pkBuf = ByteArray(members.size * 48)
        val lhBuf = ByteArray(members.size * 32)
        for ((i, m) in members.withIndex()) {
            require(m.publicKeyCompressed.size == 48) { "public key at index $i must be 48 bytes" }
            require(m.leafHash.size == 32) { "leaf hash at index $i must be 32 bytes" }
            m.publicKeyCompressed.copyInto(pkBuf, i * 48)
            m.leafHash.copyInto(lhBuf, i * 32)
        }
        return pkBuf to lhBuf
    }
}
