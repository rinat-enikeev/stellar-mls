package chat.onym.android.onchain

import android.content.Context
import android.util.Log
import chat.onym.android.BuildConfig
import com.stellarmls.mls.SEPCommitmentBuilder
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPGroupType
import com.stellarmls.mls.SEPMembershipProofBundle
import com.stellarmls.mls.SEPProofGenerator
import com.stellarmls.mls.SEPTier
import java.security.MessageDigest

/**
 * Orchestrates ZK proof generation and Soroban contract interaction.
 *
 * Uses [SEPContractClient] for HTTP-based contract invocations and
 * [SEPProofGenerator] for Groth16 proof generation.
 *
 * M-10: Operations that fail due to network errors are retried with exponential
 * backoff (up to 3 attempts). Groups continue to function in "unverified" mode
 * when the contract endpoint is unreachable.
 */
class OnChainService(private val context: Context, contractID: String, transport: SEPContractTransport) {

    val contractClient = SEPContractClient(
        contractID = contractID,
        transport = transport
    )

    /** Direct endpoint constructor. */
    constructor(context: Context, contractID: String, endpoint: String) : this(
        context, contractID, OkHttpSEPContractTransport(endpoint)
    )

    /** Relayer transport constructor for fee-decoupled submission. */
    constructor(context: Context, contractID: String, relayerURL: String, authToken: String?) : this(
        context, contractID, OkHttpRelayerTransport(relayerURL, authToken)
    )

    /** Cached proving keys per tier (loaded from assets on first use). */
    private val provingKeys = mutableMapOf<SEPTier, ByteArray>()

    /**
     * Cached UpdateCircuit proving keys per tier. #59 fix: distinct circuit
     * from the membership circuit, bundled in keyset-v2.
     */
    private val updateProvingKeys = mutableMapOf<SEPTier, ByteArray>()

    /** Execute a block with exponential backoff retry on IOException. */
    private fun <T> withRetry(block: () -> T): T {
        var lastException: Exception? = null
        for (attempt in 0 until MAX_RETRIES) {
            try {
                return block()
            } catch (e: java.io.IOException) {
                lastException = e
                if (attempt < MAX_RETRIES - 1) {
                    Thread.sleep(BASE_RETRY_DELAY_MS * (1L shl attempt))
                }
            }
        }
        throw lastException!!
    }

    // -- Proving Key Management --

    companion object {
        private const val TAG = "OnChainService"
        private const val MAX_RETRIES = 3
        private const val BASE_RETRY_DELAY_MS = 1000L

        /** Current keyset version. Must match the assets in keyset-vN/. */
        const val KEYSET_VERSION = 2

        /** Expected SHA-256 hashes of membership proving keys per tier (keyset-v2). */
        val PROVING_KEY_HASHES = mapOf(
            SEPTier.SMALL to "88ae981de7f91d79caad9cdb9076dd990980beab575aba461777aae592ff599d",
            SEPTier.MEDIUM to "206400e7d1e74f6cbe96ebdab989a3e79090bdd042ddf19dcc1dee02277c0cf0",
            SEPTier.LARGE to "2daafd9ab533daea483fe433d18922b2c7a63058f2c166e298d17b843ed45d16",
        )

        /** Expected SHA-256 hashes of UpdateCircuit proving keys per tier (keyset-v2). */
        val UPDATE_PROVING_KEY_HASHES = mapOf(
            SEPTier.SMALL to "6b83e2300430bb1bd37b929ccb7d135f81d4f966745da661df100d38d1e8dbcc",
            SEPTier.MEDIUM to "ee468305f7566782448bc495f122ded42280299bad088c4f567b245205e6ed54",
            SEPTier.LARGE to "16ce9e2e712bfcc1c46bbd9d4274a9210bdcd13f7258294b96ccfd47a5f60900",
        )

        private fun tierName(tier: SEPTier): String = when (tier) {
            SEPTier.SMALL -> "small"
            SEPTier.MEDIUM -> "medium"
            SEPTier.LARGE -> "large"
        }

        private fun tierAssetName(tier: SEPTier): String =
            "keyset-v$KEYSET_VERSION/${tierName(tier)}.bin"

        private fun updateTierAssetName(tier: SEPTier): String =
            "keyset-v$KEYSET_VERSION/update-${tierName(tier)}.bin"
    }

    /** Load a proving key from bundled assets, verifying its hash. */
    fun ensureProvingKey(tier: SEPTier): ByteArray {
        return provingKeys.getOrPut(tier) {
            loadProvingKeyFromAssets(tier)
        }
    }

    /**
     * Load or return a cached UpdateCircuit proving key for the given tier.
     * Loads from `keyset-v<N>/update-<tier>.bin` with SHA-256 hash verification.
     */
    fun ensureUpdateProvingKey(tier: SEPTier): ByteArray {
        return updateProvingKeys.getOrPut(tier) {
            loadUpdateProvingKeyFromAssets(tier)
        }
    }

