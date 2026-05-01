package chat.onym.android

import chat.onym.android.crypto.GroupCrypto
import org.junit.Assert.*
import org.junit.Test

/**
 * Pure JVM unit tests for GroupCrypto functions that don't depend on
 * Android APIs (MessageDigest, HmacSHA256 are in standard Java).
 *
 * These verify cross-platform determinism against the canonical test
 * vectors from docs/cross-platform-test-vectors.json.
 */
class GroupCryptoTest {

    private fun hexToBytes(hex: String): ByteArray {
        return ByteArray(hex.length / 2) { i ->
            hex.substring(i * 2, i * 2 + 2).toInt(16).toByte()
        }
    }

    private fun ByteArray.toHex(): String =
        joinToString("") { "%02x".format(it) }

    // -----------------------------------------------------------------------
    // Topic Derivation: SHA-256("sep-topic-v1" || groupSecret)[0..8] as hex
    // -----------------------------------------------------------------------

    @Test
    fun hiddenGroupTopic_zeroSecret() {
        val secret = ByteArray(32)
        val topic = GroupCrypto.hiddenGroupTopic(secret)
        assertEquals("0c6ce887ec3881f3", topic)
    }

    @Test
    fun hiddenGroupTopic_vector42() {
        val secret = hexToBytes("4242424242424242424242424242424242424242424242424242424242424242")
        val topic = GroupCrypto.hiddenGroupTopic(secret)
        assertEquals("aa263d222a1f2e1f", topic)
    }

    @Test
    fun hiddenGroupTopic_vectorFF00() {
        val secret = hexToBytes("ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00ff00")
        val topic = GroupCrypto.hiddenGroupTopic(secret)
        assertEquals("7df84225883879ff", topic)
    }

    @Test
    fun hiddenGroupTopic_deterministic() {
        val secret = ByteArray(32) { 0x42 }
        val t1 = GroupCrypto.hiddenGroupTopic(secret)
        val t2 = GroupCrypto.hiddenGroupTopic(secret)
        assertEquals(t1, t2)
    }

    @Test
    fun hiddenGroupTopic_is16CharHex() {
        val topic = GroupCrypto.hiddenGroupTopic(ByteArray(32))
        assertEquals(16, topic.length)
        assertTrue(topic.all { it in '0'..'9' || it in 'a'..'f' })
    }

    // -----------------------------------------------------------------------
    // Inbox Derivation: SHA-256("sep-inbox-v1" || pubkey)[0..8] as hex
    // -----------------------------------------------------------------------

    @Test
    fun hiddenInboxTag_zeroPubkey() {
        val pubkey = ByteArray(32)
        val tag = GroupCrypto.hiddenInboxTag(pubkey)
        assertEquals("f434d35e24e86124", tag)
    }

    @Test
    fun hiddenInboxTag_vector42() {
        val pubkey = hexToBytes("4242424242424242424242424242424242424242424242424242424242424242")
        val tag = GroupCrypto.hiddenInboxTag(pubkey)
        assertEquals("c9bcafedc73f4289", tag)
    }

    @Test
    fun hiddenInboxTag_deterministic() {
        val pubkey = ByteArray(32) { 0x01 }
        val t1 = GroupCrypto.hiddenInboxTag(pubkey)
        val t2 = GroupCrypto.hiddenInboxTag(pubkey)
        assertEquals(t1, t2)
    }

    // -----------------------------------------------------------------------
    // HKDF Message Key Derivation
    // -----------------------------------------------------------------------

    @Test
    fun deriveMessageKey_vector1() {
        val secret = ByteArray(32)
        val salt = ByteArray(32)
        val key = GroupCrypto.deriveMessageKey(secret, 0, salt)
        assertEquals(32, key.size)
        assertEquals(
            "e7b6e0341ab181313e1325602763c196b8113f1f8accf570674e797863bd6acd",
            key.toHex()
        )
    }

