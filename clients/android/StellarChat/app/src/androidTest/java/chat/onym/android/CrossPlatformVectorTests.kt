package chat.onym.android

import androidx.test.ext.junit.runners.AndroidJUnit4
import chat.onym.android.model.hexToBytes
import chat.onym.android.model.toHex
import com.stellarmls.mls.SEPCommitmentBuilder
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPTier
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Cross-platform vector tests that verify Android (via JNI) produces
 * EXACT same outputs as the Rust reference implementation.
 *
 * These match docs/cross-platform-test-vectors.json and the corresponding
 * iOS CrossPlatformVectorTests.swift.
 *
 * Must run on device/emulator because BLS operations use JNI native library.
 */
@RunWith(AndroidJUnit4::class)
class CrossPlatformVectorTests {

    // -----------------------------------------------------------------------
    // BLS Key Derivation — Exact Hex Vectors
    // -----------------------------------------------------------------------

    @Test
    fun blsPublicKey_matchesVector1_sk42() {
        val sk = "4242424242424242424242424242424242424242424242424242424242424242".hexToBytes()
        val pk = SEPCommitmentBuilder.computePublicKey(sk)
        assertEquals(
            "b5b99c967e4c69822f427db1f6871dd119afb95ab9646ba2e707990a3db31777a59b66f69e89c2055699b0ade7357eae",
            pk.toHex()
        )
    }

    @Test
    fun blsPublicKey_matchesVector2_sk01() {
        val sk = "0101010101010101010101010101010101010101010101010101010101010101".hexToBytes()
        val pk = SEPCommitmentBuilder.computePublicKey(sk)
        assertEquals(
            "aa1a1c26055a329817a5759d877a2795f9499b97d6056edde0eea39512f24e8bc874b4471f0501127abb1ea0d9f68ac1",
            pk.toHex()
        )
    }

    @Test
    fun blsLeafHash_matchesVector1_sk42() {
        val sk = "4242424242424242424242424242424242424242424242424242424242424242".hexToBytes()
        val lh = SEPCommitmentBuilder.computeLeafHash(sk)
        assertEquals(
            "1fe19c284d0763e709dc2a0195cee0c40ae331ff7a7cd6efd16bc3c8a534e46c",
            lh.toHex()
        )
    }

    @Test
    fun blsLeafHash_matchesVector2_sk01() {
        val sk = "0101010101010101010101010101010101010101010101010101010101010101".hexToBytes()
        val lh = SEPCommitmentBuilder.computeLeafHash(sk)
        assertEquals(
            "5f44af31406da8025c110940b29eafcaeb42b0d34992e27dcc0ce22c11d3872e",
            lh.toHex()
        )
    }

    // -----------------------------------------------------------------------
    // Merkle Root — Exact Hex Vectors
    // -----------------------------------------------------------------------

    private fun makeLeaf(skHex: String): SEPGroupMemberLeaf {
        val sk = skHex.hexToBytes()
        return SEPGroupMemberLeaf(
            publicKeyCompressed = SEPCommitmentBuilder.computePublicKey(sk),
            leafHash = SEPCommitmentBuilder.computeLeafHash(sk)
        )
    }

    @Test
    fun merkleRoot_singleMember_matchesVector() {
        val leaf = makeLeaf("4242424242424242424242424242424242424242424242424242424242424242")
        val root = SEPCommitmentBuilder.computeMerkleRoot(listOf(leaf), SEPTier.SMALL)
        assertEquals(
            "5215123288dcf6706347bef8a27ea55d2d53bb64270e2a69934007ea34f2f04e",
            root.toHex()
        )
    }

    @Test
    fun merkleRoot_twoMembers_matchesVector() {
        val leaf1 = makeLeaf("4242424242424242424242424242424242424242424242424242424242424242")
        val leaf2 = makeLeaf("0101010101010101010101010101010101010101010101010101010101010101")
        val root = SEPCommitmentBuilder.computeMerkleRoot(listOf(leaf1, leaf2), SEPTier.SMALL)
        assertEquals(
            "30bb9c0d5ca89a65ce7f16817c31999800ab316c7e0eb903fea7d594087fdd94",
            root.toHex()
        )
    }

    @Test
    fun merkleRoot_twoMembers_orderIndependent() {
        val leaf1 = makeLeaf("4242424242424242424242424242424242424242424242424242424242424242")
        val leaf2 = makeLeaf("0101010101010101010101010101010101010101010101010101010101010101")
        val rootAB = SEPCommitmentBuilder.computeMerkleRoot(listOf(leaf1, leaf2), SEPTier.SMALL)
        val rootBA = SEPCommitmentBuilder.computeMerkleRoot(listOf(leaf2, leaf1), SEPTier.SMALL)
        assertArrayEquals(rootAB, rootBA)
    }

    // -----------------------------------------------------------------------
    // Poseidon Commitment — Exact Hex Vectors
    // -----------------------------------------------------------------------

    @Test
    fun poseidonCommitment_matchesVector1() {
        val root = "5215123288dcf6706347bef8a27ea55d2d53bb64270e2a69934007ea34f2f04e".hexToBytes()
        val salt = ByteArray(32) // all zeros
        val commitment = SEPCommitmentBuilder.computePoseidonCommitment(root, 0, salt)
        assertEquals(
            "255f5a330e7fb48a50a5b1a5fbce17b717aaad9861590025b53b3a1cf527010e",
            commitment.toHex()
        )
    }

