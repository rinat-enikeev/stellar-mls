package chat.onym.android

import androidx.test.ext.junit.runners.AndroidJUnit4
import chat.onym.android.model.ChatGroup
import chat.onym.android.model.ChatError
import chat.onym.android.model.BootstrapPayload
import chat.onym.android.model.InviteCode
import chat.onym.android.model.hexToBytes
import chat.onym.android.model.toHex
import chat.onym.android.onchain.OnChainVerificationResult
import com.stellarmls.mls.SEPCommitmentBuilder
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPProofGenerator
import com.stellarmls.mls.SEPTier
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Android instrumented tests matching iOS Phase3Tests.swift.
 * Must run on device/emulator because crypto operations use JNI native library.
 */
@RunWith(AndroidJUnit4::class)
class Phase3Tests {

    // -----------------------------------------------------------------------
    // 1. Group ID Hex Conversion
    // -----------------------------------------------------------------------

    @Test
    fun groupIDData_hexToCorrectBytes() {
        val group = ChatGroup(
            id = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            name = "test",
            groupSecret = ByteArray(32)
        )
        val data = group.groupIDData
        assertEquals(32, data.size)
        assertEquals(0xAB.toByte(), data[0])
        assertEquals(0xCD.toByte(), data[1])
        assertEquals(0xEF.toByte(), data[2])
        assertEquals(0x01.toByte(), data[3])
    }

    @Test
    fun groupIDData_shortHex() {
        val group = ChatGroup(
            id = "00ff",
            name = "test",
            groupSecret = ByteArray(32)
        )
        val data = group.groupIDData
        assertEquals(2, data.size)
        assertEquals(0x00.toByte(), data[0])
        assertEquals(0xFF.toByte(), data[1])
    }

    @Test
    fun groupIDData_roundtrip() {
        val original = ByteArray(32) { it.toByte() }
        val hex = original.toHex()
        val group = ChatGroup(id = hex, name = "test", groupSecret = ByteArray(32))
        assertArrayEquals(original, group.groupIDData)
    }

    // -----------------------------------------------------------------------
    // 2. OnChainVerificationResult
    // -----------------------------------------------------------------------

    @Test
    fun verificationResult_displayText() {
        assertEquals("Not published on-chain", OnChainVerificationResult.NotPublished.displayText)
        assertEquals("Verified on-chain", OnChainVerificationResult.Verified.displayText)
        assertEquals("Commitment mismatch", OnChainVerificationResult.CommitmentMismatch.displayText)
        assertEquals("Group deactivated on-chain", OnChainVerificationResult.Inactive.displayText)
    }

    @Test
    fun verificationResult_epochMismatchIncludesBothValues() {
        val result = OnChainVerificationResult.EpochMismatch(local = 5, onChain = 3)
        assertTrue(result.displayText.contains("5"))
        assertTrue(result.displayText.contains("3"))
    }

    @Test
    fun verificationResult_errorIncludesMessage() {
        val result = OnChainVerificationResult.Error("connection refused")
        assertTrue(result.displayText.contains("connection refused"))
    }

    @Test
    fun verificationResult_equality() {
        assertEquals(OnChainVerificationResult.Verified, OnChainVerificationResult.Verified)
        assertNotEquals(OnChainVerificationResult.Verified, OnChainVerificationResult.NotPublished)
        assertEquals(
            OnChainVerificationResult.EpochMismatch(1, 2),
            OnChainVerificationResult.EpochMismatch(1, 2)
        )
        assertNotEquals(
            OnChainVerificationResult.EpochMismatch(1, 2),
            OnChainVerificationResult.EpochMismatch(1, 3)
        )
    }

    // -----------------------------------------------------------------------
    // 3. Member Ordering (SEP-XXXX §2.1)
    // -----------------------------------------------------------------------

