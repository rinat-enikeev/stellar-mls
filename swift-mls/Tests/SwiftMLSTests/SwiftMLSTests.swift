import Foundation
import Testing
@testable import SwiftMLS

struct SwiftMLSTests {
    @Test
    func proofGenerationViaRustBridge() throws {
        let tier: SEPTier = .small
        let provingKey = try SEPProofGenerator.generateTestingProvingKey(tier: tier, seed: 7)

        let secretKeys = [fieldBytes(100), fieldBytes(200)]
        let members = try secretKeys.map { secretKey in
            SEPGroupMemberLeaf(
                publicKeyCompressed: try SEPCommitmentBuilder.computePublicKey(secretKey: secretKey),
                leafHash: try SEPCommitmentBuilder.computeLeafHash(secretKey: secretKey)
            )
        }
        let salt = Data(repeating: 0xAA, count: 32)

        let proofBundle = try SEPProofGenerator.generateMembershipProof(
            provingKey: provingKey,
            members: members,
            secretKey: secretKeys[0],
            epoch: 0,
            salt: salt,
            tier: tier
        )

        #expect(proofBundle.proof.count == 192)
        #expect(proofBundle.publicInputs.commitment.count == 32)
        #expect(proofBundle.publicInputs.epoch == 0)
    }

    @Test
    func commitmentConstruction() throws {
        let tier: SEPTier = .small
        let members = try [fieldBytes(2), fieldBytes(1)].map { secretKey in
            SEPGroupMemberLeaf(
                publicKeyCompressed: try SEPCommitmentBuilder.computePublicKey(secretKey: secretKey),
                leafHash: try SEPCommitmentBuilder.computeLeafHash(secretKey: secretKey)
            )
        }
        let root = try SEPCommitmentBuilder.computeMerkleRoot(members: members, tier: tier)
        let salt = Data(repeating: 0x11, count: 32)

        let shaCommitment = try SEPCommitmentBuilder.computeSHA256Commitment(
            poseidonRoot: root,
            epoch: 5,
            salt: salt
        )
        let poseidonCommitment = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root,
            epoch: 5,
            salt: salt
        )

        #expect(root.count == 32)
        #expect(shaCommitment.count == 32)
        #expect(poseidonCommitment.count == 32)
        #expect(shaCommitment != poseidonCommitment)
    }

    @Test
    func contractClientDelegatesToTransport() async throws {
        let transport = MockTransport()
        let client = SEPContractClient(contractID: "contract-123", transport: transport)

        let request = SEPVerifyMembershipRequest(
            groupID: Data([0x01, 0x02]),
            proof: Data(repeating: 0xAB, count: 192),
            publicInputs: SEPPublicInputs(commitment: Data(repeating: 0xCD, count: 32), epoch: 9)
        )

        let response = try await client.verifyMembership(request)

        #expect(response.valid)
        let lastInvocation = await transport.lastInvocation
        #expect(lastInvocation?.contractID == "contract-123")
        #expect(lastInvocation?.function == "verify_membership")
    }

    @Test
    func invitationSenderBuildsAndPublishesNostrEvent() async throws {
        let recipientPublicKey = Data(repeating: 0x55, count: 32)
        let relayA = URL(string: "wss://relay-a.example")!
        let relayB = URL(string: "wss://relay-b.example")!
        let bootstrap = SEPInvitationBootstrap(
            groupID: Data([0x01, 0x02, 0x03]),
            epoch: 12,
            stellarContractID: "contract-xyz",
            relayHints: [relayA, relayB],
            welcomePayload: Data([0x0A, 0x0B]),
            sepBootstrapMaterial: Data([0xAA, 0xBB, 0xCC])
        )

        let crypto = MockInvitationCrypto(
            hiddenInboxTag: "opaque-inbox-tag",
            sealedEnvelope: SEPSealedInvitationEnvelope(
                version: 1,
                scheme: "mock-box-v1",
                ephemeralPublicKey: Data([0x10, 0x11]),
                nonce: Data([0x22, 0x23]),
                ciphertext: Data([0x30, 0x31, 0x32]),
                authenticationTag: Data([0x40, 0x41])
            )
        )
        let signer = MockNostrSigner(
            signerPublicKey: Data(repeating: 0x11, count: 32),
            signature: Data(repeating: 0x22, count: 64)
        )
        let relayTransport = MockNostrRelayTransport()

        let result = try await SEPInvitationSender.sendInvitation(
            bootstrap: bootstrap,
            recipientPublicKey: recipientPublicKey,
            relayURLs: [relayB, relayA],
            cryptoProvider: crypto,
            signer: signer,
            relayTransport: relayTransport,
            options: SEPInvitationSendOptions(
                kind: 24_113,
                createdAt: 1_717_171_717,
                additionalTags: [["client", "swift-mls-test"]]
            )
        )

        #expect(result.relayResults.count == 2)
        #expect(result.relayResults.allSatisfy { $0.accepted })
        #expect(result.event.pubkey == String(repeating: "11", count: 32))
        #expect(result.event.sig == String(repeating: "22", count: 64))
        #expect(result.event.id.count == 64)
        #expect(result.event.tags == [
            ["sep_inbox", "opaque-inbox-tag"],
            ["sep_version", "1"],
            ["client", "swift-mls-test"],
        ])

        let published = await relayTransport.published
        #expect(published.count == 2)
        #expect(published.map(\.relayURL).sorted { $0.absoluteString < $1.absoluteString } == [relayA, relayB])
        #expect(published.allSatisfy { $0.event == result.event })

        let envelopeData = Data(base64Encoded: result.event.content)
        let decodedEnvelope = try JSONDecoder().decode(SEPSealedInvitationEnvelope.self, from: try #require(envelopeData))
        #expect(decodedEnvelope.scheme == "mock-box-v1")
        #expect(decodedEnvelope.ciphertext == Data([0x30, 0x31, 0x32]))
    }

    @Test
    func invitationSenderRejectsEmptyRelayList() async throws {
        let bootstrap = SEPInvitationBootstrap(
            groupID: Data([0xFF]),
            epoch: 0,
            stellarContractID: "contract-empty",
            relayHints: [],
            welcomePayload: Data(),
            sepBootstrapMaterial: Data()
        )

        await #expect(throws: SEPError.emptyRelayList) {
            try await SEPInvitationSender.sendInvitation(
                bootstrap: bootstrap,
                recipientPublicKey: Data(repeating: 0x55, count: 32),
                relayURLs: [],
                cryptoProvider: MockInvitationCrypto(
                    hiddenInboxTag: "tag",
                    sealedEnvelope: SEPSealedInvitationEnvelope(version: 1, scheme: "mock", ciphertext: Data([0x01]))
                ),
                signer: MockNostrSigner(
                    signerPublicKey: Data(repeating: 0x11, count: 32),
                    signature: Data(repeating: 0x22, count: 64)
                )
            )
        }
    }

    @Test
    func rustBackedNostrSignerProducesValidSizedOutputs() throws {
        let signer = try RustBackedNostrSigner(secretKey: fieldBytes(7))
        let publicKey = try signer.publicKey()

        #expect(publicKey.count == 32)

        let eventID = try SEPInvitationSender.eventID(
            pubkeyHex: String(repeating: "11", count: 32),
            createdAt: 1_700_000_000,
            kind: 24_113,
            tags: [["sep_inbox", "opaque"]],
            content: "AQID"
        )
        let signature = try signer.signEventID(eventID)

        #expect(eventID.count == 32)
        #expect(signature.count == 64)
    }

    private func fieldBytes(_ value: UInt64) -> Data {
        var bytes = Data(repeating: 0, count: 32)
        var bigEndian = value.bigEndian
        withUnsafeBytes(of: &bigEndian) { rawBuffer in
            bytes.replaceSubrange(24..<32, with: rawBuffer)
        }
        return bytes
    }
}

