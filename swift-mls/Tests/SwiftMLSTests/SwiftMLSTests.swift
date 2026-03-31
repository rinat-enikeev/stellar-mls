import Foundation
import Testing
@testable import SwiftMLS

struct SwiftMLSTests {
    @Test
    func proofGenerationViaRustBridge() throws {
        let tier: SEPTier = .small
        let provingKey = try SEPProofGenerator.generateTestingProvingKey(tier: tier, seed: 7)

        let secretKeys = [fieldBytes(100), fieldBytes(200)]
        let leafHashes = try secretKeys.map { try SEPCommitmentBuilder.computeLeafHash(secretKey: $0) }
        let salt = Data(repeating: 0xAA, count: 32)

        let proofBundle = try SEPProofGenerator.generateMembershipProof(
            provingKey: provingKey,
            leafHashes: leafHashes,
            secretKey: secretKeys[0],
            proverIndex: 0,
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
        let leafHashes = try [fieldBytes(1), fieldBytes(2)].map {
            try SEPCommitmentBuilder.computeLeafHash(secretKey: $0)
        }
        let root = try SEPCommitmentBuilder.computeMerkleRoot(leafHashes: leafHashes, tier: tier)
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
