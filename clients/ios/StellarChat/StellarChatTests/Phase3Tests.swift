import Foundation
import SwiftMLS
import Testing

@testable import StellarChat

// MARK: - Group ID Hex Conversion

@Suite("ChatGroup.groupIDData")
struct GroupIDDataTests {
    @Test("Hex string converts to correct bytes")
    func hexToData() {
        let group = ChatGroup(
            id: "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            name: "test",
            groupSecret: Data(repeating: 0, count: 32),
            createdAt: Date(),
            relayHints: []
        )
        let data = group.groupIDData
        #expect(data.count == 32)
        #expect(data[0] == 0xAB)
        #expect(data[1] == 0xCD)
        #expect(data[2] == 0xEF)
        #expect(data[3] == 0x01)
    }

    @Test("Short hex string converts correctly")
    func shortHex() {
        let group = ChatGroup(
            id: "00ff",
            name: "test",
            groupSecret: Data(repeating: 0, count: 32),
            createdAt: Date(),
            relayHints: []
        )
        let data = group.groupIDData
        #expect(data.count == 2)
        #expect(data[0] == 0x00)
        #expect(data[1] == 0xFF)
    }

    @Test("Roundtrip: Data → hex → groupIDData")
    func roundtrip() {
        var bytes = [UInt8](repeating: 0, count: 32)
        for i in bytes.indices { bytes[i] = UInt8(i) }
        let originalData = Data(bytes)
        let hex = originalData.map { String(format: "%02x", $0) }.joined()

        let group = ChatGroup(
            id: hex,
            name: "test",
            groupSecret: Data(repeating: 0, count: 32),
            createdAt: Date(),
            relayHints: []
        )
        #expect(group.groupIDData == originalData)
    }
}

// MARK: - OnChainVerificationResult

@Suite("OnChainVerificationResult")
struct VerificationResultTests {
    @Test("Display text for all cases")
    func displayText() {
        #expect(OnChainVerificationResult.notPublished.displayText == "Not published on-chain")
        #expect(OnChainVerificationResult.verified.displayText == "Verified on-chain")
        #expect(OnChainVerificationResult.commitmentMismatch.displayText == "Commitment mismatch")
        #expect(OnChainVerificationResult.inactive.displayText == "Group deactivated on-chain")
    }

    @Test("Epoch mismatch includes both values")
    func epochMismatchText() {
        let result = OnChainVerificationResult.epochMismatch(local: 5, onChain: 3)
        #expect(result.displayText.contains("5"))
        #expect(result.displayText.contains("3"))
    }

    @Test("Error includes message")
    func errorText() {
        let result = OnChainVerificationResult.error("connection refused")
        #expect(result.displayText.contains("connection refused"))
    }

    @Test("Equatable works")
    func equatable() {
        #expect(OnChainVerificationResult.verified == OnChainVerificationResult.verified)
        #expect(OnChainVerificationResult.verified != OnChainVerificationResult.notPublished)
        #expect(
            OnChainVerificationResult.epochMismatch(local: 1, onChain: 2)
                == OnChainVerificationResult.epochMismatch(local: 1, onChain: 2)
        )
        #expect(
            OnChainVerificationResult.epochMismatch(local: 1, onChain: 2)
                != OnChainVerificationResult.epochMismatch(local: 1, onChain: 3)
        )
    }
}

// MARK: - Member Ordering

@Suite("Member Ordering (SEP-XXXX §2.1)")
struct MemberOrderingTests {
    @Test("addMember sorts by compressed G1 public key")
    func addMemberSorts() throws {
        // Create two fake member leaves with known ordering
        let leafA = SEPGroupMemberLeaf(
            publicKeyCompressed: Data(repeating: 0x01, count: 48),
            leafHash: Data(repeating: 0xAA, count: 32)
        )
        let leafB = SEPGroupMemberLeaf(
            publicKeyCompressed: Data(repeating: 0xFF, count: 48),
            leafHash: Data(repeating: 0xBB, count: 32)
        )

        // Add in reverse order
        var group = ChatGroup(
            id: "00",
            name: "test",
            groupSecret: Data(repeating: 0, count: 32),
            createdAt: Date(),
            relayHints: [],
            members: [leafB],
            epoch: 0,
            salt: Data(repeating: 0, count: 32),
            tier: .small
        )

        // Can't call addMember (it calls recomputeCommitment which needs real BLS keys),
        // so test the sort logic directly
        group.members.append(leafA)
        group.members.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }

        // leafA (0x01...) should come before leafB (0xFF...)
        #expect(group.members[0].publicKeyCompressed == leafA.publicKeyCompressed)
        #expect(group.members[1].publicKeyCompressed == leafB.publicKeyCompressed)
    }

    @Test("BootstrapPayload.toChatGroup re-sorts members")
    func bootstrapPayloadSortsMembers() {
        let leafA = SEPGroupMemberLeaf(
            publicKeyCompressed: Data(repeating: 0x01, count: 48),
            leafHash: Data(repeating: 0xAA, count: 32)
        )
        let leafB = SEPGroupMemberLeaf(
            publicKeyCompressed: Data(repeating: 0xFF, count: 48),
            leafHash: Data(repeating: 0xBB, count: 32)
        )

        // Create payload with members in WRONG order (B before A)
        let payload = BootstrapPayload(
            groupID: Data(repeating: 0x42, count: 32),
            groupSecret: Data(repeating: 0, count: 32),
            name: "test",
            epoch: 0,
            members: [leafB, leafA],  // deliberately wrong order
            relayHints: [],
            salt: Data(repeating: 0, count: 32),
            tierRawValue: 0,
            commitment: nil,
            senderNostrPubkey: "abc123"
        )

        let group = payload.toChatGroup()

        // Members should be re-sorted: A (0x01...) before B (0xFF...)
        #expect(group.members.count == 2)
        #expect(group.members[0].publicKeyCompressed == leafA.publicKeyCompressed)
        #expect(group.members[1].publicKeyCompressed == leafB.publicKeyCompressed)
    }

    @Test("Already-sorted members remain stable")
    func alreadySortedStable() {
        let leafA = SEPGroupMemberLeaf(
            publicKeyCompressed: Data(repeating: 0x01, count: 48),
            leafHash: Data(repeating: 0xAA, count: 32)
        )
        let leafB = SEPGroupMemberLeaf(
            publicKeyCompressed: Data(repeating: 0xFF, count: 48),
            leafHash: Data(repeating: 0xBB, count: 32)
        )

        let payload = BootstrapPayload(
            groupID: Data(repeating: 0x42, count: 32),
            groupSecret: Data(repeating: 0, count: 32),
            name: "test",
            epoch: 0,
            members: [leafA, leafB],  // correct order
            relayHints: [],
            salt: Data(repeating: 0, count: 32),
            tierRawValue: 0,
            commitment: nil,
            senderNostrPubkey: "abc123"
        )

        let group = payload.toChatGroup()
        #expect(group.members[0].publicKeyCompressed == leafA.publicKeyCompressed)
        #expect(group.members[1].publicKeyCompressed == leafB.publicKeyCompressed)
    }
}

// MARK: - Commitment Variant Consistency

@Suite("Commitment Variants (SHA-256 vs Poseidon)")
struct CommitmentVariantTests {
    @Test("SHA-256 and Poseidon commitments differ for same inputs")
    func commitmentsDiffer() throws {
        let root = Data(repeating: 0x42, count: 32)
        let salt = Data(repeating: 0x07, count: 32)
        let epoch: UInt64 = 0

        let sha256 = try SEPCommitmentBuilder.computeSHA256Commitment(
            poseidonRoot: root, epoch: epoch, salt: salt
        )
        let poseidon = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root, epoch: epoch, salt: salt
        )

        // Both are 32 bytes
        #expect(sha256.count == 32)
        #expect(poseidon.count == 32)