    @Test
    fun memberOrdering_sortsByCompressedG1Key() {
        val leafA = SEPGroupMemberLeaf(
            publicKeyCompressed = ByteArray(48) { 0x01 },
            leafHash = ByteArray(32) { 0xAA.toByte() }
        )
        val leafB = SEPGroupMemberLeaf(
            publicKeyCompressed = ByteArray(48) { 0xFF.toByte() },
            leafHash = ByteArray(32) { 0xBB.toByte() }
        )

        // Add in reverse order, then sort
        val members = mutableListOf(leafB, leafA)
        members.sortWith { a, b ->
            val aKey = a.publicKeyCompressed
            val bKey = b.publicKeyCompressed
            for (i in 0 until minOf(aKey.size, bKey.size)) {
                val cmp = (aKey[i].toInt() and 0xFF) - (bKey[i].toInt() and 0xFF)
                if (cmp != 0) return@sortWith cmp
            }
            aKey.size - bKey.size
        }

        // leafA (0x01...) should come before leafB (0xFF...)
        assertArrayEquals(leafA.publicKeyCompressed, members[0].publicKeyCompressed)
        assertArrayEquals(leafB.publicKeyCompressed, members[1].publicKeyCompressed)
    }

    @Test
    fun bootstrapPayload_toChatGroup_reSortsMembers() {
        val leafA = SEPGroupMemberLeaf(
            publicKeyCompressed = ByteArray(48) { 0x01 },
            leafHash = ByteArray(32) { 0xAA.toByte() }
        )
        val leafB = SEPGroupMemberLeaf(
            publicKeyCompressed = ByteArray(48) { 0xFF.toByte() },
            leafHash = ByteArray(32) { 0xBB.toByte() }
        )

        // Members in WRONG order (B before A)
        val payload = BootstrapPayload(
            groupID = ByteArray(32) { 0x42 },
            groupSecret = ByteArray(32),
            name = "test",
            epoch = 0,
            members = listOf(leafB, leafA),
            relayHints = emptyList(),
            salt = ByteArray(32),
            tierRawValue = 0,
            commitment = null,
            senderNostrPubkey = "abc123"
        )

        val group = payload.toChatGroup()
        assertEquals(2, group.members.size)
        assertArrayEquals(leafA.publicKeyCompressed, group.members[0].publicKeyCompressed)
        assertArrayEquals(leafB.publicKeyCompressed, group.members[1].publicKeyCompressed)
    }

    @Test
    fun bootstrapPayload_alreadySortedStable() {
        val leafA = SEPGroupMemberLeaf(
            publicKeyCompressed = ByteArray(48) { 0x01 },
            leafHash = ByteArray(32) { 0xAA.toByte() }
        )
        val leafB = SEPGroupMemberLeaf(
            publicKeyCompressed = ByteArray(48) { 0xFF.toByte() },
            leafHash = ByteArray(32) { 0xBB.toByte() }
        )

        val payload = BootstrapPayload(
            groupID = ByteArray(32) { 0x42 },
            groupSecret = ByteArray(32),
            name = "test",
            epoch = 0,
            members = listOf(leafA, leafB),
            relayHints = emptyList(),
            salt = ByteArray(32),
            tierRawValue = 0,
            commitment = null,
            senderNostrPubkey = "abc123"
        )

        val group = payload.toChatGroup()
        assertArrayEquals(leafA.publicKeyCompressed, group.members[0].publicKeyCompressed)
        assertArrayEquals(leafB.publicKeyCompressed, group.members[1].publicKeyCompressed)
    }

    // -----------------------------------------------------------------------
    // 4. Commitment Variant Consistency (SHA-256 vs Poseidon)
    // -----------------------------------------------------------------------

    @Test
    fun commitments_sha256AndPoseidonDiffer() {
        val root = ByteArray(32) { 0x42 }
        val salt = ByteArray(32) { 0x07 }
        val epoch = 0L

        val sha256 = SEPCommitmentBuilder.computeSHA256Commitment(root, epoch, salt)
        val poseidon = SEPCommitmentBuilder.computePoseidonCommitment(root, epoch, salt)

        assertEquals(32, sha256.size)
        assertEquals(32, poseidon.size)
        assertFalse(sha256.contentEquals(poseidon))
    }

