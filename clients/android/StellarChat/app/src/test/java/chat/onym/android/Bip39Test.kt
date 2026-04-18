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
    fun stellarAndX25519_fromBip39DerivedNostrKey_zeroEntropy() {
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

        // Pinned cross-platform values — iOS tests MUST produce the same hex
        assertEquals("45d014863f395145d396e193adcd6d761c919d8db64d5356486e9fff4a32c4ca", stellarKey.toHex())
        assertEquals("b86719ab1a4317618b81cf7225be405c92dc315886d0c817da71cc12fb08cc17", x25519Key.toHex())
    }

    @Test
    fun stellarAndX25519_fromBip39DerivedNostrKey_hex42Entropy() {
        val entropy = ByteArray(16) { 0x42 }
        val mnemonic = Bip39.mnemonicFromEntropy(entropy)
        val seed = Bip39.seedFromMnemonic(mnemonic)
        val nostrKey = Bip39.deriveNostrKey(seed)

        val stellarKey = GroupCrypto.hkdf(
            ikm = nostrKey,
            salt = "chat.onym.ios".toByteArray(),
            info = "stellar-ed25519-v1".toByteArray(),
            length = 32
        )

        val x25519Key = GroupCrypto.hkdf(
            ikm = nostrKey,
            salt = "chat.onym.ios".toByteArray(),
            info = "x25519-key-agreement-v1".toByteArray(),
            length = 32
        )

        assertEquals(32, stellarKey.size)
        assertEquals(32, x25519Key.size)

        // Pinned cross-platform values — iOS tests MUST produce the same hex
        assertEquals("6ec053eb7fb174f774fcd80bd6f2b7dc839e3e4e20fa99d2c01a1f85c475a2bb", stellarKey.toHex())
        assertEquals("81497994b0a24bb8b4295c0621541427253d5a02a10e00833cb03913bfd15c56", x25519Key.toHex())
    }

    // -----------------------------------------------------------------------
    // Restore Round-Trip
    // -----------------------------------------------------------------------

    @Test
    fun restoreRoundTrip_keysMatchOriginal() {
        // Generate a mnemonic, derive keys, then re-derive from the same mnemonic
        // and verify we get identical keys — this is the critical restore path.
        val mnemonic = Bip39.generateMnemonic()
        val seed1 = Bip39.seedFromMnemonic(mnemonic)
        val nostr1 = Bip39.deriveNostrKey(seed1)
        val bls1 = Bip39.deriveBlsKey(seed1)

        // Simulate restore: re-derive from the same mnemonic
        val entropy = Bip39.entropyFromMnemonic(mnemonic)
        assertNotNull(entropy)
        val restored = Bip39.mnemonicFromEntropy(entropy!!)
        assertEquals(mnemonic, restored)

        val seed2 = Bip39.seedFromMnemonic(restored)
        val nostr2 = Bip39.deriveNostrKey(seed2)
        val bls2 = Bip39.deriveBlsKey(seed2)

        assertArrayEquals("Nostr keys must match after restore", nostr1, nostr2)
        assertArrayEquals("BLS keys must match after restore", bls1, bls2)

        // Also verify Stellar and X25519 derived keys match
        val stellar1 = GroupCrypto.hkdf(nostr1, "chat.onym.ios".toByteArray(), "stellar-ed25519-v1".toByteArray(), 32)
        val stellar2 = GroupCrypto.hkdf(nostr2, "chat.onym.ios".toByteArray(), "stellar-ed25519-v1".toByteArray(), 32)
        assertArrayEquals("Stellar keys must match after restore", stellar1, stellar2)

        val x1 = GroupCrypto.hkdf(nostr1, "chat.onym.ios".toByteArray(), "x25519-key-agreement-v1".toByteArray(), 32)
        val x2 = GroupCrypto.hkdf(nostr2, "chat.onym.ios".toByteArray(), "x25519-key-agreement-v1".toByteArray(), 32)
        assertArrayEquals("X25519 keys must match after restore", x1, x2)
    }

    // -----------------------------------------------------------------------
    // Legacy Install Path
    // -----------------------------------------------------------------------

    @Test
    fun legacyInstall_noBip39Entropy_returnsNullPhrase() {
        // A legacy install has keys but no BIP39 entropy.
        // Verify that entropyFromMnemonic returns null for invalid input,
        // simulating what happens when there's no stored entropy.
        val invalidMnemonic = "not a valid mnemonic phrase"
        assertNull(Bip39.entropyFromMnemonic(invalidMnemonic))
        assertFalse(Bip39.isValidMnemonic(invalidMnemonic))
    }

    @Test
    fun legacyInstall_randomKeysNotBip39Derivable() {
        // Legacy installs have random keys that won't match any BIP39 derivation.
        // Verify that arbitrary 32-byte keys don't accidentally collide with BIP39 paths.
        val randomKey = ByteArray(32) { (it * 7 + 13).toByte() }
        val mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        val seed = Bip39.seedFromMnemonic(mnemonic)
        val bip39Nostr = Bip39.deriveNostrKey(seed)

        assertFalse("Random key must not match BIP39-derived key", randomKey.contentEquals(bip39Nostr))
    }
}
