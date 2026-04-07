import Foundation
import SwiftMLS
import Testing

@testable import StellarChat

/// Cross-platform test vectors that verify iOS produces EXACT same outputs
/// as the Rust reference implementation. These match the vectors in
/// `docs/cross-platform-test-vectors.json`.
///
/// Unlike Phase3Tests.swift which checks sizes and determinism, these tests
/// compare hex-encoded outputs byte-for-byte against the canonical vectors.

@Suite("Cross-Platform Vectors: BLS Key Derivation")
struct CrossPlatformBLSVectorTests {
    private func hexToData(_ hex: String) -> Data {
        let bytes = stride(from: 0, to: hex.count, by: 2).compactMap { i -> UInt8? in
            let start = hex.index(hex.startIndex, offsetBy: i)
            let end = hex.index(start, offsetBy: 2)
            return UInt8(hex[start..<end], radix: 16)
        }
        return Data(bytes)
    }

    private func dataToHex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }

    @Test("BLS public key matches vector 1 (sk=0x4242...)")
    func publicKeyVector1() throws {
        let sk = hexToData("4242424242424242424242424242424242424242424242424242424242424242")
        let pk = try SEPCommitmentBuilder.computePublicKey(secretKey: sk)
        let pkHex = dataToHex(pk)
        #expect(pkHex == "b5b99c967e4c69822f427db1f6871dd119afb95ab9646ba2e707990a3db31777a59b66f69e89c2055699b0ade7357eae")
    }

    @Test("BLS public key matches vector 2 (sk=0x0101...)")
    func publicKeyVector2() throws {
        let sk = hexToData("0101010101010101010101010101010101010101010101010101010101010101")
        let pk = try SEPCommitmentBuilder.computePublicKey(secretKey: sk)
        let pkHex = dataToHex(pk)
        #expect(pkHex == "aa1a1c26055a329817a5759d877a2795f9499b97d6056edde0eea39512f24e8bc874b4471f0501127abb1ea0d9f68ac1")
    }

    @Test("BLS leaf hash matches vector 1 (sk=0x4242...)")
    func leafHashVector1() throws {
        let sk = hexToData("4242424242424242424242424242424242424242424242424242424242424242")
        let lh = try SEPCommitmentBuilder.computeLeafHash(secretKey: sk)
        let lhHex = dataToHex(lh)
        #expect(lhHex == "1fe19c284d0763e709dc2a0195cee0c40ae331ff7a7cd6efd16bc3c8a534e46c")
    }

    @Test("BLS leaf hash matches vector 2 (sk=0x0101...)")
    func leafHashVector2() throws {
        let sk = hexToData("0101010101010101010101010101010101010101010101010101010101010101")
        let lh = try SEPCommitmentBuilder.computeLeafHash(secretKey: sk)
        let lhHex = dataToHex(lh)
        #expect(lhHex == "5f44af31406da8025c110940b29eafcaeb42b0d34992e27dcc0ce22c11d3872e")
    }
}

@Suite("Cross-Platform Vectors: Merkle Root")
struct CrossPlatformMerkleRootVectorTests {
    private func hexToData(_ hex: String) -> Data {
        let bytes = stride(from: 0, to: hex.count, by: 2).compactMap { i -> UInt8? in
            let start = hex.index(hex.startIndex, offsetBy: i)
            let end = hex.index(start, offsetBy: 2)
            return UInt8(hex[start..<end], radix: 16)
        }
        return Data(bytes)
    }

    private func dataToHex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }

    private func makeLeaf(skHex: String) throws -> SEPGroupMemberLeaf {
        let sk = hexToData(skHex)
        return SEPGroupMemberLeaf(
            publicKeyCompressed: try SEPCommitmentBuilder.computePublicKey(secretKey: sk),
            leafHash: try SEPCommitmentBuilder.computeLeafHash(secretKey: sk)
        )
    }

    @Test("Merkle root matches vector: single member (sk=0x4242...)")
    func singleMemberRoot() throws {
        let leaf = try makeLeaf(skHex: "4242424242424242424242424242424242424242424242424242424242424242")
        let root = try SEPCommitmentBuilder.computeMerkleRoot(members: [leaf], tier: .small)
        let rootHex = dataToHex(root)
        #expect(rootHex == "5215123288dcf6706347bef8a27ea55d2d53bb64270e2a69934007ea34f2f04e")
    }

    @Test("Merkle root matches vector: two members (sk=0x4242..., sk=0x0101...)")
    func twoMembersRoot() throws {
        let leaf1 = try makeLeaf(skHex: "4242424242424242424242424242424242424242424242424242424242424242")
        let leaf2 = try makeLeaf(skHex: "0101010101010101010101010101010101010101010101010101010101010101")
        let root = try SEPCommitmentBuilder.computeMerkleRoot(members: [leaf1, leaf2], tier: .small)
        let rootHex = dataToHex(root)
        #expect(rootHex == "30bb9c0d5ca89a65ce7f16817c31999800ab316c7e0eb903fea7d594087fdd94")
    }
}

