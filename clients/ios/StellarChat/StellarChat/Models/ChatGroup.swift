import CryptoKit
import Foundation

struct ChatGroup: Identifiable, Codable {
    let id: String          // hex-encoded 32-byte group ID
    let name: String
    let groupSecret: Data   // 32-byte shared secret
    let createdAt: Date
    var relayHints: [URL]

    var topicTag: String {
        GroupCrypto.hiddenGroupTopic(groupSecret: groupSecret)
    }

    var encryptionKey: SymmetricKey {
        GroupCrypto.deriveMessageKey(groupSecret: groupSecret)
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

    var errorDescription: String? {
        switch self {
        case .invalidInviteCode: return "Invalid invite code"
        case .encryptionFailed: return "Failed to encrypt message"
        case .decryptionFailed: return "Failed to decrypt message"
        case .noKey: return "No signing key available"
        }
    }
}
