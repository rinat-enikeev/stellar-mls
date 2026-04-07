import Foundation
import Testing

@testable import StellarChat

@Suite("StorageEncryption")
struct StorageEncryptionTests {
    @Test("Encrypt and decrypt round-trip for Data")
    func dataRoundtrip() throws {
        let original = Data("Hello, encrypted storage!".utf8)
        let encrypted = try StorageEncryption.encrypt(original)
        let decrypted = try StorageEncryption.decrypt(encrypted)
        #expect(decrypted == original)
    }

    @Test("Encrypt and decrypt round-trip for String")
    func stringRoundtrip() throws {
        let original = "Secret group name"
        let encrypted = try StorageEncryption.encrypt(original)
        let decrypted = try StorageEncryption.decryptString(encrypted)
        #expect(decrypted == original)
    }

    @Test("Encrypted output is longer than input (nonce + tag overhead)")
    func encryptedOutputSize() throws {
        let plaintext = Data("test".utf8)
        let encrypted = try StorageEncryption.encrypt(plaintext)
        // Must include at least 12 (nonce) + 16 (tag) = 28 bytes overhead
        #expect(encrypted.count >= plaintext.count + 28)
    }

    @Test("Two encryptions of same plaintext produce different ciphertext")
    func uniqueNonces() throws {
        let plaintext = Data("same data".utf8)
        let e1 = try StorageEncryption.encrypt(plaintext)
        let e2 = try StorageEncryption.encrypt(plaintext)
        #expect(e1 != e2, "Each encryption must use a fresh random nonce")
    }

    @Test("Empty data encrypts and decrypts")
    func emptyData() throws {
        let original = Data()
        let encrypted = try StorageEncryption.encrypt(original)
        let decrypted = try StorageEncryption.decrypt(encrypted)
        #expect(decrypted == original)
    }

    @Test("Large data encrypts and decrypts")
    func largeData() throws {
        let original = Data(repeating: 0x42, count: 100_000)
        let encrypted = try StorageEncryption.encrypt(original)
        let decrypted = try StorageEncryption.decrypt(encrypted)
        #expect(decrypted == original)
    }
}