private actor MockTransport: SEPContractTransport {
    var lastInvocation: CapturedInvocation?

    func invoke<Payload, Response>(
        _ invocation: SEPContractInvocation<Payload>,
        responseType: Response.Type
    ) async throws -> Response where Payload: Encodable & Sendable, Response: Decodable & Sendable {
        lastInvocation = CapturedInvocation(contractID: invocation.contractID, function: invocation.function)

        if responseType == SEPVerifyMembershipResponse.self {
            return SEPVerifyMembershipResponse(valid: true) as! Response
        }

        if responseType == SEPSubmissionResponse.self {
            return SEPSubmissionResponse(accepted: true, transactionHash: "txhash", message: nil) as! Response
        }

        if responseType == SEPCommitmentEntry.self {
            return SEPCommitmentEntry(
                commitment: Data(repeating: 0x01, count: 32),
                epoch: 0,
                timestamp: 0,
                tier: 0,
                active: true
            ) as! Response
        }

        if responseType == [SEPCommitmentEntry].self {
            return [SEPCommitmentEntry(
                commitment: Data(repeating: 0x02, count: 32),
                epoch: 1,
                timestamp: 1,
                tier: 0,
                active: true
            )] as! Response
        }

        return SEPEmptyResponse() as! Response
    }
}

private struct CapturedInvocation: Sendable, Equatable {
    let contractID: String
    let function: String
}

private struct MockInvitationCrypto: SEPInvitationCryptoProvider {
    let hiddenInboxTag: String
    let sealedEnvelope: SEPSealedInvitationEnvelope

    func hiddenInboxTag(recipientPublicKey: Data) throws -> String {
        hiddenInboxTag
    }

    func sealInvitation(_ plaintext: Data, recipientPublicKey: Data) throws -> SEPSealedInvitationEnvelope {
        sealedEnvelope
    }
}

private struct MockNostrSigner: SEPNostrEventSigner {
    let signerPublicKey: Data
    let signature: Data

    func publicKey() throws -> Data {
        signerPublicKey
    }

    func signEventID(_ eventID: Data) throws -> Data {
        signature
    }
}

private actor MockNostrRelayTransport: SEPNostrRelayTransport {
    private(set) var published: [(relayURL: URL, event: SEPNostrEvent)] = []

    func publish(event: SEPNostrEvent, to relayURL: URL) async throws -> SEPNostrRelaySendResult {
        published.append((relayURL, event))
        return SEPNostrRelaySendResult(relayURL: relayURL, accepted: true, message: "ok")
    }
}