    @Test
    fun poseidonCommitment_matchesVector2() {
        val root = "30bb9c0d5ca89a65ce7f16817c31999800ab316c7e0eb903fea7d594087fdd94".hexToBytes()
        val salt = ByteArray(32) { 0xAA.toByte() }
        val commitment = SEPCommitmentBuilder.computePoseidonCommitment(root, 5, salt)
        assertEquals(
            "0ff7547913428aaea8c7242dbcbf1e7921b58878307b89fc430a9a32ccb56aa5",
            commitment.toHex()
        )
    }

    // -----------------------------------------------------------------------
    // SHA-256 Commitment — Exact Hex Vectors
    // -----------------------------------------------------------------------

    @Test
    fun sha256Commitment_matchesVector1() {
        val root = "5215123288dcf6706347bef8a27ea55d2d53bb64270e2a69934007ea34f2f04e".hexToBytes()
        val salt = ByteArray(32) // all zeros
        val commitment = SEPCommitmentBuilder.computeSHA256Commitment(root, 0, salt)
        assertEquals(
            "851617b0103de7c3df520bb34b6e2ae989bd9aaa23355baa5cf033255d88fa26",
            commitment.toHex()
        )
    }

    @Test
    fun sha256Commitment_matchesVector2() {
        val root = "30bb9c0d5ca89a65ce7f16817c31999800ab316c7e0eb903fea7d594087fdd94".hexToBytes()
        val salt = ByteArray(32) { 0xAA.toByte() }
        val commitment = SEPCommitmentBuilder.computeSHA256Commitment(root, 5, salt)
        assertEquals(
            "58a1d00ea6f30a925ea91e791f934b08e815f4c345b445af5a9d072926df67a0",
            commitment.toHex()
        )
    }

    // -----------------------------------------------------------------------
    // Cross-Variant Consistency
    // -----------------------------------------------------------------------

    @Test
    fun poseidonAndSha256_differentForSameInputs() {
        val root = "5215123288dcf6706347bef8a27ea55d2d53bb64270e2a69934007ea34f2f04e".hexToBytes()
        val salt = ByteArray(32)
        val poseidon = SEPCommitmentBuilder.computePoseidonCommitment(root, 0, salt)
        val sha256 = SEPCommitmentBuilder.computeSHA256Commitment(root, 0, salt)
        assertFalse(
            "Poseidon and SHA-256 commitments must differ for same inputs",
            poseidon.contentEquals(sha256)
        )
    }

    // -----------------------------------------------------------------------
    // Admin Commitment (§6.2.2) — Poseidon(Poseidon(admin_root, 0), salt)
    // -----------------------------------------------------------------------

    @Test
    fun adminCommitment_bindsRosterAndSalt() {
        val admins = listOf(
            makeLeaf("0101010101010101010101010101010101010101010101010101010101010101"),
            makeLeaf("0202020202020202020202020202020202020202020202020202020202020202")
        )
        val salt = ByteArray(32) { 0x5A }

        val adminCommitment = SEPCommitmentBuilder.computeAdminCommitment(admins, salt)
        assertEquals(32, adminCommitment.size)

        // §6.2.2: admin_commitment = Poseidon(Poseidon(admin_root, 0), salt).
        // admin_epoch is pinned to 0 at group birth.
        val adminRoot = SEPCommitmentBuilder.computeMerkleRoot(admins, SEPTier.SMALL)
        val expected = SEPCommitmentBuilder.computePoseidonCommitment(adminRoot, 0L, salt)
        assertArrayEquals(adminCommitment, expected)

        // Non-zero epoch must produce a different commitment — guards against
        // regressions that drop the epoch=0 binding.
        val wrongEpoch = SEPCommitmentBuilder.computePoseidonCommitment(adminRoot, 1L, salt)
        assertFalse(adminCommitment.contentEquals(wrongEpoch))

        // Roster change → different commitment.
        val smaller = admins.take(1)
        val smallerCommitment = SEPCommitmentBuilder.computeAdminCommitment(smaller, salt)
        assertFalse(adminCommitment.contentEquals(smallerCommitment))

        // Salt change → different commitment.
        val otherSalt = ByteArray(32) { 0xA5.toByte() }
        val otherSaltCommitment = SEPCommitmentBuilder.computeAdminCommitment(admins, otherSalt)
        assertFalse(adminCommitment.contentEquals(otherSaltCommitment))
    }

    // -----------------------------------------------------------------------
    // End-to-End Pipeline with Vectors
    // -----------------------------------------------------------------------

    @Test
    fun endToEnd_singleMember_commitmentPipelineMatchesVectors() {
        // Build tree from sk=0x4242...
        val leaf = makeLeaf("4242424242424242424242424242424242424242424242424242424242424242")
        val root = SEPCommitmentBuilder.computeMerkleRoot(listOf(leaf), SEPTier.SMALL)

        // Root should match vector
        assertEquals(
            "5215123288dcf6706347bef8a27ea55d2d53bb64270e2a69934007ea34f2f04e",
            root.toHex()
        )

        // Poseidon commitment should match vector
        val poseidon = SEPCommitmentBuilder.computePoseidonCommitment(root, 0, ByteArray(32))
        assertEquals(
            "255f5a330e7fb48a50a5b1a5fbce17b717aaad9861590025b53b3a1cf527010e",
            poseidon.toHex()
        )

        // SHA-256 commitment should match vector
        val sha256 = SEPCommitmentBuilder.computeSHA256Commitment(root, 0, ByteArray(32))
        assertEquals(
            "851617b0103de7c3df520bb34b6e2ae989bd9aaa23355baa5cf033255d88fa26",
            sha256.toHex()
        )
    }
}
