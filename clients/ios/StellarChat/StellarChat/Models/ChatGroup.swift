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
    var commitment: Data?   // latest verified commitment (Poseidon variant)
    var tier: SEPTier = .large
    var isPublishedOnChain: Bool = false
    /// Unix timestamp of the last received event, used for offline catch-up.
    var lastEventTimestamp: Int64 = 0
    /// When set, the user is pinned to this epoch (epoch branching).
    /// nil = follow latest epoch (default behavior).
    var pinnedEpoch: UInt64?

    /// Nostr subscription topic tag for this group.
    /// Derivation (SEP-XXXX §3.1): `topicTag = hex(SHA-256("sep-topic-v1" || groupSecret)[0..8])`
    /// Both platforms MUST use this same derivation to ensure cross-platform message visibility.
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

    /// Recompute Merkle root and Poseidon commitment from current member list.
    mutating func recomputeCommitment() throws {
        let root = try SEPCommitmentBuilder.computeMerkleRoot(members: members, tier: tier)
        let newCommitment = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: root,
            epoch: epoch,
            salt: salt
        )
        self.commitment = newCommitment
    }

    /// Add a member and recompute the commitment.
    /// Members are sorted by compressed G1 public key per SEP-XXXX §2.1.
    /// Salt is derived deterministically from the previous salt and the new member's key.
    mutating func addMember(_ leaf: SEPGroupMemberLeaf) throws {
        guard members.count < tier.maxMembers else { return }
        members.append(leaf)
        members.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }
        epoch += 1
        salt = SEPCommitmentBuilder.deriveSalt(previousSalt: salt, memberKey: leaf.publicKeyCompressed)
        try recomputeCommitment()
    }
}

/// Captures group state at a specific epoch for epoch branching.
struct EpochSnapshot: Codable {
    let epoch: UInt64
    let members: [SEPGroupMemberLeaf]
    let salt: Data
    let groupSecret: Data
    let changeDescription: String

    var topicTag: String {
        GroupCrypto.hiddenGroupTopic(groupSecret: groupSecret)
    }

    func encryptionKey() -> SymmetricKey {
        GroupCrypto.deriveMessageKey(groupSecret: groupSecret, epoch: epoch, salt: salt)
    }
}

enum MessageStatus: String, Codable {
    case sending
    case sent
    case delivered
    case failed
}

struct ChatMessage: Identifiable, Codable {
    let id: String
    let groupID: String
    let senderPubkey: String
    let text: String
    let timestamp: Date
    let isMine: Bool
    var status: MessageStatus
    let mediaAttachment: MediaAttachment?
    var isSystemMessage: Bool
    /// The epoch this message was sent on (for epoch branching filtering).
    var epoch: UInt64?

    init(id: String, groupID: String, senderPubkey: String, text: String, timestamp: Date, isMine: Bool, status: MessageStatus = .sent, mediaAttachment: MediaAttachment? = nil, isSystemMessage: Bool = false, epoch: UInt64? = nil) {
        self.id = id
        self.groupID = groupID
        self.senderPubkey = senderPubkey
        self.text = text
        self.timestamp = timestamp
        self.isMine = isMine
        self.status = status
        self.mediaAttachment = mediaAttachment
        self.isSystemMessage = isSystemMessage
        self.epoch = epoch
    }
}

/// Encrypted media attachment metadata, embedded inside E2EE message envelopes.
struct MediaAttachment: Codable {
    let blobHash: String           // SHA-256 hex of encrypted blob on Blossom
    let fileKey: Data              // 32-byte AES-256-GCM per-file key
    let mimeType: String           // e.g. "image/jpeg" or "video/mp4"
    let width: Int
    let height: Int
    let size: Int                  // encrypted blob size in bytes
    let blossomServers: [String]   // server base URLs for retrieval
    let encryptedThumbnail: Data?  // combined-format encrypted thumbnail
    let duration: Int?             // video duration in seconds, nil for images

    var isVideo: Bool { mimeType.hasPrefix("video/") }
    var isAudio: Bool { mimeType.hasPrefix("audio/") }
}

struct InviteCode: Codable {
    let groupID: Data
    let groupSecret: Data
    let name: String
    let relayHints: [String]
    /// Full group state — required for joiners to share the same encryption key and member list.
    let members: [SEPGroupMemberLeaf]
    let epoch: UInt64
    let salt: Data
    let commitment: Data?
    let tierRawValue: Int

    init(groupID: Data, groupSecret: Data, name: String, relayHints: [String], members: [SEPGroupMemberLeaf], epoch: UInt64, salt: Data, commitment: Data?, tierRawValue: Int = SEPTier.large.rawValue) {
        self.groupID = groupID
        self.groupSecret = groupSecret
        self.name = name
        self.relayHints = relayHints
        self.members = members
        self.epoch = epoch
        self.salt = salt
        self.commitment = commitment
        self.tierRawValue = tierRawValue
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        groupID = try container.decode(Data.self, forKey: .groupID)
        groupSecret = try container.decode(Data.self, forKey: .groupSecret)
        name = try container.decode(String.self, forKey: .name)
        relayHints = try container.decode([String].self, forKey: .relayHints)
        members = try container.decode([SEPGroupMemberLeaf].self, forKey: .members)
        epoch = try container.decode(UInt64.self, forKey: .epoch)
        salt = try container.decode(Data.self, forKey: .salt)
        commitment = try container.decodeIfPresent(Data.self, forKey: .commitment)
        tierRawValue = try container.decodeIfPresent(Int.self, forKey: .tierRawValue) ?? SEPTier.large.rawValue
    }

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
