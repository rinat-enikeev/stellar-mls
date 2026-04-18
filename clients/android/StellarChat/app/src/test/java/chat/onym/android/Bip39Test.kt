package chat.onym.android

import chat.onym.android.crypto.Bip39
import chat.onym.android.crypto.GroupCrypto
import org.junit.Assert.*
import org.junit.Test

/**
 * Cross-platform BIP39 test vectors.
 * These must produce identical results to the iOS Bip39Tests.
 */
class Bip39Test {

    private fun ByteArray.toHex(): String =
        joinToString("") { "%02x".format(it) }

    private fun hexToBytes(hex: String): ByteArray =
        ByteArray(hex.length / 2) { i ->
            hex.substring(i * 2, i * 2 + 2).toInt(16).toByte()
        }

    // -----------------------------------------------------------------------
    // Mnemonic Encoding and Validation
    // -----------------------------------------------------------------------

    @Test
    fun mnemonicFromEntropy_zeroEntropy() {
        val entropy = ByteArray(16) // all zeros
        val mnemonic = Bip39.mnemonicFromEntropy(entropy)
        assertEquals(
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
            mnemonic
        )
    }

    @Test
    fun entropyRoundTrip() {
        val originalEntropy = ByteArray(16) // all zeros
        val mnemonic = Bip39.mnemonicFromEntropy(originalEntropy)
        val recovered = Bip39.entropyFromMnemonic(mnemonic)
        assertNotNull(recovered)
        assertArrayEquals(originalEntropy, recovered)
    }

    @Test
    fun validMnemonic_passes() {
        val mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        assertTrue(Bip39.isValidMnemonic(mnemonic))
    }

    @Test
    fun invalidChecksum_fails() {
        val mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon"
        assertFalse(Bip39.isValidMnemonic(mnemonic))
    }

    @Test
    fun unknownWord_fails() {
        val mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon zzzzz"
        assertFalse(Bip39.isValidMnemonic(mnemonic))
    }

    @Test
    fun wrongWordCount_fails() {
        val mnemonic = "abandon abandon abandon"
        assertFalse(Bip39.isValidMnemonic(mnemonic))
    }

    @Test
    fun generatedMnemonic_isValid() {
        repeat(10) {
            val mnemonic = Bip39.generateMnemonic()
            assertTrue("Generated mnemonic should be valid", Bip39.isValidMnemonic(mnemonic))
            assertEquals("Mnemonic should have 12 words", 12, mnemonic.split(" ").size)
        }
    }

    // -----------------------------------------------------------------------
    // Seed Derivation (PBKDF2-HMAC-SHA512)
    // -----------------------------------------------------------------------

    @Test
    fun seedFromKnownMnemonic_matchesBip39TestVector() {
        val mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        val seed = Bip39.seedFromMnemonic(mnemonic)
        assertEquals(64, seed.size)
        val seedHex = seed.toHex()
        // Standard BIP39 test vector for this mnemonic
        assertEquals(
            "5eb00bbddcf069084889a8ab9155568165f5c453ccb85e70811aaed6f6da5fc19a5ac40b389cd370d086206dec8aa6c43daea6690f20ad3d8d48b2d2ce9e38e4",
            seedHex
        )
    }

    // -----------------------------------------------------------------------
    // Cross-Platform Key Derivation
    // -----------------------------------------------------------------------

    @Test
    fun keyDerivation_zeroEntropyMnemonic() {
        val mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        val seed = Bip39.seedFromMnemonic(mnemonic)
        val nostrKey = Bip39.deriveNostrKey(seed)
        val blsKey = Bip39.deriveBlsKey(seed)

        assertEquals(32, nostrKey.size)
        assertEquals(32, blsKey.size)

        val nostrHex = nostrKey.toHex()
        val blsHex = blsKey.toHex()

        // These MUST match the iOS test vectors exactly
        assertEquals("9a41cb80566383dc6cbab39414c139dc940cbd640c9ca4cb355fa6c8a34fb868", nostrHex)
        assertEquals("7ee35a41f2cfa0ebf9989328d5dcd8d10da98c0b8955dac71e309750e6fdc623", blsHex)
    }

    @Test
    fun keyDerivation_hex42Entropy() {
        val entropy = ByteArray(16) { 0x42 }
        val mnemonic = Bip39.mnemonicFromEntropy(entropy)
        val seed = Bip39.seedFromMnemonic(mnemonic)
        val nostrKey = Bip39.deriveNostrKey(seed)
        val blsKey = Bip39.deriveBlsKey(seed)

        assertEquals(32, nostrKey.size)
        assertEquals(32, blsKey.size)

        val nostrHex = nostrKey.toHex()
        val blsHex = blsKey.toHex()

        // These MUST match the iOS test vectors exactly
        assertEquals("7cbc4d71b42367f74dd2356e3ba1ec8f1f26e7ed5a8cf150c5820efb2fc701d2", nostrHex)
        assertEquals("0b402ebb55641736888bb315fb7c6dc49ae0a50ce066a05227f6e55f71c6a097", blsHex)
    }

    @Test
    fun stellarAndX25519_fromBip39DerivedNostrKey() {
        val mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        val seed = Bip39.seedFromMnemonic(mnemonic)
        val nostrKey = Bip39.deriveNostrKey(seed)

        // Derive Stellar Ed25519 using the same HKDF path as KeyManager
        val stellarKey = GroupCrypto.hkdf(
            ikm = nostrKey,
            salt = "chat.onym.ios".toByteArray(),
            info = "stellar-ed25519-v1".toByteArray(),
            length = 32
        )

        // Derive X25519 using the same HKDF path as KeyManager
        val x25519Key = GroupCrypto.hkdf(
            ikm = nostrKey,
            salt = "chat.onym.ios".toByteArray(),
            info = "x25519-key-agreement-v1".toByteArray(),
            length = 32
        )

        assertEquals(32, stellarKey.size)
        assertEquals(32, x25519Key.size)
        assertFalse("Stellar and X25519 keys must differ", stellarKey.contentEquals(x25519Key))
        assertFalse("Stellar and Nostr keys must differ", stellarKey.contentEquals(nostrKey))
    }
}
