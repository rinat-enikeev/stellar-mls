package com.stellarmls.chat.onchain

import com.stellarmls.mls.SEPCommitmentBuilder
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPMembershipProofBundle
import com.stellarmls.mls.SEPProofGenerator
import com.stellarmls.mls.SEPTier

/**
 * Orchestrates ZK proof generation and Soroban contract interaction.
 *
 * Uses [SEPContractClient] for HTTP-based contract invocations and
 * [SEPProofGenerator] for Groth16 proof generation.
 */
class OnChainService(contractID: String, transport: SEPContractTransport) {

    val contractClient = SEPContractClient(
        contractID = contractID,
        transport = transport
    )

    /** Direct endpoint constructor. */
    constructor(contractID: String, endpoint: String) : this(
        contractID, OkHttpSEPContractTransport(endpoint)
    )

    /** Relayer transport constructor for fee-decoupled submission. */
    constructor(contractID: String, relayerURL: String, authToken: String?) : this(
        contractID, OkHttpRelayerTransport(relayerURL, authToken)
    )

    /** Cached proving keys per tier (generated on first use). */
    private val provingKeys = mutableMapOf<SEPTier, ByteArray>()

    // -- Proving Key Management --

    fun ensureProvingKey(tier: SEPTier): ByteArray {
        return provingKeys.getOrPut(tier) {
            SEPProofGenerator.generateTestingProvingKey(tier)
        }
    }

    // -- Proof Generation --

    fun generateProof(
        members: List<SEPGroupMemberLeaf>,
        blsSecretKey: ByteArray,
        epoch: Long,
        salt: ByteArray,
        tier: SEPTier
    ): SEPMembershipProofBundle {
        val pk = ensureProvingKey(tier)
        return SEPProofGenerator.generateMembershipProof(
            provingKey = pk,
            members = members,
            secretKey = blsSecretKey,
            epoch = epoch,
            salt = salt,
            tier = tier
        )
    }

    // -- Proof Format Conversion --

    /**
     * Convert a compressed Groth16 proof (192 bytes) to the uncompressed
     * contract format (384 bytes = proofA||proofB||proofC).
     */
    private fun proofForContract(compressedProof: ByteArray): ByteArray {
        val components = SEPProofGenerator.proofToContractFormat(compressedProof)
        // A (96) || B (192) || C (96) = 384 bytes
        return components.proofA + components.proofB + components.proofC
    }

    // -- Contract Operations --

    /**
     * Publish a new group creation on-chain.
     *
     * 1. Generates a membership proof at the initial state.
     * 2. Decompresses the proof to uncompressed BLS12-381 points (384 bytes).
     * 3. Submits create_group to the Soroban contract.
     */
    fun publishGroupCreation(
        groupIDData: ByteArray,
        members: List<SEPGroupMemberLeaf>,
        blsSecretKey: ByteArray,
        epoch: Long,
        salt: ByteArray,
        tier: SEPTier,
        callerAddress: String
    ): SEPSubmissionResponse {
        val proofBundle = generateProof(members, blsSecretKey, epoch, salt, tier)
        val uncompressedProof = proofForContract(proofBundle.proof)

        return contractClient.createGroup(
            caller = callerAddress,
            groupID = groupIDData,
            commitment = proofBundle.publicInputs.commitment,
            proof = uncompressedProof,
            publicInputsCommitment = proofBundle.publicInputs.commitment,
            epoch = epoch,
            tier = tier.id
        )
    }