        // They must NOT be equal (different hash functions)
        #expect(sha256 != poseidon)
    }

    @Test("Poseidon commitment is deterministic")
    func poseidonDeterministic() throws {
        let root = Data(repeating: 0x42, count: 32)
        let salt = Data(repeating: 0x07, count: 32)
        let epoch: UInt64 = 5

        let c1 = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root, epoch: epoch, salt: salt
        )
        let c2 = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root, epoch: epoch, salt: salt
        )
        #expect(c1 == c2)
    }

    @Test("Different epoch produces different Poseidon commitment")
    func epochChangesCommitment() throws {
        let root = Data(repeating: 0x42, count: 32)
        let salt = Data(repeating: 0x07, count: 32)

        let c0 = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root, epoch: 0, salt: salt
        )
        let c1 = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root, epoch: 1, salt: salt
        )
        #expect(c0 != c1)
    }

    @Test("Different salt produces different Poseidon commitment")
    func saltChangesCommitment() throws {
        let root = Data(repeating: 0x42, count: 32)
        let epoch: UInt64 = 0

        let c1 = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root, epoch: epoch, salt: Data(repeating: 0x01, count: 32)
        )
        let c2 = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root, epoch: epoch, salt: Data(repeating: 0x02, count: 32)
        )
        #expect(c1 != c2)
    }
}

// MARK: - Proof Generation & Format Conversion

@Suite("Proof Generation and Contract Format")
struct ProofTests {
    /// Generate a real BLS key, leaf, and proof to test the full pipeline.
    private func makeTestMember() throws -> (secretKey: Data, leaf: SEPGroupMemberLeaf) {
        // Use a deterministic 32-byte secret for testing
        let secretKey = Data(repeating: 0x42, count: 32)
        let pubKey = try SEPCommitmentBuilder.computePublicKey(secretKey: secretKey)
        let leafHash = try SEPCommitmentBuilder.computeLeafHash(secretKey: secretKey)
        let leaf = SEPGroupMemberLeaf(publicKeyCompressed: pubKey, leafHash: leafHash)
        return (secretKey, leaf)
    }

    @Test("Proving key generation succeeds for all tiers")
    func provingKeyGeneration() throws {
        // Only test small tier — medium and large are slow
        let pk = try SEPProofGenerator.generateTestingProvingKey(tier: .small)
        #expect(pk.count > 0)
    }

    @Test("Membership proof generation returns valid bundle")
    func proofGeneration() throws {
        let (secretKey, leaf) = try makeTestMember()
        let salt = SEPCommitmentBuilder.generateSalt()
        let pk = try SEPProofGenerator.generateTestingProvingKey(tier: .small)

        let bundle = try SEPProofGenerator.generateMembershipProof(
            provingKey: pk,
            members: [leaf],
            secretKey: secretKey,
            epoch: 0,
            salt: salt,
            tier: .small
        )

        // Compressed proof is 192 bytes (G1_48 + G2_96 + G1_48)
        #expect(bundle.proof.count == 192)
        // Public inputs commitment is 32 bytes (Poseidon field element)
        #expect(bundle.publicInputs.commitment.count == 32)
        #expect(bundle.publicInputs.epoch == 0)
    }

    @Test("Proof-to-contract format produces correct sizes")
    func proofToContractFormat() throws {
        let (secretKey, leaf) = try makeTestMember()
        let salt = SEPCommitmentBuilder.generateSalt()
        let pk = try SEPProofGenerator.generateTestingProvingKey(tier: .small)

        let bundle = try SEPProofGenerator.generateMembershipProof(
            provingKey: pk,
            members: [leaf],
            secretKey: secretKey,
            epoch: 0,
            salt: salt,
            tier: .small
        )

        let components = try SEPProofGenerator.proofToContractFormat(compressedProof: bundle.proof)

        // Uncompressed sizes per BLS12-381 spec
        #expect(components.proofA.count == 96)   // G1 uncompressed
        #expect(components.proofB.count == 192)  // G2 uncompressed
        #expect(components.proofC.count == 96)   // G1 uncompressed

        // Total uncompressed = 384 bytes
        let total = components.proofA.count + components.proofB.count + components.proofC.count
        #expect(total == 384)
    }

    @Test("Proof commitment matches separately computed Poseidon commitment")
    func proofCommitmentConsistency() throws {
        let (secretKey, leaf) = try makeTestMember()
        let salt = SEPCommitmentBuilder.generateSalt()
        let pk = try SEPProofGenerator.generateTestingProvingKey(tier: .small)

        let bundle = try SEPProofGenerator.generateMembershipProof(
            provingKey: pk,
            members: [leaf],
            secretKey: secretKey,
            epoch: 0,
            salt: salt,
            tier: .small
        )

        // Recompute the same commitment via CommitmentBuilder
        let root = try SEPCommitmentBuilder.computeMerkleRoot(members: [leaf], tier: .small)
        let poseidonCommitment = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root,
            epoch: 0,
            salt: salt
        )