    @Test
    fun commitments_poseidonDeterministic() {
        val root = ByteArray(32) { 0x42 }
        val salt = ByteArray(32) { 0x07 }
        val epoch = 5L

        val c1 = SEPCommitmentBuilder.computePoseidonCommitment(root, epoch, salt)
        val c2 = SEPCommitmentBuilder.computePoseidonCommitment(root, epoch, salt)
        assertArrayEquals(c1, c2)
    }

    @Test
    fun commitments_differentEpochProducesDifferent() {
        val root = ByteArray(32) { 0x42 }
        val salt = ByteArray(32) { 0x07 }

        val c0 = SEPCommitmentBuilder.computePoseidonCommitment(root, 0, salt)
        val c1 = SEPCommitmentBuilder.computePoseidonCommitment(root, 1, salt)
        assertFalse(c0.contentEquals(c1))
    }

    @Test
    fun commitments_differentSaltProducesDifferent() {
        val root = ByteArray(32) { 0x42 }
        val epoch = 0L

        val c1 = SEPCommitmentBuilder.computePoseidonCommitment(root, epoch, ByteArray(32) { 0x01 })
        val c2 = SEPCommitmentBuilder.computePoseidonCommitment(root, epoch, ByteArray(32) { 0x02 })
        assertFalse(c1.contentEquals(c2))
    }

    // -----------------------------------------------------------------------
    // 5. Proof Generation & Format Conversion
    // -----------------------------------------------------------------------

    private fun makeTestMember(): Pair<ByteArray, SEPGroupMemberLeaf> {
        val secretKey = ByteArray(32) { 0x42 }
        val pubKey = SEPCommitmentBuilder.computePublicKey(secretKey)
        val leafHash = SEPCommitmentBuilder.computeLeafHash(secretKey)
        return secretKey to SEPGroupMemberLeaf(publicKeyCompressed = pubKey, leafHash = leafHash)
    }

    @Test
    fun provingKeyGeneration_succeeds() {
        val pk = SEPProofGenerator.generateTestingProvingKey(SEPTier.SMALL)
        assertTrue(pk.isNotEmpty())
    }

    @Test
    fun proofGeneration_returnsValidBundle() {
        val (secretKey, leaf) = makeTestMember()
        val salt = SEPCommitmentBuilder.generateSalt()
        val pk = SEPProofGenerator.generateTestingProvingKey(SEPTier.SMALL)

        val bundle = SEPProofGenerator.generateMembershipProof(
            provingKey = pk,
            members = listOf(leaf),
            secretKey = secretKey,
            epoch = 0,
            salt = salt,
            tier = SEPTier.SMALL
        )

        // Compressed proof = 192 bytes (G1_48 + G2_96 + G1_48)
        assertEquals(192, bundle.proof.size)
        // Poseidon commitment = 32 bytes
        assertEquals(32, bundle.publicInputs.commitment.size)
        assertEquals(0L, bundle.publicInputs.epoch)
    }

    @Test
    fun proofToContractFormat_correctSizes() {
        val (secretKey, leaf) = makeTestMember()
        val salt = SEPCommitmentBuilder.generateSalt()
        val pk = SEPProofGenerator.generateTestingProvingKey(SEPTier.SMALL)

        val bundle = SEPProofGenerator.generateMembershipProof(
            pk, listOf(leaf), secretKey, 0, salt, SEPTier.SMALL
        )

        val components = SEPProofGenerator.proofToContractFormat(bundle.proof)

        // Uncompressed BLS12-381 point sizes
        assertEquals(96, components.proofA.size)   // G1 uncompressed
        assertEquals(192, components.proofB.size)  // G2 uncompressed
        assertEquals(96, components.proofC.size)   // G1 uncompressed

        // Total uncompressed = 384 bytes
        assertEquals(384, components.proofA.size + components.proofB.size + components.proofC.size)
    }

