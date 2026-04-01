import CryptoKit
import Foundation
import SwiftMLS

struct ChatGroup: Identifiable, Codable {
    let id: String          // hex-encoded 32-byte group ID
    let name: String
    let groupSecret: Data   // 32-byte shared secret
    let createdAt: Date
    var relayHints: [URL]

    // SEP membership state
    var members: [SEPGroupMemberLeaf] = []
    var epoch: UInt64 = 0
    var salt: Data = SEPCommitmentBuilder.generateSalt()
    var commitment: Data?   // latest verified commitment (SHA-256 variant)
    var tier: SEPTier = .small
    var isPublishedOnChain: Bool = false

    var topicTag: String {
        GroupCrypto.hiddenGroupTopic(groupSecret: groupSecret)
    }

    var encryptionKey: SymmetricKey {
        GroupCrypto.deriveMessageKey(groupSecret: groupSecret, epoch: epoch, salt: salt)
    }

    /// Group ID as raw bytes (converts hex string back to 32-byte Data).
    var groupIDData: Data {
        let bytes = stride(from: 0, to: id.count, by: 2).compactMap { i -> UInt8? in
            let start = id.index(id.startIndex, offsetBy: i)
            let end = id.index(start, offsetBy: 2)
            return UInt8(id[start..<end], radix: 16)
        }
        return Data(bytes)
    }

    /// Recompute Merkle root and commitment from current member list.
    mutating func recomputeCommitment() throws {
        let root = try SEPCommitmentBuilder.computeMerkleRoot(members: members, tier: tier)
        let newCommitment = try SEPCommitmentBuilder.computeSHA256Commitment(
            poseidonRoot: root,
            epoch: epoch,
            salt: salt
        )
        self.commitment = newCommitment
    }

    /// Add a member and recompute the commitment.
    /// Members are sorted by compressed G1 public key per SEP-XXXX §2.1.
    mutating func addMember(_ leaf: SEPGroupMemberLeaf) throws {
        guard members.count < tier.maxMembers else { return }
        members.append(leaf)
        members.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }
        epoch += 1
        salt = SEPCommitmentBuilder.generateSalt()
        try recomputeCommitment()
    }
}

struct ChatMessage: Identifiable, Codable {
    let id: String
    let groupID: String
    let senderPubkey: String
    let text: String
    let timestamp: Date
    let isMine: Bool
}

struct InviteCode: Codable {
    let groupID: Data
    let groupSecret: Data
    let name: String
    let relayHints: [String]

    func encode() -> String {
        let data = try! JSONEncoder().encode(self)
        return data.base64EncodedString()
    }

    static func decode(from string: String) throws -> InviteCode {
        guard let data = Data(base64Encoded: string) else {
            throw ChatError.invalidInviteCode
        }
        return try JSONDecoder().decode(InviteCode.self, from: data)
    }
}

enum ChatError: LocalizedError {
    case invalidInviteCode
    case encryptionFailed
    case decryptionFailed
    case noKey
    case verificationFailed(String)
    case relayPublishFailed
    case contractNotConfigured
    case onChainPublishFailed(String)

    var errorDescription: String? {
        switch self {
        case .invalidInviteCode: return "Invalid invite code"
        case .encryptionFailed: return "Failed to encrypt message"
        case .decryptionFailed: return "Failed to decrypt message"
        case .noKey: return "No signing key available"
        case .verificationFailed(let reason): return "Verification failed: \(reason)"
        case .relayPublishFailed: return "Failed to publish to any relay"
        case .contractNotConfigured: return "Stellar contract not configured"
        case .onChainPublishFailed(let reason): return "On-chain publish failed: \(reason)"
        }
    }
}