    @Test
    fun deriveMessageKey_vector2() {
        val secret = hexToBytes("4242424242424242424242424242424242424242424242424242424242424242")
        val salt = hexToBytes("0707070707070707070707070707070707070707070707070707070707070707")
        val key = GroupCrypto.deriveMessageKey(secret, 5, salt)
        assertEquals(
            "68346c282ce344b6ff7f6c7138737873438a2d7fd28c89573899d73ee6a9d52e",
            key.toHex()
        )
    }

    @Test
    fun deriveMessageKey_is32Bytes() {
        val key = GroupCrypto.deriveMessageKey(ByteArray(32), 0, ByteArray(32))
        assertEquals(32, key.size)
    }

    @Test
    fun deriveMessageKey_differentEpochsDifferentKeys() {
        val secret = ByteArray(32) { 0x42 }
        val salt = ByteArray(32) { 0xAA.toByte() }
        val k0 = GroupCrypto.deriveMessageKey(secret, 0, salt)
        val k1 = GroupCrypto.deriveMessageKey(secret, 1, salt)
        assertFalse(k0.contentEquals(k1))
    }

    @Test
    fun deriveMessageKey_differentSaltsDifferentKeys() {
        val secret = ByteArray(32) { 0x42 }
        val k1 = GroupCrypto.deriveMessageKey(secret, 0, ByteArray(32) { 0x01 })
        val k2 = GroupCrypto.deriveMessageKey(secret, 0, ByteArray(32) { 0x02 })
        assertFalse(k1.contentEquals(k2))
    }

    // -----------------------------------------------------------------------
    // HKDF Primitive
    // -----------------------------------------------------------------------

    @Test
    fun hkdf_deterministic() {
        val ikm = ByteArray(32) { 0x42 }
        val salt = "test-salt".toByteArray()
        val info = "test-info".toByteArray()
        val k1 = GroupCrypto.hkdf(ikm, salt, info, 32)
        val k2 = GroupCrypto.hkdf(ikm, salt, info, 32)
        assertArrayEquals(k1, k2)
    }

    @Test
    fun hkdf_differentInfoDifferentOutput() {
        val ikm = ByteArray(32) { 0x42 }
        val salt = "salt".toByteArray()
        val k1 = GroupCrypto.hkdf(ikm, salt, "info1".toByteArray(), 32)
        val k2 = GroupCrypto.hkdf(ikm, salt, "info2".toByteArray(), 32)
        assertFalse(k1.contentEquals(k2))
    }

    @Test
    fun hkdf_differentSaltsDifferentOutput() {
        val ikm = ByteArray(32) { 0x42 }
        val info = "info".toByteArray()
        val k1 = GroupCrypto.hkdf(ikm, "salt1".toByteArray(), info, 32)
        val k2 = GroupCrypto.hkdf(ikm, "salt2".toByteArray(), info, 32)
        assertFalse(k1.contentEquals(k2))
    }

    @Test
    fun hkdf_respectsRequestedLength_16Bytes() {
        val key = GroupCrypto.hkdf(
            ikm = ByteArray(32),
            salt = "salt".toByteArray(),
            info = "info".toByteArray(),
            length = 16
        )
        assertEquals(16, key.size)
    }

    @Test
    fun hkdf_respectsRequestedLength_32Bytes() {
        val key = GroupCrypto.hkdf(
            ikm = ByteArray(32),
            salt = "salt".toByteArray(),
            info = "info".toByteArray(),
            length = 32
        )
        assertEquals(32, key.size)
    }

    @Test
    fun hkdf_differentIkmDifferentOutput() {
        val salt = "salt".toByteArray()
        val info = "info".toByteArray()
        val k1 = GroupCrypto.hkdf(ByteArray(32) { 0x01 }, salt, info, 32)
        val k2 = GroupCrypto.hkdf(ByteArray(32) { 0x02 }, salt, info, 32)
        assertFalse(k1.contentEquals(k2))
    }
}
