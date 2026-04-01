import CryptoKit
import Foundation

enum GroupCrypto {
    /// Derive a hidden group topic from the group secret.
    /// Topic = first 16 hex chars of SHA256("sep-topic-v1" || groupSecret).
    static func hiddenGroupTopic(groupSecret: Data) -> String {
        var hasher = SHA256()
        hasher.update(data: Data("sep-topic-v1".utf8))
        hasher.update(data: groupSecret)
        let hash = hasher.finalize()
        return hash.prefix(8).map { String(format: "%02x", $0) }.joined()
    }

    /// Derive a hidden inbox tag for a recipient.
    /// Tag = first 16 hex chars of SHA256("sep-inbox-v1" || recipientPubkey).
    static func hiddenInboxTag(recipientPublicKey: Data) -> String {
        var hasher = SHA256()
        hasher.update(data: Data("sep-inbox-v1".utf8))
        hasher.update(data: recipientPublicKey)
        let hash = hasher.finalize()
        return hash.prefix(8).map { String(format: "%02x", $0) }.joined()
    }

    /// Derive an AES-256-GCM key for message encryption from the group secret.
    static func deriveMessageKey(groupSecret: Data) -> SymmetricKey {
        let salt = Data("sep-msg-key-v1".utf8)
        let key = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: groupSecret),
            salt: salt,
            info: Data("traffic".utf8),
            outputByteCount: 32
        )
        return key
    }

    /// Encrypt a plaintext message for the group.
    static func encrypt(_ plaintext: String, key: SymmetricKey) throws -> SealedEnvelope {
        let data = Data(plaintext.utf8)
        let nonce = AES.GCM.Nonce()
        let sealed = try AES.GCM.seal(data, using: key, nonce: nonce)

        return SealedEnvelope(
            version: 1,
            scheme: "aes-256-gcm-v1",
            ephemeralPublicKey: nil,
            nonce: Data(nonce),
            ciphertext: sealed.ciphertext,
            authenticationTag: sealed.tag
        )
    }

    /// Decrypt a sealed envelope using the group key.
    static func decrypt(_ envelope: SealedEnvelope, key: SymmetricKey) throws -> String {
        guard envelope.scheme == "aes-256-gcm-v1" else {
            throw ChatError.decryptionFailed
        }
        guard let nonceData = envelope.nonce, let tag = envelope.authenticationTag else {
            throw ChatError.decryptionFailed
        }
        let nonce = try AES.GCM.Nonce(data: nonceData)
        let sealedBox = try AES.GCM.SealedBox(
            nonce: nonce,
            ciphertext: envelope.ciphertext,
            tag: tag
        )
        let decrypted = try AES.GCM.open(sealedBox, using: key)
        guard let text = String(data: decrypted, encoding: .utf8) else {
            throw ChatError.decryptionFailed
        }
        return text
    }
}

struct SealedEnvelope: Codable {
    let version: Int
    let scheme: String
    let ephemeralPublicKey: Data?
    let nonce: Data?
    let ciphertext: Data
    let authenticationTag: Data?

    enum CodingKeys: String, CodingKey {
        case version, scheme
        case ephemeralPublicKey = "ephemeral_public_key"
        case nonce, ciphertext
        case authenticationTag = "authentication_tag"
    }
}