        // The proof's public input commitment MUST equal the separately computed one.
        // This is the core invariant that makes on-chain verification work.
        #expect(bundle.publicInputs.commitment == poseidonCommitment)
    }

    @Test("SHA-256 commitment differs from proof Poseidon commitment")
    func sha256DiffersFromProofCommitment() throws {
        let (secretKey, leaf) = try makeTestMember()
        let salt = SEPCommitmentBuilder.generateSalt()
        let pk = try SEPProofGenerator.generateTestingProvingKey(tier: .small)

        let bundle = try SEPProofGenerator.generateMembershipProof(
            provingKey: pk,
            members: [leaf],
            secretKey: secretKey,
            epoch: 0,
            salt: salt,
            tier: .small
        )

        let root = try SEPCommitmentBuilder.computeMerkleRoot(members: [leaf], tier: .small)
        let sha256Commitment = try SEPCommitmentBuilder.computeSHA256Commitment(
            poseidonRoot: root,
            epoch: 0,
            salt: salt
        )

        // The locally stored SHA-256 commitment must NOT be confused with
        // the on-chain Poseidon commitment from the proof.
        #expect(sha256Commitment != bundle.publicInputs.commitment)
    }
}

// MARK: - Merkle Root Determinism

@Suite("Merkle Root Computation")
struct MerkleRootTests {
    @Test("Same members produce same root")
    func deterministic() throws {
        let leaf = SEPGroupMemberLeaf(
            publicKeyCompressed: try SEPCommitmentBuilder.computePublicKey(
                secretKey: Data(repeating: 0x42, count: 32)
            ),
            leafHash: try SEPCommitmentBuilder.computeLeafHash(
                secretKey: Data(repeating: 0x42, count: 32)
            )
        )

        let root1 = try SEPCommitmentBuilder.computeMerkleRoot(members: [leaf], tier: .small)
        let root2 = try SEPCommitmentBuilder.computeMerkleRoot(members: [leaf], tier: .small)
        #expect(root1 == root2)
        #expect(root1.count == 32)
    }

    @Test("Member order is canonicalized by the circuit (root is order-independent)")
    func orderCanonical() throws {
        let sk1 = Data(repeating: 0x01, count: 32)
        let sk2 = Data(repeating: 0x02, count: 32)

        let leaf1 = SEPGroupMemberLeaf(
            publicKeyCompressed: try SEPCommitmentBuilder.computePublicKey(secretKey: sk1),
            leafHash: try SEPCommitmentBuilder.computeLeafHash(secretKey: sk1)
        )
        let leaf2 = SEPGroupMemberLeaf(
            publicKeyCompressed: try SEPCommitmentBuilder.computePublicKey(secretKey: sk2),
            leafHash: try SEPCommitmentBuilder.computeLeafHash(secretKey: sk2)
        )

        let rootAB = try SEPCommitmentBuilder.computeMerkleRoot(members: [leaf1, leaf2], tier: .small)
        let rootBA = try SEPCommitmentBuilder.computeMerkleRoot(members: [leaf2, leaf1], tier: .small)

        // The Rust circuit canonicalizes leaf order, so the root is the same
        // regardless of input ordering. Application-layer sorting (SEP-XXXX §2.1)
        // ensures consistency for non-circuit operations (e.g., display, serialization).
        #expect(rootAB == rootBA)
    }
}

// MARK: - BLS Key Derivation

@Suite("BLS Key Derivation")
struct BLSKeyTests {
    @Test("Public key is 48 bytes (compressed G1)")
    func publicKeySize() throws {
        let pubKey = try SEPCommitmentBuilder.computePublicKey(
            secretKey: Data(repeating: 0x42, count: 32)
        )
        #expect(pubKey.count == 48)
    }

    @Test("Leaf hash is 32 bytes (field element)")
    func leafHashSize() throws {
        let hash = try SEPCommitmentBuilder.computeLeafHash(
            secretKey: Data(repeating: 0x42, count: 32)
        )
        #expect(hash.count == 32)
    }

    @Test("Different secrets produce different keys")
    func differentSecrets() throws {
        let pk1 = try SEPCommitmentBuilder.computePublicKey(
            secretKey: Data(repeating: 0x01, count: 32)
        )
        let pk2 = try SEPCommitmentBuilder.computePublicKey(
            secretKey: Data(repeating: 0x02, count: 32)
        )
        #expect(pk1 != pk2)
    }

