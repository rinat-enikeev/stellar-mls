package com.stellarmls.mls

/**
 * JNI bridge to the Rust sep-xxxx-circuits library.
 * Each method maps to a `Java_com_stellarmls_mls_RustBridge_*` JNI export.
 */
internal object RustBridge {
    init {
        System.loadLibrary("sep_xxxx_circuits")
    }

    /** Compute Poseidon leaf hash from a 32-byte secret key. Returns 32 bytes. */
    external fun computeLeafHash(secretKey: ByteArray): ByteArray

    /** Compute BLS12-381 compressed public key from 32-byte secret key. Returns 48 bytes. */
    external fun computePublicKey(secretKey: ByteArray): ByteArray

    /** Compute Poseidon Merkle root. publicKeys: concatenated 48-byte keys. leafHashes: concatenated 32-byte hashes. */
    external fun computeMerkleRoot(memberPublicKeys: ByteArray, leafHashes: ByteArray, depth: Int): ByteArray

    /** Derive secp256k1 x-only public key from 32-byte secret key. Returns 32 bytes. */
    external fun nostrDerivePublicKey(secretKey: ByteArray): ByteArray

    /** Sign a 32-byte event ID with secp256k1 Schnorr. Returns 64-byte signature. */
    external fun nostrSignEventId(secretKey: ByteArray, eventId: ByteArray): ByteArray

    /** SHA256(poseidon_root || epoch_be || salt). Returns 32 bytes. */
    external fun computeSha256Commitment(poseidonRoot: ByteArray, epoch: Long, salt: ByteArray): ByteArray

    /** Poseidon(poseidon_root, epoch, salt). Returns 32 bytes. */
    external fun computePoseidonCommitment(poseidonRoot: ByteArray, epoch: Long, salt: ByteArray): ByteArray

    /** Generate a testing proving key for the given depth and seed. Returns serialized key. */
    external fun generateTestingProvingKey(depth: Int, seed: Long): ByteArray

    /**
     * Generate membership proof. Returns packed: [4B proof_len][proof][4B commitment_len][commitment].
     * Use [SEPProofGenerator.unpackProofBundle] to split.
     */
    external fun generateMembershipProof(
        provingKey: ByteArray,
        memberPublicKeys: ByteArray,
        leafHashes: ByteArray,
        secretKey: ByteArray,
        epoch: Long,
        salt: ByteArray,
        depth: Int
    ): ByteArray

    /**
     * Decompress proof to contract format. Returns packed: [4B a_len][a][4B b_len][b][4B c_len][c].
     * Use [SEPProofGenerator.unpackContractComponents] to split.
     */
    external fun proofToContractFormat(compressedProof: ByteArray): ByteArray
}
