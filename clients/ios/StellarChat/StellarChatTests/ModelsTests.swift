import Foundation
import SwiftMLS
import Testing

@testable import StellarChat

// MARK: - SealedEnvelope

@Suite("SealedEnvelope")
struct SealedEnvelopeTests {
    @Test("SealedEnvelope Codable round-trip preserves all fields")
    func codableRoundtrip() throws {
        let envelope = SealedEnvelope(
            version: 1,
            scheme: "aes-256-gcm-v1",
            ephemeralPublicKey: nil,
            ephemeralKeySignature: nil,
            senderEd25519PublicKey: nil,
            nonce: Data(repeating: 0x01, count: 12),
            ciphertext: Data(repeating: 0xFF, count: 64),
            authenticationTag: Data(repeating: 0xAB, count: 16)
        )

        let encoded = try JSONEncoder().encode(envelope)
        let decoded = try JSONDecoder().decode(SealedEnvelope.self, from: encoded)

        #expect(decoded.version == 1)
        #expect(decoded.scheme == "aes-256-gcm-v1")
        #expect(decoded.nonce == envelope.nonce)
        #expect(decoded.ciphertext == envelope.ciphertext)
        #expect(decoded.authenticationTag == envelope.authenticationTag)
        #expect(decoded.ephemeralPublicKey == nil)
    }

    @Test("Invitation SealedEnvelope preserves optional fields")
    func invitationFields() throws {
        let envelope = SealedEnvelope(
            version: 1,
            scheme: "x25519-aes-256-gcm-v1",
            ephemeralPublicKey: Data(repeating: 0x01, count: 32),
            ephemeralKeySignature: Data(repeating: 0x02, count: 64),
            senderEd25519PublicKey: Data(repeating: 0x03, count: 32),
            nonce: Data(repeating: 0x04, count: 12),
            ciphertext: Data(repeating: 0x05, count: 128),
            authenticationTag: Data(repeating: 0x06, count: 16)
        )

        let encoded = try JSONEncoder().encode(envelope)
        let decoded = try JSONDecoder().decode(SealedEnvelope.self, from: encoded)

        #expect(decoded.ephemeralPublicKey?.count == 32)
        #expect(decoded.ephemeralKeySignature?.count == 64)
        #expect(decoded.senderEd25519PublicKey?.count == 32)
    }
}

// MARK: - KeyAttestation

@Suite("KeyAttestation")
struct KeyAttestationTests {
    @Test("KeyAttestation creation produces valid signature")
    func createAndVerify() throws {
        let keyManager = try KeyManager()
        let attestation = try keyManager.createAttestation()

        #expect(attestation.blsPubkey.count == 48)
        #expect(attestation.ed25519Pubkey.count == 32)
        #expect(attestation.signature.count == 64)
        #expect(attestation.verify() == true)
    }

    @Test("KeyAttestation with tampered signature fails verification")
    func tamperedSignatureFails() throws {
        let keyManager = try KeyManager()
        let attestation = try keyManager.createAttestation()

        var tamperedSig = attestation.signature
        tamperedSig[0] ^= 0xFF

        let tampered = KeyAttestation(
            blsPubkey: attestation.blsPubkey,
            ed25519Pubkey: attestation.ed25519Pubkey,
            signature: tamperedSig
        )
        #expect(tampered.verify() == false)
    }

    @Test("Binding message is deterministic for same BLS pubkey")
    func bindingMessageDeterministic() {
        let blsPubkey = Data(repeating: 0x42, count: 48)
        let msg1 = KeyAttestation.bindingMessage(blsPubkey: blsPubkey)
        let msg2 = KeyAttestation.bindingMessage(blsPubkey: blsPubkey)
        #expect(msg1 == msg2)
    }
}

// MARK: - MediaAttachment

@Suite("MediaAttachment")
struct MediaAttachmentTests {
    @Test("MediaAttachment Codable round-trip")
    func codableRoundtrip() throws {
        let attachment = MediaAttachment(
            blobHash: "abc123",
            fileKey: Data(repeating: 0x42, count: 32),
            mimeType: "image/jpeg",
            width: 1920,
            height: 1080,
            size: 204800,
            blossomServers: ["https://blossom.example.com"],
            encryptedThumbnail: Data(repeating: 0xFF, count: 100),
            duration: nil
        )

        let encoded = try JSONEncoder().encode(attachment)
        let decoded = try JSONDecoder().decode(MediaAttachment.self, from: encoded)

        #expect(decoded.blobHash == "abc123")
        #expect(decoded.fileKey.count == 32)
        #expect(decoded.mimeType == "image/jpeg")
        #expect(decoded.width == 1920)
        #expect(decoded.height == 1080)
        #expect(decoded.size == 204800)
        #expect(decoded.blossomServers == ["https://blossom.example.com"])
        #expect(decoded.encryptedThumbnail?.count == 100)
        #expect(decoded.duration == nil)
    }

    @Test("MediaAttachment with video duration")
    func videoDuration() throws {
        let attachment = MediaAttachment(
            blobHash: "video123",
            fileKey: Data(repeating: 0x01, count: 32),
            mimeType: "video/mp4",
            width: 1280,
            height: 720,
            size: 5_000_000,
            blossomServers: [],
            encryptedThumbnail: nil,
            duration: 120
        )

        let encoded = try JSONEncoder().encode(attachment)
        let decoded = try JSONDecoder().decode(MediaAttachment.self, from: encoded)
        #expect(decoded.duration == 120)
    }
}

// MARK: - ChatGroup