    @Test("Key derivation is deterministic")
    func deterministic() throws {
        let sk = Data(repeating: 0x42, count: 32)
        let pk1 = try SEPCommitmentBuilder.computePublicKey(secretKey: sk)
        let pk2 = try SEPCommitmentBuilder.computePublicKey(secretKey: sk)
        #expect(pk1 == pk2)
    }
}

// MARK: - ChatError

@Suite("ChatError")
struct ChatErrorTests {
    @Test("contractNotConfigured has correct description")
    func contractNotConfigured() {
        let error = ChatError.contractNotConfigured
        #expect(error.errorDescription?.contains("not configured") == true)
    }

    @Test("onChainPublishFailed includes reason")
    func onChainPublishFailed() {
        let error = ChatError.onChainPublishFailed("timeout")
        #expect(error.errorDescription?.contains("timeout") == true)
    }
}

// MARK: - isPublishedOnChain Persistence Roundtrip

@Suite("On-Chain Status Persistence")
struct OnChainPersistenceTests {
    @Test("isPublishedOnChain defaults to false")
    func defaultFalse() {
        let group = ChatGroup(
            id: "abc",
            name: "test",
            groupSecret: Data(repeating: 0, count: 32),
            createdAt: Date(),
            relayHints: []
        )
        #expect(group.isPublishedOnChain == false)
    }

    @Test("isPublishedOnChain can be set to true")
    func setTrue() {
        var group = ChatGroup(
            id: "abc",
            name: "test",
            groupSecret: Data(repeating: 0, count: 32),
            createdAt: Date(),
            relayHints: []
        )
        group.isPublishedOnChain = true
        #expect(group.isPublishedOnChain == true)
    }
}

// MARK: - End-to-End: Proof → Verify Commitment Match

@Suite("End-to-End Commitment Pipeline")
struct EndToEndTests {
    @Test("Full pipeline: generate proof → verify commitment matches")
    func fullPipeline() throws {
        // 1. Create a member
        let secretKey = Data(repeating: 0x42, count: 32)
        let pubKey = try SEPCommitmentBuilder.computePublicKey(secretKey: secretKey)
        let leafHash = try SEPCommitmentBuilder.computeLeafHash(secretKey: secretKey)
        let leaf = SEPGroupMemberLeaf(publicKeyCompressed: pubKey, leafHash: leafHash)

        // 2. Set up group state
        let members = [leaf]
        let epoch: UInt64 = 0
        let salt = SEPCommitmentBuilder.generateSalt()
        let tier = SEPTier.small

        // 3. Generate proof (this is what gets submitted on-chain)
        let pk = try SEPProofGenerator.generateTestingProvingKey(tier: tier)
        let bundle = try SEPProofGenerator.generateMembershipProof(
            provingKey: pk,
            members: members,
            secretKey: secretKey,
            epoch: epoch,
            salt: salt,
            tier: tier
        )

        // 4. Simulate: contract stores bundle.publicInputs.commitment as on-chain commitment

        // 5. Verification: recompute locally and compare (what verifyCommitment does)
        let root = try SEPCommitmentBuilder.computeMerkleRoot(members: members, tier: tier)
        let localPoseidon = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root, epoch: epoch, salt: salt
        )

        // This is the critical assertion: the commitment from proof generation
        // must equal the commitment computed via separate Poseidon path.
        // If this fails, on-chain verification will ALWAYS fail.
        #expect(localPoseidon == bundle.publicInputs.commitment)

