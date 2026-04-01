import CryptoKit
import Foundation

enum GroupCrypto {
    /// Derive a hidden group topic from the group secret (SEP-XXXX §3.1).
    ///
    /// Canonical derivation: `topicTag = hex(SHA-256("sep-topic-v1" || groupSecret)[0..8])`
    /// Result: 16-character hex string used as the `t` tag value in Nostr kind 24114 events.
    /// This derivation MUST be identical on all platforms for cross-platform interoperability.
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

    /// Derive an AES-256-GCM key for message encryption, bound to the current epoch and salt.
    /// Key rotates on every membership change (epoch increment + new salt), ensuring
    /// removed members cannot decrypt messages sent after their removal.
    ///
    /// N-20: **No forward secrecy.** If the group secret is compromised, all past messages
    /// (for all epochs) are decryptable because the epoch key is deterministically derived
    /// from `HKDF(groupSecret || epoch || salt)`. There is no ratcheting mechanism.
    /// This is a deliberate design trade-off: the protocol favors simplicity and group-wide
    /// key sharing over forward secrecy. For higher-security deployments, consider
    /// integrating MLS-style ratcheting.
    static func deriveMessageKey(groupSecret: Data, epoch: UInt64, salt: Data) -> SymmetricKey {
        // IKM = groupSecret || epoch (big-endian) || salt
        var ikm = Data()
        ikm.append(groupSecret)
        var epochBE = epoch.bigEndian
        withUnsafeBytes(of: &epochBE) { ikm.append(contentsOf: $0) }
        ikm.append(salt)

        let key = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: ikm),
            salt: Data("sep-msg-key-v1".utf8),
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
            ephemeralKeySignature: nil,
            nonce: Data(nonce),
            ciphertext: sealed.ciphertext,
            authenticationTag: sealed.tag
        )
    }

    // MARK: - Invitation Encryption (X25519 ECDH + AES-256-GCM)

    /// Encrypt a bootstrap payload to a specific recipient using ephemeral X25519 ECDH.
    /// The sealed envelope carries the ephemeral public key so the recipient can derive the shared secret.
    /// The ephemeral public key is signed with the sender's Ed25519 identity key (M-5) to prevent
    /// MITM substitution of the ephemeral key.
    static func encryptInvitation(
        _ payload: Data,
        recipientKeyAgreementPubkey: Data,
        senderSigningKey: Curve25519.Signing.PrivateKey? = nil
    ) throws -> SealedEnvelope {
        let ephemeral = Curve25519.KeyAgreement.PrivateKey()
        let recipientPubkey = try Curve25519.KeyAgreement.PublicKey(
            rawRepresentation: recipientKeyAgreementPubkey
        )

        let sharedSecret = try ephemeral.sharedSecretFromKeyAgreement(with: recipientPubkey)
        let key = sharedSecret.hkdfDerivedSymmetricKey(
            using: SHA256.self,
            salt: Data("sep-invitation-v1".utf8),
            sharedInfo: Data("aes-256-gcm".utf8),
            outputByteCount: 32
        )

        let nonce = AES.GCM.Nonce()
        let sealed = try AES.GCM.seal(payload, using: key, nonce: nonce)

        // M-5: Sign ephemeral public key with sender's Ed25519 identity key
        let ephPubData = Data(ephemeral.publicKey.rawRepresentation)
        let ephKeySignature: Data?
        if let signingKey = senderSigningKey {
            ephKeySignature = try signingKey.signature(for: ephPubData)
        } else {
            ephKeySignature = nil
        }

        return SealedEnvelope(
            version: 1,
            scheme: "x25519-aes-256-gcm-v1",
            ephemeralPublicKey: ephPubData,
            ephemeralKeySignature: ephKeySignature,
            nonce: Data(nonce),
            ciphertext: sealed.ciphertext,
            authenticationTag: sealed.tag
        )
    }

    /// Decrypt an invitation sealed envelope using the recipient's X25519 private key.
    /// N-1: If `senderEd25519Pubkey` is provided, verifies the ephemeral key signature
    /// to prevent MITM substitution. If signature is missing or invalid, decryption is rejected.
    static func decryptInvitation(
        _ envelope: SealedEnvelope,
        privateKey: Curve25519.KeyAgreement.PrivateKey,
        senderEd25519Pubkey: Data? = nil
    ) throws -> Data {
        guard envelope.scheme == "x25519-aes-256-gcm-v1" else {
            throw ChatError.decryptionFailed
        }
        guard let ephPubData = envelope.ephemeralPublicKey,
              let nonceData = envelope.nonce,
              let tag = envelope.authenticationTag
        else { throw ChatError.decryptionFailed }

        // N-1: Verify ephemeral key signature if present
        if let sigData = envelope.ephemeralKeySignature {
            guard let senderPubkey = senderEd25519Pubkey else {
                SecurityLog.invalidAttestation(reason: "ephemeral key signature present but no sender pubkey to verify against")
                throw ChatError.decryptionFailed
            }
            let verifyingKey = try Curve25519.Signing.PublicKey(rawRepresentation: senderPubkey)
            guard verifyingKey.isValidSignature(sigData, for: ephPubData) else {
                SecurityLog.invalidAttestation(reason: "ephemeral key signature verification failed")
                throw ChatError.decryptionFailed
            }
        }

        let ephPub = try Curve25519.KeyAgreement.PublicKey(rawRepresentation: ephPubData)
        let sharedSecret = try privateKey.sharedSecretFromKeyAgreement(with: ephPub)
        let key = sharedSecret.hkdfDerivedSymmetricKey(
            using: SHA256.self,
            salt: Data("sep-invitation-v1".utf8),
            sharedInfo: Data("aes-256-gcm".utf8),
            outputByteCount: 32
        )

        let nonce = try AES.GCM.Nonce(data: nonceData)
        let box = try AES.GCM.SealedBox(nonce: nonce, ciphertext: envelope.ciphertext, tag: tag)
        return try AES.GCM.open(box, using: key)
    }

    // MARK: - Group Message Encryption

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
    /// Ed25519 signature over the ephemeral public key by the sender's identity key (M-5).
    /// Prevents MITM substitution of the ephemeral X25519 key in invitations.
    let ephemeralKeySignature: Data?
    let nonce: Data?
    let ciphertext: Data
    let authenticationTag: Data?

    enum CodingKeys: String, CodingKey {
        case version, scheme
        case ephemeralPublicKey = "ephemeral_public_key"
        case ephemeralKeySignature = "ephemeral_key_signature"
        case nonce, ciphertext
        case authenticationTag = "authentication_tag"
    }
}