@Suite("ChatGroup Model")
struct ChatGroupModelTests {
    @Test("topicTag uses GroupCrypto.hiddenGroupTopic")
    func topicTagDerivation() {
        let secret = Data(repeating: 0x42, count: 32)
        let group = ChatGroup(
            id: "00",
            name: "test",
            groupSecret: secret,
            createdAt: Date(),
            relayHints: []
        )
        let expected = GroupCrypto.hiddenGroupTopic(groupSecret: secret)
        #expect(group.topicTag == expected)
    }

    @Test("Default epoch is 0")
    func defaultEpoch() {
        let group = ChatGroup(
            id: "00",
            name: "test",
            groupSecret: Data(repeating: 0, count: 32),
            createdAt: Date(),
            relayHints: []
        )
        #expect(group.epoch == 0)
    }

    @Test("Default tier is large")
    func defaultTier() {
        let group = ChatGroup(
            id: "00",
            name: "test",
            groupSecret: Data(repeating: 0, count: 32),
            createdAt: Date(),
            relayHints: []
        )
        #expect(group.tier == .large)
    }
}

// MARK: - EpochSnapshot

@Suite("EpochSnapshot")
struct EpochSnapshotTests {
    @Test("EpochSnapshot preserves all fields")
    func preservesFields() {
        let members = [
            SEPGroupMemberLeaf(
                publicKeyCompressed: Data(repeating: 0x01, count: 48),
                leafHash: Data(repeating: 0xAA, count: 32)
            )
        ]
        let snapshot = EpochSnapshot(
            epoch: 5,
            members: members,
            salt: Data(repeating: 0xBB, count: 32),
            groupSecret: Data(repeating: 0xCC, count: 32),
            changeDescription: "Member added"
        )
        #expect(snapshot.epoch == 5)
        #expect(snapshot.members.count == 1)
        #expect(snapshot.changeDescription == "Member added")
    }
}

// MARK: - OnChainService.fetchOnChainStateAwaitingEpoch

/// Regression tests for the chain-first receive-path retry helper. The
/// receive path rejects an update when the on-chain commitment at the
/// update's epoch disagrees with what the envelope claims. Without bounded
/// retry, an RPC node that trails the network by one ledger close would
/// cause a legitimate update to be silently dropped. These tests lock in
/// the retry semantics so a future refactor can't regress to single-shot
/// polling.
@Suite("OnChainService.fetchOnChainStateAwaitingEpoch")
struct ChainStateRetryTests {

    private func entry(epoch: UInt64, active: Bool = true) -> SEPCommitmentEntry {
        SEPCommitmentEntry(
            commitment: Data(repeating: 0x11, count: 32),
            epoch: epoch,
            timestamp: 0,
            tier: 0,
            active: active
        )
    }

    @Test("returns immediately when chain already at expected epoch")
    func returnsImmediatelyWhenAtExpectedEpoch() async throws {
        var calls = 0
        let result = try await OnChainService.fetchOnChainStateAwaitingEpoch(
            fetch: {
                calls += 1
                return self.entry(epoch: 5)
            },
            expectedEpoch: 5,
            maxAttempts: 4,
            initialDelayMs: 0,
            sleep: { _ in }
        )
        #expect(result.epoch == 5)
        #expect(calls == 1)
    }

    @Test("retries until epoch advances, simulating RPC lag")
    func retriesUntilEpochAdvances() async throws {
        let epochsToReturn: [UInt64] = [3, 3, 5]
        var attempt = 0
        var epochsSeen: [UInt64] = []
        let result = try await OnChainService.fetchOnChainStateAwaitingEpoch(
            fetch: {
                let e = epochsToReturn[attempt]
                attempt += 1
                epochsSeen.append(e)
                return self.entry(epoch: e)
            },
            expectedEpoch: 5,
            maxAttempts: 4,
            initialDelayMs: 0,
            sleep: { _ in }
        )
        #expect(result.epoch == 5)
        #expect(epochsSeen == [3, 3, 5])
    }

    @Test("returns last stale entry when chain never advances within budget")
    func returnsLastStaleEntry() async throws {
        var calls = 0
        let result = try await OnChainService.fetchOnChainStateAwaitingEpoch(
            fetch: {
                calls += 1
                return self.entry(epoch: 3)
            },
            expectedEpoch: 5,
            maxAttempts: 3,
            initialDelayMs: 0,
            sleep: { _ in }
        )
        #expect(result.epoch == 3)
        #expect(calls == 3)
    }

    @Test("rethrows when every attempt fails")
    func rethrowsWhenAllAttemptsFail() async throws {
        struct RPCDown: Error {}
        var calls = 0
        await #expect(throws: RPCDown.self) {
            try await OnChainService.fetchOnChainStateAwaitingEpoch(
                fetch: {
                    calls += 1
                    throw RPCDown()
                },
                expectedEpoch: 5,
                maxAttempts: 3,
                initialDelayMs: 0,
                sleep: { _ in }
            )
        }
        #expect(calls == 3)
    }

    @Test("returns successful entry even after a transient error")
    func recoversFromTransientError() async throws {
        struct Transient: Error {}
        var attempt = 0
        let result = try await OnChainService.fetchOnChainStateAwaitingEpoch(
            fetch: {
                attempt += 1
                if attempt == 1 { throw Transient() }
                return self.entry(epoch: 7)
            },
            expectedEpoch: 7,
            maxAttempts: 4,
            initialDelayMs: 0,
            sleep: { _ in }
        )
        #expect(result.epoch == 7)
        #expect(attempt == 2)
    }
}