    @Test
    fun proofCommitment_matchesSeparatelyComputedPoseidon() {
        val (secretKey, leaf) = makeTestMember()
        val salt = SEPCommitmentBuilder.generateSalt()
        val pk = SEPProofGenerator.generateTestingProvingKey(SEPTier.SMALL)

        val bundle = SEPProofGenerator.generateMembershipProof(
            pk, listOf(leaf), secretKey, 0, salt, SEPTier.SMALL
        )

        // Recompute via CommitmentBuilder
        val root = SEPCommitmentBuilder.computeMerkleRoot(listOf(leaf), SEPTier.SMALL)
        val poseidon = SEPCommitmentBuilder.computePoseidonCommitment(root, 0, salt)

        // Core invariant: proof commitment == separately computed Poseidon commitment
        assertArrayEquals(poseidon, bundle.publicInputs.commitment)
    }

    @Test
    fun sha256Commitment_differsFromProofCommitment() {
        val (secretKey, leaf) = makeTestMember()
        val salt = SEPCommitmentBuilder.generateSalt()
        val pk = SEPProofGenerator.generateTestingProvingKey(SEPTier.SMALL)

        val bundle = SEPProofGenerator.generateMembershipProof(
            pk, listOf(leaf), secretKey, 0, salt, SEPTier.SMALL
        )

        val root = SEPCommitmentBuilder.computeMerkleRoot(listOf(leaf), SEPTier.SMALL)
        val sha256 = SEPCommitmentBuilder.computeSHA256Commitment(root, 0, salt)

        // Local SHA-256 commitment must NOT equal on-chain Poseidon commitment
        assertFalse(sha256.contentEquals(bundle.publicInputs.commitment))
    }

    // -----------------------------------------------------------------------
    // 6. Merkle Root Determinism
    // -----------------------------------------------------------------------

    @Test
    fun merkleRoot_deterministic() {
        val secretKey = ByteArray(32) { 0x42 }
        val leaf = SEPGroupMemberLeaf(
            publicKeyCompressed = SEPCommitmentBuilder.computePublicKey(secretKey),
            leafHash = SEPCommitmentBuilder.computeLeafHash(secretKey)
        )

        val root1 = SEPCommitmentBuilder.computeMerkleRoot(listOf(leaf), SEPTier.SMALL)
        val root2 = SEPCommitmentBuilder.computeMerkleRoot(listOf(leaf), SEPTier.SMALL)
        assertArrayEquals(root1, root2)
        assertEquals(32, root1.size)
    }

    @Test
    fun merkleRoot_orderCanonicalizedByCircuit() {
        val sk1 = ByteArray(32) { 0x01 }
        val sk2 = ByteArray(32) { 0x02 }

        val leaf1 = SEPGroupMemberLeaf(
            publicKeyCompressed = SEPCommitmentBuilder.computePublicKey(sk1),
            leafHash = SEPCommitmentBuilder.computeLeafHash(sk1)
        )
        val leaf2 = SEPGroupMemberLeaf(
            publicKeyCompressed = SEPCommitmentBuilder.computePublicKey(sk2),
            leafHash = SEPCommitmentBuilder.computeLeafHash(sk2)
        )

        val rootAB = SEPCommitmentBuilder.computeMerkleRoot(listOf(leaf1, leaf2), SEPTier.SMALL)
        val rootBA = SEPCommitmentBuilder.computeMerkleRoot(listOf(leaf2, leaf1), SEPTier.SMALL)

        // Rust circuit canonicalizes leaf order — root is the same regardless of input order.
        assertArrayEquals(rootAB, rootBA)
    }

    // -----------------------------------------------------------------------
    // 7. BLS Key Derivation
    // -----------------------------------------------------------------------

    @Test
    fun blsPublicKey_is48Bytes() {
        val pubKey = SEPCommitmentBuilder.computePublicKey(ByteArray(32) { 0x42 })
        assertEquals(48, pubKey.size)
    }

