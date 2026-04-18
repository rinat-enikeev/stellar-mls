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
    /// Both iOS and Android must derive identical keys from this mnemonic.
    @Test("Key derivation from 0x42 entropy mnemonic")
    func hex42EntropyKeyDerivation() {
        let entropy = Data(repeating: 0x42, count: 16)
        let mnemonic = Bip39.mnemonicFromEntropy(entropy)
        let seed = Bip39.seedFromMnemonic(mnemonic)
        let nostrKey = Bip39.deriveNostrKey(from: seed)
        let blsKey = Bip39.deriveBlsKey(from: seed)

        #expect(nostrKey.count == 32)
        #expect(blsKey.count == 32)

        let nostrHex = nostrKey.map { String(format: "%02x", $0) }.joined()
        let blsHex = blsKey.map { String(format: "%02x", $0) }.joined()

        // These are the canonical cross-platform test vectors.
        // Android tests MUST produce the same values.
        #expect(nostrHex == "7cbc4d71b42367f74dd2356e3ba1ec8f1f26e7ed5a8cf150c5820efb2fc701d2")
        #expect(blsHex == "0b402ebb55641736888bb315fb7c6dc49ae0a50ce066a05227f6e55f71c6a097")
    }

    /// Verify that Stellar and X25519 keys derived from the BIP39-derived Nostr key
    /// produce pinned cross-platform values (zero-entropy vector).
    @Test("Stellar and X25519 derivation from zero-entropy Nostr key matches pinned values")
    func stellarAndX25519FromBip39_zeroEntropy() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        let seed = Bip39.seedFromMnemonic(mnemonic)
        let nostrKey = Bip39.deriveNostrKey(from: seed)

        let stellarDerived = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: nostrKey),
            salt: Data("chat.onym.ios".utf8),
            info: Data("stellar-ed25519-v1".utf8),
            outputByteCount: 32
        )
        let stellarKey = stellarDerived.withUnsafeBytes { Data($0) }

        let x25519Derived = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: nostrKey),
            salt: Data("chat.onym.ios".utf8),
            info: Data("x25519-key-agreement-v1".utf8),
            outputByteCount: 32
        )
        let x25519Key = x25519Derived.withUnsafeBytes { Data($0) }

        #expect(stellarKey.count == 32)
        #expect(x25519Key.count == 32)

        // Pinned cross-platform values — Android tests MUST produce the same hex
        let stellarHex = stellarKey.map { String(format: "%02x", $0) }.joined()
        let x25519Hex = x25519Key.map { String(format: "%02x", $0) }.joined()
        #expect(stellarHex == "45d014863f395145d396e193adcd6d761c919d8db64d5356486e9fff4a32c4ca")
        #expect(x25519Hex == "b86719ab1a4317618b81cf7225be405c92dc315886d0c817da71cc12fb08cc17")
    }

    /// Verify Stellar and X25519 pinned values for 0x42 entropy vector.
    @Test("Stellar and X25519 derivation from 0x42-entropy Nostr key matches pinned values")
    func stellarAndX25519FromBip39_hex42Entropy() {
        let entropy = Data(repeating: 0x42, count: 16)
        let mnemonic = Bip39.mnemonicFromEntropy(entropy)
        let seed = Bip39.seedFromMnemonic(mnemonic)
        let nostrKey = Bip39.deriveNostrKey(from: seed)

        let stellarDerived = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: nostrKey),
            salt: Data("chat.onym.ios".utf8),
            info: Data("stellar-ed25519-v1".utf8),
            outputByteCount: 32
        )
        let stellarKey = stellarDerived.withUnsafeBytes { Data($0) }

        let x25519Derived = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: nostrKey),
            salt: Data("chat.onym.ios".utf8),
            info: Data("x25519-key-agreement-v1".utf8),
            outputByteCount: 32
        )
        let x25519Key = x25519Derived.withUnsafeBytes { Data($0) }

        #expect(stellarKey.count == 32)
        #expect(x25519Key.count == 32)

        let stellarHex = stellarKey.map { String(format: "%02x", $0) }.joined()
        let x25519Hex = x25519Key.map { String(format: "%02x", $0) }.joined()
        #expect(stellarHex == "6ec053eb7fb174f774fcd80bd6f2b7dc839e3e4e20fa99d2c01a1f85c475a2bb")
        #expect(x25519Hex == "81497994b0a24bb8b4295c0621541427253d5a02a10e00833cb03913bfd15c56")
    }
}

// MARK: - 24-Word Mnemonic (256-bit entropy)

@Suite("BIP39: 24-Word Mnemonic")
struct Bip39TwentyFourWordTests {

    @Test("24-word mnemonic from 256-bit entropy round-trips correctly")
    func mnemonicFromEntropy24Words() {
        let entropy = Data(repeating: 0x00, count: 32)
        let mnemonic = Bip39.mnemonicFromEntropy(entropy)
        let words = mnemonic.split(separator: " ")
        #expect(words.count == 24)
        #expect(Bip39.isValidMnemonic(mnemonic))
        let recovered = Bip39.entropyFromMnemonic(mnemonic)
        #expect(recovered == entropy)
    }