@Suite("Cross-Platform Vectors: Poseidon Commitment")
struct CrossPlatformPoseidonCommitmentVectorTests {
    private func hexToData(_ hex: String) -> Data {
        let bytes = stride(from: 0, to: hex.count, by: 2).compactMap { i -> UInt8? in
            let start = hex.index(hex.startIndex, offsetBy: i)
            let end = hex.index(start, offsetBy: 2)
            return UInt8(hex[start..<end], radix: 16)
        }
        return Data(bytes)
    }

    private func dataToHex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }

    @Test("Poseidon commitment vector 1: root=single-member, epoch=0, salt=0x00...")
    func poseidonCommitmentVector1() throws {
        let root = hexToData("5215123288dcf6706347bef8a27ea55d2d53bb64270e2a69934007ea34f2f04e")
        let salt = Data(repeating: 0x00, count: 32)
        let commitment = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root, epoch: 0, salt: salt
        )
        let hex = dataToHex(commitment)
        #expect(hex == "255f5a330e7fb48a50a5b1a5fbce17b717aaad9861590025b53b3a1cf527010e")
    }

    @Test("Poseidon commitment vector 2: root=two-member, epoch=5, salt=0xAA...")
    func poseidonCommitmentVector2() throws {
        let root = hexToData("30bb9c0d5ca89a65ce7f16817c31999800ab316c7e0eb903fea7d594087fdd94")
        let salt = Data(repeating: 0xAA, count: 32)
        let commitment = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root, epoch: 5, salt: salt
        )
        let hex = dataToHex(commitment)
        #expect(hex == "0ff7547913428aaea8c7242dbcbf1e7921b58878307b89fc430a9a32ccb56aa5")
    }
}

@Suite("Cross-Platform Vectors: SHA-256 Commitment")
struct CrossPlatformSHA256CommitmentVectorTests {
    private func hexToData(_ hex: String) -> Data {
        let bytes = stride(from: 0, to: hex.count, by: 2).compactMap { i -> UInt8? in
            let start = hex.index(hex.startIndex, offsetBy: i)
            let end = hex.index(start, offsetBy: 2)
            return UInt8(hex[start..<end], radix: 16)
        }
        return Data(bytes)
    }

    private func dataToHex(_ data: Data) -> String {
        data.map { String(format: "%02x", $0) }.joined()
    }

    @Test("SHA-256 commitment vector 1: epoch=0, salt=0x00...")
    func sha256CommitmentVector1() throws {
        let root = hexToData("5215123288dcf6706347bef8a27ea55d2d53bb64270e2a69934007ea34f2f04e")
        let salt = Data(repeating: 0x00, count: 32)
        let commitment = try SEPCommitmentBuilder.computeSHA256Commitment(
            poseidonRoot: root, epoch: 0, salt: salt
        )
        let hex = dataToHex(commitment)
        #expect(hex == "851617b0103de7c3df520bb34b6e2ae989bd9aaa23355baa5cf033255d88fa26")
    }

    @Test("SHA-256 commitment vector 2: epoch=5, salt=0xAA...")
    func sha256CommitmentVector2() throws {
        let root = hexToData("30bb9c0d5ca89a65ce7f16817c31999800ab316c7e0eb903fea7d594087fdd94")
        let salt = Data(repeating: 0xAA, count: 32)
        let commitment = try SEPCommitmentBuilder.computeSHA256Commitment(
            poseidonRoot: root, epoch: 5, salt: salt
        )
        let hex = dataToHex(commitment)
        #expect(hex == "58a1d00ea6f30a925ea91e791f934b08e815f4c345b445af5a9d072926df67a0")
    }
}