    @Test
    fun blsLeafHash_is32Bytes() {
        val hash = SEPCommitmentBuilder.computeLeafHash(ByteArray(32) { 0x42 })
        assertEquals(32, hash.size)
    }

    @Test
    fun blsKeys_differentSecretsDifferentKeys() {
        val pk1 = SEPCommitmentBuilder.computePublicKey(ByteArray(32) { 0x01 })
        val pk2 = SEPCommitmentBuilder.computePublicKey(ByteArray(32) { 0x02 })
        assertFalse(pk1.contentEquals(pk2))
    }

    @Test
    fun blsKeys_deterministic() {
        val sk = ByteArray(32) { 0x42 }
        val pk1 = SEPCommitmentBuilder.computePublicKey(sk)
        val pk2 = SEPCommitmentBuilder.computePublicKey(sk)
        assertArrayEquals(pk1, pk2)
    }

    // -----------------------------------------------------------------------
    // 8. ChatError
    // -----------------------------------------------------------------------

    @Test
    fun chatError_contractNotConfigured() {
        val error = ChatError.ContractNotConfigured
        assertTrue(error.message!!.contains("not configured"))
    }

    @Test
    fun chatError_onChainPublishFailed_includesReason() {
        val error = ChatError.OnChainPublishFailed("timeout")
        assertTrue(error.message!!.contains("timeout"))
    }

    // -----------------------------------------------------------------------
    // 9. isPublishedOnChain Persistence Roundtrip
    // -----------------------------------------------------------------------

    @Test
    fun isPublishedOnChain_defaultsFalse() {
        val group = ChatGroup(
            id = "abc",
            name = "test",
            groupSecret = ByteArray(32)
        )
        assertFalse(group.isPublishedOnChain)
    }

    @Test
    fun isPublishedOnChain_canSetTrue() {
        val group = ChatGroup(
            id = "abc",
            name = "test",
            groupSecret = ByteArray(32)
        )
        group.isPublishedOnChain = true
        assertTrue(group.isPublishedOnChain)
    }

    // -----------------------------------------------------------------------
    // 10. End-to-End: Proof → Verify Commitment Match
    // -----------------------------------------------------------------------

    @Test
    fun endToEnd_fullPipeline() {
        // 1. Create member
        val secretKey = ByteArray(32) { 0x42 }
        val pubKey = SEPCommitmentBuilder.computePublicKey(secretKey)
        val leafHash = SEPCommitmentBuilder.computeLeafHash(secretKey)
        val leaf = SEPGroupMemberLeaf(pubKey, leafHash)

        // 2. Group state
        val members = listOf(leaf)
        val epoch = 0L
        val salt = SEPCommitmentBuilder.generateSalt()
        val tier = SEPTier.SMALL

        // 3. Generate proof
        val pk = SEPProofGenerator.generateTestingProvingKey(tier)
        val bundle = SEPProofGenerator.generateMembershipProof(
            pk, members, secretKey, epoch, salt, tier
        )

        // 4. Verification: recompute locally
        val root = SEPCommitmentBuilder.computeMerkleRoot(members, tier)
        val localPoseidon = SEPCommitmentBuilder.computePoseidonCommitment(root, epoch, salt)

        // Critical: proof commitment == local Poseidon commitment
        assertArrayEquals(localPoseidon, bundle.publicInputs.commitment)

        // 5. Verify proof format
        val components = SEPProofGenerator.proofToContractFormat(bundle.proof)
        assertEquals(96, components.proofA.size)
        assertEquals(192, components.proofB.size)
        assertEquals(96, components.proofC.size)
    }