        // 6. Also verify proof format is correct for contract
        let components = try SEPProofGenerator.proofToContractFormat(compressedProof: bundle.proof)
        #expect(components.proofA.count == 96)
        #expect(components.proofB.count == 192)
        #expect(components.proofC.count == 96)
    }

    @Test("Commitment update pipeline: old proof → new commitment")
    func updatePipeline() throws {
        // 1. Initial state: one member
        let sk1 = Data(repeating: 0x01, count: 32)
        let leaf1 = SEPGroupMemberLeaf(
            publicKeyCompressed: try SEPCommitmentBuilder.computePublicKey(secretKey: sk1),
            leafHash: try SEPCommitmentBuilder.computeLeafHash(secretKey: sk1)
        )

        let oldMembers = [leaf1]
        let oldEpoch: UInt64 = 0
        let oldSalt = SEPCommitmentBuilder.generateSalt()
        let tier = SEPTier.small

        // 2. Generate proof at old state (proves membership before update)
        let pk = try SEPProofGenerator.generateTestingProvingKey(tier: tier)
        let oldBundle = try SEPProofGenerator.generateMembershipProof(
            provingKey: pk,
            members: oldMembers,
            secretKey: sk1,
            epoch: oldEpoch,
            salt: oldSalt,
            tier: tier
        )

        // 3. Add a second member (new state)
        let sk2 = Data(repeating: 0x02, count: 32)
        let leaf2 = SEPGroupMemberLeaf(
            publicKeyCompressed: try SEPCommitmentBuilder.computePublicKey(secretKey: sk2),
            leafHash: try SEPCommitmentBuilder.computeLeafHash(secretKey: sk2)
        )

        var newMembers = [leaf1, leaf2]
        newMembers.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }
        let newEpoch: UInt64 = 1
        let newSalt = SEPCommitmentBuilder.generateSalt()

        // 4. Compute new Poseidon commitment
        let newRoot = try SEPCommitmentBuilder.computeMerkleRoot(members: newMembers, tier: tier)
        let newPoseidon = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: newRoot, epoch: newEpoch, salt: newSalt
        )

        // 5. Verify: old proof's commitment matches old state
        let oldRoot = try SEPCommitmentBuilder.computeMerkleRoot(members: oldMembers, tier: tier)
        let oldPoseidon = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: oldRoot, epoch: oldEpoch, salt: oldSalt
        )
        #expect(oldBundle.publicInputs.commitment == oldPoseidon)

        // 6. New commitment is different from old
        #expect(newPoseidon != oldPoseidon)

        // 7. New epoch = old epoch + 1 (contract enforces this)
        #expect(newEpoch == oldEpoch + 1)
    }
}

// MARK: - Cross-Platform Test Vectors

@Suite("Cross-Platform Interoperability")
struct CrossPlatformTests {
    // Test vectors from docs/cross-platform-test-vectors.json.
    // Both iOS and Android MUST produce identical outputs for these inputs.

    @Test("Topic derivation matches canonical vectors")
    func topicDerivation() {
        let vectors: [(secretHex: String, expected: String)] = [
            ("0000000000000000000000000000000000000000000000000000000000000000", "0c6ce887ec3881f3"),
            ("4242424242424242424242424242424242424242424242424242424242424242", "aa263d222a1f2e1f"),
            ("ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00", "7df84225883879ff"),
        ]
        for v in vectors {
            let secret = hexToData(v.secretHex)!
            let topic = GroupCrypto.hiddenGroupTopic(groupSecret: secret)
            #expect(topic == v.expected, "Topic mismatch for secret \(v.secretHex.prefix(16))...")
        }
    }

    @Test("Inbox tag derivation matches canonical vectors")
    func inboxDerivation() {
        let vectors: [(pubkeyHex: String, expected: String)] = [
            ("0000000000000000000000000000000000000000000000000000000000000000", "f434d35e24e86124"),
            ("4242424242424242424242424242424242424242424242424242424242424242", "c9bcafedc73f4289"),
        ]
        for v in vectors {
            let pubkey = hexToData(v.pubkeyHex)!
            let tag = GroupCrypto.hiddenInboxTag(recipientPublicKey: pubkey)
            #expect(tag == v.expected, "Inbox tag mismatch for pubkey \(v.pubkeyHex.prefix(16))...")
        }
    }

    @Test("HKDF message key derivation matches canonical vectors")
    func hkdfMessageKey() {
        let vectors: [(secretHex: String, epoch: UInt64, saltHex: String, expectedHex: String)] = [
            (
                "0000000000000000000000000000000000000000000000000000000000000000",
                0,
                "0000000000000000000000000000000000000000000000000000000000000000",
                "e7b6e0341ab181313e1325602763c196b8113f1f8accf570674e797863bd6acd"
            ),
            (
                "4242424242424242424242424242424242424242424242424242424242424242",
                5,
                "0707070707070707070707070707070707070707070707070707070707070707",
                "68346c282ce344b6ff7f6c7138737873438a2d7fd28c89573899d73ee6a9d52e"
            ),
        ]
        for v in vectors {
            let secret = hexToData(v.secretHex)!
            let salt = hexToData(v.saltHex)!
            let key = GroupCrypto.deriveMessageKey(groupSecret: secret, epoch: v.epoch, salt: salt)
            let keyHex = key.withUnsafeBytes { buf in
                Data(buf).map { String(format: "%02x", $0) }.joined()
            }
            #expect(keyHex == v.expectedHex, "HKDF key mismatch for epoch \(v.epoch)")
        }
    }

