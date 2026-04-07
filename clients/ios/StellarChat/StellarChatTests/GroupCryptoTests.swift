import CryptoKit
import Foundation
import Testing

@testable import StellarChat

// MARK: - Group Message Encryption

@Suite("GroupCrypto: Group Message Encryption")
struct GroupMessageEncryptionTests {
    @Test("Encrypt and decrypt round-trip produces original plaintext")
    func encryptDecryptRoundtrip() throws {
        let key = GroupCrypto.deriveMessageKey(
            groupSecret: Data(repeating: 0x42, count: 32),
            epoch: 0,
            salt: Data(repeating: 0xAA, count: 32)
        )
        let plaintext = "Hello, Onym!"
        let envelope = try GroupCrypto.encrypt(plaintext, key: key)
        let decrypted = try GroupCrypto.decrypt(envelope, key: key)
        #expect(decrypted == plaintext)
    }

    @Test("Encrypted envelope has correct scheme")
    func envelopeScheme() throws {
        let key = GroupCrypto.deriveMessageKey(
            groupSecret: Data(repeating: 0x01, count: 32),
            epoch: 0,
            salt: Data(repeating: 0, count: 32)
        )
        let envelope = try GroupCrypto.encrypt("test", key: key)
        #expect(envelope.scheme == "aes-256-gcm-v1")
        #expect(envelope.version == 1)
    }

    @Test("Encrypted envelope nonce is 12 bytes")
    func nonceSize() throws {
        let key = GroupCrypto.deriveMessageKey(
            groupSecret: Data(repeating: 0x01, count: 32),
            epoch: 0,
            salt: Data(repeating: 0, count: 32)
        )
        let envelope = try GroupCrypto.encrypt("test", key: key)
        #expect(envelope.nonce?.count == 12)
    }

    @Test("Encrypted envelope authentication tag is 16 bytes")
    func tagSize() throws {
        let key = GroupCrypto.deriveMessageKey(
            groupSecret: Data(repeating: 0x01, count: 32),
            epoch: 0,
            salt: Data(repeating: 0, count: 32)
        )
        let envelope = try GroupCrypto.encrypt("test", key: key)
        #expect(envelope.authenticationTag?.count == 16)
    }

    @Test("Decrypt with wrong key throws")
    func decryptWrongKeyThrows() throws {
        let key1 = GroupCrypto.deriveMessageKey(
            groupSecret: Data(repeating: 0x01, count: 32),
            epoch: 0,
            salt: Data(repeating: 0, count: 32)
        )
        let key2 = GroupCrypto.deriveMessageKey(
            groupSecret: Data(repeating: 0x02, count: 32),
            epoch: 0,
            salt: Data(repeating: 0, count: 32)
        )
        let envelope = try GroupCrypto.encrypt("secret message", key: key1)
        #expect(throws: (any Error).self) {
            _ = try GroupCrypto.decrypt(envelope, key: key2)
        }
    }

    @Test("Each encryption produces a unique nonce")
    func uniqueNonces() throws {
        let key = GroupCrypto.deriveMessageKey(
            groupSecret: Data(repeating: 0x42, count: 32),
            epoch: 0,
            salt: Data(repeating: 0xAA, count: 32)
        )
        let e1 = try GroupCrypto.encrypt("same plaintext", key: key)
        let e2 = try GroupCrypto.encrypt("same plaintext", key: key)
        #expect(e1.nonce != e2.nonce, "Each encryption must use a fresh random nonce")
    }

    @Test("Message key changes with epoch (key rotation)")
    func keyRotation() {
        let secret = Data(repeating: 0x42, count: 32)
        let salt = Data(repeating: 0xAA, count: 32)
        let key0 = GroupCrypto.deriveMessageKey(groupSecret: secret, epoch: 0, salt: salt)
        let key1 = GroupCrypto.deriveMessageKey(groupSecret: secret, epoch: 1, salt: salt)

        // Serialize keys to compare
        let k0 = key0.withUnsafeBytes { Data($0) }
        let k1 = key1.withUnsafeBytes { Data($0) }
        #expect(k0 != k1, "Different epochs must produce different message keys")
    }

    @Test("Message key is 32 bytes (AES-256)")
    func keySize() {
        let key = GroupCrypto.deriveMessageKey(
            groupSecret: Data(repeating: 0x42, count: 32),
            epoch: 0,
            salt: Data(repeating: 0, count: 32)
        )
        let keyData = key.withUnsafeBytes { Data($0) }
        #expect(keyData.count == 32)
    }
}

// MARK: - Invitation Encryption

@Suite("GroupCrypto: Invitation Encryption")
struct InvitationEncryptionTests {
    @Test("Encrypt and decrypt invitation round-trip")
    func invitationRoundtrip() throws {
        let senderSigningKey = Curve25519.Signing.PrivateKey()
        let recipientKeyAgreement = Curve25519.KeyAgreement.PrivateKey()

        let payload = Data("invitation payload".utf8)
        let envelope = try GroupCrypto.encryptInvitation(
            payload,
            recipientKeyAgreementPubkey: Data(recipientKeyAgreement.publicKey.rawRepresentation),
            senderSigningKey: senderSigningKey
        )

        let decrypted = try GroupCrypto.decryptInvitation(
            envelope,
            privateKey: recipientKeyAgreement,
            senderEd25519Pubkey: Data(senderSigningKey.publicKey.rawRepresentation)
        )

        #expect(decrypted == payload)
    }

    @Test("Invitation envelope has correct scheme")
    func invitationScheme() throws {
        let recipientKey = Curve25519.KeyAgreement.PrivateKey()
        let envelope = try GroupCrypto.encryptInvitation(
            Data("test".utf8),
            recipientKeyAgreementPubkey: Data(recipientKey.publicKey.rawRepresentation)
        )
        #expect(envelope.scheme == "x25519-aes-256-gcm-v1")
        #expect(envelope.version == 1)
    }

    @Test("Invitation envelope contains ephemeral public key (32 bytes)")
    func ephemeralPublicKey() throws {
        let recipientKey = Curve25519.KeyAgreement.PrivateKey()
        let envelope = try GroupCrypto.encryptInvitation(
            Data("test".utf8),
            recipientKeyAgreementPubkey: Data(recipientKey.publicKey.rawRepresentation)
        )
        #expect(envelope.ephemeralPublicKey?.count == 32)
    }

    @Test("Signed invitation includes ephemeral key signature (64 bytes)")
    func ephemeralKeySignature() throws {
        let senderKey = Curve25519.Signing.PrivateKey()
        let recipientKey = Curve25519.KeyAgreement.PrivateKey()
        let envelope = try GroupCrypto.encryptInvitation(
            Data("test".utf8),
            recipientKeyAgreementPubkey: Data(recipientKey.publicKey.rawRepresentation),
            senderSigningKey: senderKey
        )
        #expect(envelope.ephemeralKeySignature?.count == 64)
        #expect(envelope.senderEd25519PublicKey?.count == 32)
    }

    @Test("Invitation decryption with wrong key fails")
    func wrongRecipientKeyFails() throws {
        let recipientKey = Curve25519.KeyAgreement.PrivateKey()
        let wrongKey = Curve25519.KeyAgreement.PrivateKey()

        let envelope = try GroupCrypto.encryptInvitation(
            Data("secret".utf8),
            recipientKeyAgreementPubkey: Data(recipientKey.publicKey.rawRepresentation)
        )

        #expect(throws: (any Error).self) {
            _ = try GroupCrypto.decryptInvitation(envelope, privateKey: wrongKey)
        }
    }
}