    @Test
    fun endToEnd_commitmentUpdatePipeline() {
        // 1. Initial state: one member
        val sk1 = ByteArray(32) { 0x01 }
        val leaf1 = SEPGroupMemberLeaf(
            SEPCommitmentBuilder.computePublicKey(sk1),
            SEPCommitmentBuilder.computeLeafHash(sk1)
        )

        val oldMembers = listOf(leaf1)
        val oldEpoch = 0L
        val oldSalt = SEPCommitmentBuilder.generateSalt()
        val tier = SEPTier.SMALL

        // 2. Proof at old state
        val pk = SEPProofGenerator.generateTestingProvingKey(tier)
        val oldBundle = SEPProofGenerator.generateMembershipProof(
            pk, oldMembers, sk1, oldEpoch, oldSalt, tier
        )

        // 3. Add second member
        val sk2 = ByteArray(32) { 0x02 }
        val leaf2 = SEPGroupMemberLeaf(
            SEPCommitmentBuilder.computePublicKey(sk2),
            SEPCommitmentBuilder.computeLeafHash(sk2)
        )

        val newMembers = listOf(leaf1, leaf2).sortedWith { a, b ->
            val aKey = a.publicKeyCompressed
            val bKey = b.publicKeyCompressed
            for (i in 0 until minOf(aKey.size, bKey.size)) {
                val cmp = (aKey[i].toInt() and 0xFF) - (bKey[i].toInt() and 0xFF)
                if (cmp != 0) return@sortedWith cmp
            }
            aKey.size - bKey.size
        }
        val newEpoch = 1L
        val newSalt = SEPCommitmentBuilder.generateSalt()

        // 4. New Poseidon commitment
        val newRoot = SEPCommitmentBuilder.computeMerkleRoot(newMembers, tier)
        val newPoseidon = SEPCommitmentBuilder.computePoseidonCommitment(newRoot, newEpoch, newSalt)

        // 5. Old proof commitment matches old state
        val oldRoot = SEPCommitmentBuilder.computeMerkleRoot(oldMembers, tier)
        val oldPoseidon = SEPCommitmentBuilder.computePoseidonCommitment(oldRoot, oldEpoch, oldSalt)
        assertArrayEquals(oldBundle.publicInputs.commitment, oldPoseidon)

        // 6. New != old
        assertFalse(newPoseidon.contentEquals(oldPoseidon))

        // 7. Epoch increment
        assertEquals(oldEpoch + 1, newEpoch)
    }

    // -----------------------------------------------------------------------
    // 11. Cross-Platform Interoperability Test Vectors
    //     (must match docs/cross-platform-test-vectors.json and iOS Phase3Tests)
    // -----------------------------------------------------------------------

    @Test
    fun crossPlatform_topicDerivationMatchesCanonicalVectors() {
        val vectors = listOf(
            "0000000000000000000000000000000000000000000000000000000000000000" to "0c6ce887ec3881f3",
            "4242424242424242424242424242424242424242424242424242424242424242" to "aa263d222a1f2e1f",
            "ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00" to "7df84225883879ff",
        )
        for ((secretHex, expected) in vectors) {
            val topic = chat.onym.android.crypto.GroupCrypto.hiddenGroupTopic(secretHex.hexToBytes())
            assertEquals(expected, topic, "Topic mismatch for secret ${secretHex.take(16)}...")
        }
    }

    @Test
    fun crossPlatform_inboxDerivationMatchesCanonicalVectors() {
        val vectors = listOf(
            "0000000000000000000000000000000000000000000000000000000000000000" to "f434d35e24e86124",
            "4242424242424242424242424242424242424242424242424242424242424242" to "c9bcafedc73f4289",
        )
        for ((pubkeyHex, expected) in vectors) {
            val tag = chat.onym.android.crypto.GroupCrypto.hiddenInboxTag(pubkeyHex.hexToBytes())
            assertEquals(expected, tag, "Inbox tag mismatch for pubkey ${pubkeyHex.take(16)}...")
        }
    }