    /**
     * Publish a commitment update after a membership change.
     *
     * 1. Generates a membership proof at the OLD state (proving current membership).
     * 2. Decompresses the proof to uncompressed format (384 bytes).
     * 3. Computes the new Poseidon commitment from the new state.
     * 4. Submits update_commitment to the Soroban contract.
     */
    fun publishCommitmentUpdate(
        groupIDData: ByteArray,
        oldMembers: List<SEPGroupMemberLeaf>,
        oldEpoch: Long,
        oldSalt: ByteArray,
        newMembers: List<SEPGroupMemberLeaf>,
        newEpoch: Long,
        newSalt: ByteArray,
        blsSecretKey: ByteArray,
        tier: SEPTier
    ): SEPSubmissionResponse {
        // Proof against OLD (current on-chain) state
        val oldProofBundle = generateProof(oldMembers, blsSecretKey, oldEpoch, oldSalt, tier)
        val uncompressedProof = proofForContract(oldProofBundle.proof)

        // Compute new Poseidon commitment
        val newRoot = SEPCommitmentBuilder.computeMerkleRoot(newMembers, tier)
        val newPoseidonCommitment = SEPCommitmentBuilder.computePoseidonCommitment(
            newRoot, newEpoch, newSalt
        )

        return contractClient.updateCommitment(
            groupID = groupIDData,
            newCommitment = newPoseidonCommitment,
            newEpoch = newEpoch,
            proof = uncompressedProof,
            oldCommitment = oldProofBundle.publicInputs.commitment,
            oldEpoch = oldEpoch
        )
    }

    /** Fetch the current on-chain state for a group. */
    fun fetchOnChainState(groupIDData: ByteArray): SEPCommitmentEntry {
        return contractClient.getState(groupIDData)
    }

    // -- Verification --

    /**
     * Verify that a group's local state matches its on-chain commitment.
     *
     * Computes the Poseidon commitment from local (members, epoch, salt)
     * and compares it against the on-chain stored commitment.
     */
    fun verifyCommitment(
        groupIDData: ByteArray,
        members: List<SEPGroupMemberLeaf>,
        epoch: Long,
        salt: ByteArray,
        tier: SEPTier
    ): OnChainVerificationResult {
        return try {
            val entry = fetchOnChainState(groupIDData)

            if (!entry.active) return OnChainVerificationResult.Inactive

            if (entry.epoch != epoch) {
                return OnChainVerificationResult.EpochMismatch(local = epoch, onChain = entry.epoch)
            }

            val root = SEPCommitmentBuilder.computeMerkleRoot(members, tier)
            val localPoseidonCommitment = SEPCommitmentBuilder.computePoseidonCommitment(
                root, epoch, salt
            )

            if (localPoseidonCommitment.contentEquals(entry.commitment)) {
                OnChainVerificationResult.Verified
            } else {
                OnChainVerificationResult.CommitmentMismatch
            }
        } catch (e: Exception) {
            val message = e.message ?: e.toString()
            if (message.contains("404") || message.contains("not found") ||
                message.contains("GroupNotFound")
            ) {
                OnChainVerificationResult.NotPublished
            } else {
                OnChainVerificationResult.Error(message)
            }
        }
    }

    /**
     * Deactivate a group on-chain. Requires a valid ZK proof of membership.
     * Any authorized member can deactivate. The contract sets active = false.
     */
    fun deactivateGroup(
        groupIDData: ByteArray,
        members: List<SEPGroupMemberLeaf>,
        blsSecretKey: ByteArray,
        epoch: Long,
        salt: ByteArray,
        tier: SEPTier
    ): SEPSubmissionResponse {
        val proofBundle = generateProof(members, blsSecretKey, epoch, salt, tier)
        val uncompressedProof = proofForContract(proofBundle.proof)

        return contractClient.deactivateGroup(
            groupID = groupIDData,
            proof = uncompressedProof,
            commitment = proofBundle.publicInputs.commitment,
            epoch = epoch
        )
    }

    /**
     * Verify membership via the on-chain contract (read-only).
     *
     * Generates a proof, decompresses to 384-byte contract format,
     * and submits verify_membership to the contract.
     */
    fun verifyMembership(
        groupIDData: ByteArray,
        members: List<SEPGroupMemberLeaf>,
        blsSecretKey: ByteArray,
        epoch: Long,
        salt: ByteArray,
        tier: SEPTier
    ): Boolean {
        val proofBundle = generateProof(members, blsSecretKey, epoch, salt, tier)
        val uncompressedProof = proofForContract(proofBundle.proof)

        val response = contractClient.verifyMembership(
            groupID = groupIDData,
            proof = uncompressedProof,
            commitment = proofBundle.publicInputs.commitment,
            epoch = epoch
        )
        return response.valid
    }
}