    @Test("Key derivation from 24-word mnemonic is deterministic")
    func keyDerivation24Words() {
        let entropy = Data(repeating: 0x00, count: 32)
        let mnemonic = Bip39.mnemonicFromEntropy(entropy)
        let seed = Bip39.seedFromMnemonic(mnemonic)
        let nostrKey = Bip39.deriveNostrKey(from: seed)
        let blsKey = Bip39.deriveBlsKey(from: seed)

        #expect(nostrKey.count == 32)
        #expect(blsKey.count == 32)

        let seed2 = Bip39.seedFromMnemonic(mnemonic)
        #expect(Bip39.deriveNostrKey(from: seed2) == nostrKey)
        #expect(Bip39.deriveBlsKey(from: seed2) == blsKey)
    }
}

// MARK: - Passphrase Test

@Suite("BIP39: Passphrase")
struct Bip39PassphraseTests {

    @Test("Seed with TREZOR passphrase matches BIP39 reference vector")
    func seedWithPassphrase() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        let seed = Bip39.seedFromMnemonic(mnemonic, passphrase: "TREZOR")
        #expect(seed.count == 64)
        let seedHex = seed.map { String(format: "%02x", $0) }.joined()
        #expect(seedHex == "c55257c360c07c72029aebc1b53c05ed0362ada38ead3e3e7e24052f25f89aa21542af40e8a46a291678e2f1ef17e2af5a3be3712d464bfb0d35d4e012cb3b99")
        // Also verify it differs from the no-passphrase seed
        let seedNoPassphrase = Bip39.seedFromMnemonic(mnemonic)
        #expect(seed != seedNoPassphrase, "Passphrase must change the derived seed")
    }
}

// MARK: - Restore Over Existing Identity

@Suite("BIP39: Restore Over Existing Identity")
struct Bip39RestoreOverExistingTests {

    @Test("Restoring with a different mnemonic produces different keys")
    func restoreOverExisting() {
        let mnemonicA = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        let mnemonicB = Bip39.mnemonicFromEntropy(Data(repeating: 0x42, count: 16))

        let seedA = Bip39.seedFromMnemonic(mnemonicA)
        let nostrA = Bip39.deriveNostrKey(from: seedA)

        let seedB = Bip39.seedFromMnemonic(mnemonicB)
        let nostrB = Bip39.deriveNostrKey(from: seedB)
        let blsB = Bip39.deriveBlsKey(from: seedB)

        #expect(nostrA != nostrB, "Different mnemonics must produce different keys")

        // Simulate restore: re-derive from B's mnemonic
        let entropyB = Bip39.entropyFromMnemonic(mnemonicB)
        #expect(entropyB != nil)
        let restoredMnemonic = Bip39.mnemonicFromEntropy(entropyB!)
        #expect(restoredMnemonic == mnemonicB)
        let restoredSeed = Bip39.seedFromMnemonic(restoredMnemonic)
        let restoredNostr = Bip39.deriveNostrKey(from: restoredSeed)
        let restoredBls = Bip39.deriveBlsKey(from: restoredSeed)

        #expect(restoredNostr == nostrB, "Restored Nostr key must match original B")
        #expect(restoredBls == blsB, "Restored BLS key must match original B")
    }
}

// MARK: - Restore Round-Trip

@Suite("BIP39: Restore Round-Trip")
struct Bip39RestoreTests {

    @Test("Mnemonic → keys → entropy → mnemonic → keys produces identical keys")
    func restoreRoundTrip() {
        let mnemonic = Bip39.generateMnemonic()
        let seed1 = Bip39.seedFromMnemonic(mnemonic)
        let nostr1 = Bip39.deriveNostrKey(from: seed1)
        let bls1 = Bip39.deriveBlsKey(from: seed1)

        // Simulate restore: entropy round-trip
        let entropy = Bip39.entropyFromMnemonic(mnemonic)
        #expect(entropy != nil)
        let restored = Bip39.mnemonicFromEntropy(entropy!)
        #expect(restored == mnemonic)

        let seed2 = Bip39.seedFromMnemonic(restored)
        let nostr2 = Bip39.deriveNostrKey(from: seed2)
        let bls2 = Bip39.deriveBlsKey(from: seed2)

        #expect(nostr1 == nostr2, "Nostr keys must match after restore")
        #expect(bls1 == bls2, "BLS keys must match after restore")

        // Also verify downstream Stellar and X25519 keys match
        let stellar1 = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: nostr1),
            salt: Data("chat.onym.ios".utf8),
            info: Data("stellar-ed25519-v1".utf8),
            outputByteCount: 32
        ).withUnsafeBytes { Data($0) }
        let stellar2 = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: nostr2),
            salt: Data("chat.onym.ios".utf8),
            info: Data("stellar-ed25519-v1".utf8),
            outputByteCount: 32
        ).withUnsafeBytes { Data($0) }
        #expect(stellar1 == stellar2, "Stellar keys must match after restore")
    }
}

// MARK: - Legacy Install Path

@Suite("BIP39: Legacy Install Path")
struct Bip39LegacyTests {

    @Test("Invalid mnemonic returns nil entropy and fails validation")
    func legacyInstall_noBip39Entropy() {
        let invalidMnemonic = "not a valid mnemonic phrase"
        #expect(Bip39.entropyFromMnemonic(invalidMnemonic) == nil)
        #expect(!Bip39.isValidMnemonic(invalidMnemonic))
    }

    @Test("Random keys do not collide with BIP39-derived keys")
    func legacyInstall_randomKeysNotBip39Derivable() {
        let randomKey = Data((0..<32).map { UInt8(($0 * 7 + 13) & 0xFF) })
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        let seed = Bip39.seedFromMnemonic(mnemonic)
        let bip39Nostr = Bip39.deriveNostrKey(from: seed)
        #expect(randomKey != bip39Nostr, "Random key must not match BIP39-derived key")
    }
}
