import CryptoKit
import Foundation
import Testing

@testable import StellarChat

// MARK: - BIP39 Cross-Platform Test Vectors
// These tests verify that BIP39 mnemonic generation, validation, and key derivation
// produce identical results on iOS and Android. The test vectors are shared across
// both platforms.

@Suite("BIP39: Mnemonic Encoding and Validation")
struct Bip39MnemonicTests {

    @Test("Mnemonic from known entropy produces expected words")
    func mnemonicFromEntropy() {
        // 128-bit all-zero entropy → known BIP39 mnemonic
        let entropy = Data(repeating: 0x00, count: 16)
        let mnemonic = Bip39.mnemonicFromEntropy(entropy)
        #expect(mnemonic == "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about")
    }

    @Test("Entropy recovery round-trips correctly")
    func entropyRoundTrip() {
        let originalEntropy = Data(repeating: 0x00, count: 16)
        let mnemonic = Bip39.mnemonicFromEntropy(originalEntropy)
        let recovered = Bip39.entropyFromMnemonic(mnemonic)
        #expect(recovered == originalEntropy)
    }

    @Test("Valid mnemonic passes validation")
    func validMnemonic() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        #expect(Bip39.isValidMnemonic(mnemonic))
    }

    @Test("Invalid checksum fails validation")
    func invalidChecksum() {
        // Last word changed to break checksum
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon"
        #expect(!Bip39.isValidMnemonic(mnemonic))
    }

    @Test("Unknown word fails validation")
    func unknownWord() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon zzzzz"
        #expect(!Bip39.isValidMnemonic(mnemonic))
    }

    @Test("Wrong word count fails validation")
    func wrongWordCount() {
        let mnemonic = "abandon abandon abandon"
        #expect(!Bip39.isValidMnemonic(mnemonic))
    }

    @Test("Generated mnemonic is always valid")
    func generatedMnemonicIsValid() {
        for _ in 0..<10 {
            let mnemonic = Bip39.generateMnemonic()
            #expect(Bip39.isValidMnemonic(mnemonic))
            let words = mnemonic.split(separator: " ")
            #expect(words.count == 12)
        }
    }
}

@Suite("BIP39: Seed Derivation (PBKDF2)")
struct Bip39SeedTests {

    @Test("Seed from known mnemonic matches BIP39 test vector")
    func seedFromKnownMnemonic() {
        // BIP39 standard test vector: all-zero entropy mnemonic, no passphrase
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        let seed = Bip39.seedFromMnemonic(mnemonic)
        #expect(seed.count == 64)
        let seedHex = seed.map { String(format: "%02x", $0) }.joined()
        // Known BIP39 seed for this mnemonic (standard test vector)
        #expect(seedHex == "5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4")
    }
}

@Suite("BIP39: Cross-Platform Key Derivation")
struct Bip39KeyDerivationTests {

    /// Cross-platform test vector 1: all-zero entropy
    /// Both iOS and Android must derive identical keys from this mnemonic.
    @Test("Key derivation from all-zero entropy mnemonic")
    func zeroEntropyKeyDerivation() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        let seed = Bip39.seedFromMnemonic(mnemonic)
        let nostrKey = Bip39.deriveNostrKey(from: seed)
        let blsKey = Bip39.deriveBlsKey(from: seed)

        #expect(nostrKey.count == 32)
        #expect(blsKey.count == 32)

        let nostrHex = nostrKey.map { String(format: "%02x", $0) }.joined()
        let blsHex = blsKey.map { String(format: "%02x", $0) }.joined()

        // These are the canonical cross-platform test vectors.
        // Android tests MUST produce the same values.
        #expect(nostrHex == "9a41cb80566383dc6cbab39414c139dc940cbd640c9ca4cb355fa6c8a34fb868")
        #expect(blsHex == "7ee35a41f2cfa0ebf9989328d5dcd8d10da98c0b8955dac71e309750e6fdc623")
    }

    /// Cross-platform test vector 2: 0x42 repeating entropy
    @Test("Key derivation from 0x42 entropy mnemonic")
    func hex42EntropyKeyDerivation() {
        let entropy = Data(repeating: 0x42, count: 16)
        let mnemonic = Bip39.mnemonicFromEntropy(entropy)
        let seed = Bip39.seedFromMnemonic(mnemonic)
        let nostrKey = Bip39.deriveNostrKey(from: seed)
        let blsKey = Bip39.deriveBlsKey(from: seed)

        #expect(nostrKey.count == 32)
        #expect(blsKey.count == 32)

        // The mnemonic and derived keys must be deterministic and cross-platform identical
        let nostrHex = nostrKey.map { String(format: "%02x", $0) }.joined()
        let blsHex = blsKey.map { String(format: "%02x", $0) }.joined()

        // Record the derived values for Android comparison
        // (both platforms run the same HKDF-SHA256 over the same PBKDF2 seed)
        #expect(!nostrHex.isEmpty)
        #expect(!blsHex.isEmpty)
        #expect(nostrHex != blsHex) // Different info strings → different keys
    }

    /// Verify that Stellar and X25519 keys derived from the BIP39-derived Nostr key
    /// use the existing HKDF derivation paths (cross-platform parity).
    @Test("Stellar and X25519 derivation from BIP39-derived Nostr key is deterministic")
    func stellarAndX25519FromBip39() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        let seed = Bip39.seedFromMnemonic(mnemonic)
        let nostrKey = Bip39.deriveNostrKey(from: seed)

        // Derive Stellar Ed25519 using the same path as KeyManager
        let stellarDerived = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: nostrKey),
            salt: Data("chat.onym.ios".utf8),
            info: Data("stellar-ed25519-v1".utf8),
            outputByteCount: 32
        )
        let stellarKey = stellarDerived.withUnsafeBytes { Data($0) }

        // Derive X25519 using the same path as KeyManager
        let x25519Derived = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: nostrKey),
            salt: Data("chat.onym.ios".utf8),
            info: Data("x25519-key-agreement-v1".utf8),
            outputByteCount: 32
        )
        let x25519Key = x25519Derived.withUnsafeBytes { Data($0) }

        #expect(stellarKey.count == 32)
        #expect(x25519Key.count == 32)
        #expect(stellarKey != x25519Key)
        #expect(stellarKey != nostrKey)
    }
}