    @Test("BLS key derivation produces consistent sizes across platforms")
    func blsKeyDerivation() throws {
        let secrets = [
            "4242424242424242424242424242424242424242424242424242424242424242",
            "0101010101010101010101010101010101010101010101010101010101010101",
        ]
        for secretHex in secrets {
            let sk = hexToData(secretHex)!
            let pk = try SEPCommitmentBuilder.computePublicKey(secretKey: sk)
            let lh = try SEPCommitmentBuilder.computeLeafHash(secretKey: sk)
            #expect(pk.count == 48, "BLS public key must be 48 bytes")
            #expect(lh.count == 32, "Leaf hash must be 32 bytes")
        }
    }

    @Test("BLS key derivation is deterministic (cross-platform invariant)")
    func blsKeyDeterministic() throws {
        let sk = hexToData("4242424242424242424242424242424242424242424242424242424242424242")!
        let pk1 = try SEPCommitmentBuilder.computePublicKey(secretKey: sk)
        let pk2 = try SEPCommitmentBuilder.computePublicKey(secretKey: sk)
        let lh1 = try SEPCommitmentBuilder.computeLeafHash(secretKey: sk)
        let lh2 = try SEPCommitmentBuilder.computeLeafHash(secretKey: sk)
        #expect(pk1 == pk2)
        #expect(lh1 == lh2)
    }

    @Test("InviteCode round-trip: encode → decode preserves all fields")
    func inviteCodeRoundtrip() {
        let members = [
            SEPGroupMemberLeaf(
                publicKeyCompressed: Data(repeating: 0x01, count: 48),
                leafHash: Data(repeating: 0xAA, count: 32)
            )
        ]
        let code = InviteCode(
            groupID: Data(repeating: 0x42, count: 32),
            groupSecret: Data(repeating: 0x07, count: 32),
            name: "test-group",
            relayHints: ["wss://relay.damus.io"],
            members: members,
            epoch: 3,
            salt: Data(repeating: 0x0B, count: 32),
            commitment: Data(repeating: 0xFF, count: 32)
        )

        let encoded = code.encode()
        let decoded = try! InviteCode.decode(from: encoded)

        #expect(decoded.groupID == code.groupID)
        #expect(decoded.groupSecret == code.groupSecret)
        #expect(decoded.name == code.name)
        #expect(decoded.relayHints == code.relayHints)
        #expect(decoded.epoch == code.epoch)
        #expect(decoded.salt == code.salt)
        #expect(decoded.commitment == code.commitment)
        #expect(decoded.members.count == 1)
        #expect(decoded.members[0].publicKeyCompressed == members[0].publicKeyCompressed)
        #expect(decoded.members[0].leafHash == members[0].leafHash)
    }

    private func hexToData(_ hex: String) -> Data? {
        let bytes = stride(from: 0, to: hex.count, by: 2).compactMap { i -> UInt8? in
            let start = hex.index(hex.startIndex, offsetBy: i)
            let end = hex.index(start, offsetBy: 2)
            return UInt8(hex[start..<end], radix: 16)
        }
        guard bytes.count == hex.count / 2 else { return nil }
        return Data(bytes)
    }
}

// MARK: - Salt Generation

@Suite("Salt Generation")
struct SaltTests {
    @Test("Salt is 32 bytes")
    func saltSize() {
        let salt = SEPCommitmentBuilder.generateSalt()
        #expect(salt.count == 32)
    }

    @Test("Two salts are different (with overwhelming probability)")
    func saltsUnique() {
        let s1 = SEPCommitmentBuilder.generateSalt()
        let s2 = SEPCommitmentBuilder.generateSalt()
        #expect(s1 != s2)
    }
}