    @Test
    fun crossPlatform_hkdfMessageKeyMatchesCanonicalVectors() {
        data class Vector(val secretHex: String, val epoch: Long, val saltHex: String, val expectedHex: String)
        val vectors = listOf(
            Vector(
                "0000000000000000000000000000000000000000000000000000000000000000",
                0,
                "0000000000000000000000000000000000000000000000000000000000000000",
                "e7b6e0341ab181313e1325602763c196b8113f1f8accf570674e797863bd6acd"
            ),
            Vector(
                "4242424242424242424242424242424242424242424242424242424242424242",
                5,
                "0707070707070707070707070707070707070707070707070707070707070707",
                "68346c282ce344b6ff7f6c7138737873438a2d7fd28c89573899d73ee6a9d52e"
            ),
        )
        for (v in vectors) {
            val key = chat.onym.android.crypto.GroupCrypto.deriveMessageKey(
                v.secretHex.hexToBytes(), v.epoch, v.saltHex.hexToBytes()
            )
            assertEquals(v.expectedHex, key.toHex(), "HKDF key mismatch for epoch ${v.epoch}")
        }
    }

//    @Test
//    fun crossPlatform_blsKeyDerivationConsistentSizes() {
//        val secrets = listOf(
//            "4242424242424242424242424242424242424242424242424242424242424242",
//            "0101010101010101010101010101010101010101010101010101010101010101",
//        )
//        for (secretHex in secrets) {
//            val sk = secretHex.hexToBytes()
//            val pk = SEPCommitmentBuilder.computePublicKey(sk)
//            val lh = SEPCommitmentBuilder.computeLeafHash(sk)
//            assertEquals(48, pk.size, "BLS public key must be 48 bytes")
//            assertEquals(32, lh.size, "Leaf hash must be 32 bytes")
//        }
//    }

    @Test
    fun crossPlatform_blsKeyDerivationDeterministic() {
        val sk = "4242424242424242424242424242424242424242424242424242424242424242".hexToBytes()
        val pk1 = SEPCommitmentBuilder.computePublicKey(sk)
        val pk2 = SEPCommitmentBuilder.computePublicKey(sk)
        val lh1 = SEPCommitmentBuilder.computeLeafHash(sk)
        val lh2 = SEPCommitmentBuilder.computeLeafHash(sk)
        assertArrayEquals(pk1, pk2)
        assertArrayEquals(lh1, lh2)
    }

    @Test
    fun crossPlatform_inviteCodeRoundtrip() {
        val members = listOf(
            SEPGroupMemberLeaf(
                publicKeyCompressed = ByteArray(48) { 0x01 },
                leafHash = ByteArray(32) { 0xAA.toByte() }
            )
        )
        val code = InviteCode(
            groupID = ByteArray(32) { 0x42 },
            groupSecret = ByteArray(32) { 0x07 },
            name = "test-group",
            relayHints = listOf("wss://relay.damus.io"),
            members = members,
            epoch = 3,
            salt = ByteArray(32) { 0x0B },
            commitment = ByteArray(32) { 0xFF.toByte() }
        )

        val encoded = code.encode()
        val decoded = InviteCode.decode(encoded)

        assertArrayEquals(code.groupID, decoded.groupID)
        assertArrayEquals(code.groupSecret, decoded.groupSecret)
        assertEquals(code.name, decoded.name)
        assertEquals(code.relayHints, decoded.relayHints)
        assertEquals(code.epoch, decoded.epoch)
        assertArrayEquals(code.salt, decoded.salt)
        assertArrayEquals(code.commitment, decoded.commitment)
        assertEquals(1, decoded.members.size)
        assertArrayEquals(members[0].publicKeyCompressed, decoded.members[0].publicKeyCompressed)
        assertArrayEquals(members[0].leafHash, decoded.members[0].leafHash)
    }

    // -----------------------------------------------------------------------
    // 12. Salt Generation
    // -----------------------------------------------------------------------

    @Test
    fun salt_is32Bytes() {
        val salt = SEPCommitmentBuilder.generateSalt()
        assertEquals(32, salt.size)
    }

    @Test
    fun salt_twoSaltsDiffer() {
        val s1 = SEPCommitmentBuilder.generateSalt()
        val s2 = SEPCommitmentBuilder.generateSalt()
        assertFalse(s1.contentEquals(s2))
    }
}