    private fun loadProvingKeyFromAssets(tier: SEPTier): ByteArray {
        val assetPath = tierAssetName(tier)
        val bytes = context.assets.open(assetPath).use { it.readBytes() }

        val expectedHash = PROVING_KEY_HASHES[tier]
        if (expectedHash != null) {
            val actualHash = MessageDigest.getInstance("SHA-256")
                .digest(bytes)
                .joinToString("") { "%02x".format(it) }
            require(actualHash == expectedHash) {
                "Proving key hash mismatch for $tier: expected $expectedHash, got $actualHash"
            }
        }

        Log.d(TAG, "Loaded proving key from assets: $assetPath (${bytes.size} bytes)")
        return bytes
    }

    private fun loadUpdateProvingKeyFromAssets(tier: SEPTier): ByteArray {
        val assetPath = updateTierAssetName(tier)
        val bytes = context.assets.open(assetPath).use { it.readBytes() }

        val expectedHash = UPDATE_PROVING_KEY_HASHES[tier]
        if (expectedHash != null) {
            val actualHash = MessageDigest.getInstance("SHA-256")
                .digest(bytes)
                .joinToString("") { "%02x".format(it) }
            require(actualHash == expectedHash) {
                "Update proving key hash mismatch for $tier: expected $expectedHash, got $actualHash"
            }
        }

        Log.d(TAG, "Loaded update proving key from assets: $assetPath (${bytes.size} bytes)")
        return bytes
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
     * 3. Submits the correct creation entrypoint for the requested governance
     *    type:
     *     - [SEPGroupType.ANARCHY]    → `create_group` (V1)
     *     - [SEPGroupType.ONE_ON_ONE],
     *       [SEPGroupType.DEMOCRACY]  → `create_group_v2`
     *     - [SEPGroupType.OLIGARCHY]  → `create_oligarchy_group` (requires
     *       [adminLeaves] + [adminSalt] to seed the admin tree)
     */
    fun publishGroupCreation(
        groupIDData: ByteArray,
        members: List<SEPGroupMemberLeaf>,
        blsSecretKey: ByteArray,
        epoch: Long,
        salt: ByteArray,
        tier: SEPTier,
        callerAddress: String,
        groupType: SEPGroupType = SEPGroupType.ANARCHY,
        memberCount: Int? = null,
        adminLeaves: List<SEPGroupMemberLeaf>? = null,
        adminSalt: ByteArray? = null
    ): SEPSubmissionResponse {
        val proofBundle = generateProof(members, blsSecretKey, epoch, salt, tier)
        val uncompressedProof = proofForContract(proofBundle.proof)
        val firstMember = members.firstOrNull()
        val effectiveMemberCount = memberCount ?: members.size

        if (BuildConfig.DEBUG) {
            Log.d(
                TAG,
                "publishGroupCreation group=${groupIDData.debugHexPrefix()} epoch=$epoch tier=${tier.id} " +
                    "groupType=${groupType.id} memberCount=$effectiveMemberCount " +
                    "members=${members.size} salt=${salt.debugHexPrefix(12)} " +
                    "proofCommitment=${proofBundle.publicInputs.commitment.debugHexPrefix(12)} " +
                    "firstPk=${firstMember?.publicKeyCompressed?.debugHexPrefix(12) ?: "none"} " +
                    "firstLeaf=${firstMember?.leafHash?.debugHexPrefix(12) ?: "none"}"
            )
        }

        return when (groupType) {
            SEPGroupType.ANARCHY -> withRetry {
                contractClient.createGroup(
                    caller = callerAddress,
                    groupID = groupIDData,
                    commitment = proofBundle.publicInputs.commitment,
                    proof = uncompressedProof,
                    publicInputsCommitment = proofBundle.publicInputs.commitment,
                    epoch = epoch,
                    tier = tier.id
                )
            }

            SEPGroupType.ONE_ON_ONE, SEPGroupType.DEMOCRACY -> withRetry {
                contractClient.createGroupV2(
                    caller = callerAddress,
                    groupID = groupIDData,
                    commitment = proofBundle.publicInputs.commitment,
                    tier = tier.id,
                    groupType = groupType.id,
                    memberCount = effectiveMemberCount,
                    proof = uncompressedProof,
                    publicInputsCommitment = proofBundle.publicInputs.commitment,
                    epoch = epoch
                )
            }

            SEPGroupType.OLIGARCHY -> {
                require(!adminLeaves.isNullOrEmpty() && adminSalt != null) {
                    "Oligarchy creation requires adminLeaves + adminSalt to seed the admin tree"
                }
                val adminCommitment = SEPCommitmentBuilder.computeAdminCommitment(adminLeaves!!, adminSalt!!)
                withRetry {
                    contractClient.createOligarchyGroup(
                        caller = callerAddress,
                        groupID = groupIDData,
                        commitment = proofBundle.publicInputs.commitment,
                        tier = tier.id,
                        memberCount = effectiveMemberCount,
                        adminRoot = adminCommitment,
                        proof = uncompressedProof,
                        publicInputsCommitment = proofBundle.publicInputs.commitment,
                        epoch = epoch
                    )
                }
            }
        }
    }

    /**
     * Publish a commitment update after a membership change.
     *
     * #59: binds the new commitment inside the UpdateCircuit proof so the
     * contract can cryptographically accept `c_new` as the persisted value.
     * The new epoch is derived in-circuit as `epoch_old + 1`.
     */
    fun publishCommitmentUpdate(
        groupIDData: ByteArray,
        oldMembers: List<SEPGroupMemberLeaf>,
        oldEpoch: Long,
        oldSalt: ByteArray,
        newMembers: List<SEPGroupMemberLeaf>,
        newSalt: ByteArray,
        blsSecretKey: ByteArray,
        tier: SEPTier
    ): SEPSubmissionResponse {
        val updatePK = ensureUpdateProvingKey(tier)
        val bundle = SEPProofGenerator.generateUpdateProof(
            provingKey = updatePK,
            oldMembers = oldMembers,
            newMembers = newMembers,
            secretKey = blsSecretKey,
            epochOld = oldEpoch,
            saltOld = oldSalt,
            saltNew = newSalt,
            tier = tier
        )
        val uncompressedProof = proofForContract(bundle.proof)

        return withRetry {
            contractClient.updateCommitment(
                groupID = groupIDData,
                proof = uncompressedProof,
                cOld = bundle.publicInputs.cOld,
                epochOld = bundle.publicInputs.epochOld,
                cNew = bundle.publicInputs.cNew
            )
        }
    }

    /** Fetch the current on-chain state for a group. */
    fun fetchOnChainState(groupIDData: ByteArray): SEPCommitmentEntry {
        return withRetry { contractClient.getState(groupIDData) }
    }

    // -- Democracy (#26) --

    /**
     * Delta kind for a Democracy update (add/remove/kick), matching the
     * DemocracyUpdateCircuit witness encoding (§6.4.2).
     */
    enum class DemocracyDeltaKind(val id: Int) { ADD(0), REMOVE(1), KICK(2) }

    /**
     * Generate a Groth16 proof for a quorum-signed Democracy update.
     *
     * Status: stub. The witness requires K signer BLS secret keys plus
     * per-signer Merkle paths into the member tree. Voters hold those keys
     * locally; no coordinator protocol currently exists to assemble them at
     * the finalizer. Throws [DemocracyProofNotImplementedException] until
     * that design is resolved. See §6.4.2 and the #26 tracking note.
     */
    fun generateDemocracyUpdateProof(
        ballotID: String,
        oldMembers: List<SEPGroupMemberLeaf>,
        newMembers: List<SEPGroupMemberLeaf>,
        deltaKind: DemocracyDeltaKind,
        signerSecretKeys: List<ByteArray>,
        signerPublicKeys: List<ByteArray>,
        saltOld: ByteArray,
        saltNew: ByteArray,
        epochOld: Long,
        tier: SEPTier
    ): Nothing {
        throw DemocracyProofNotImplementedException(
            "Democracy coordinator protocol not yet implemented: K voters' BLS " +
                "secret keys must reach the finalizer to assemble the witness (§6.4.2)."
        )
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
            val firstMember = members.firstOrNull()

            if (BuildConfig.DEBUG) {
                Log.d(
                    TAG,
                    "verifyCommitment group=${groupIDData.debugHexPrefix()} epoch=$epoch onChainEpoch=${entry.epoch} " +
                        "tier=${tier.id} members=${members.size} salt=${salt.debugHexPrefix(12)} " +
                        "localPoseidon=${localPoseidonCommitment.debugHexPrefix(12)} " +
                        "onChainCommitment=${entry.commitment.debugHexPrefix(12)} " +
                        "firstPk=${firstMember?.publicKeyCompressed?.debugHexPrefix(12) ?: "none"} " +
                        "firstLeaf=${firstMember?.leafHash?.debugHexPrefix(12) ?: "none"}"
                )
            }

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

    private fun ByteArray.debugHexPrefix(bytes: Int = 8): String {
        val take = minOf(bytes, size)
        return take(take).joinToString("") { "%02x".format(it) }
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

/** Thrown by [OnChainService.generateDemocracyUpdateProof] until the #26 coordinator protocol lands. */
class DemocracyProofNotImplementedException(message: String) : RuntimeException(message)
