package chat.onym.android.crypto

/**
 * Stellar StrKey encoding (SEP-0023).
 * Encodes Ed25519 public keys as "G..." account IDs.
 */
object StellarStrKey {
    private const val VERSION_ACCOUNT_ID: Byte = (6 shl 3).toByte() // 48

    private val BASE32_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567".toCharArray()

    /** Encode a 32-byte Ed25519 public key as a Stellar account ID (G...). */
    fun encodeAccountID(publicKey: ByteArray): String {
        require(publicKey.size == 32) { "Ed25519 public key must be 32 bytes" }

        val payload = ByteArray(35) // 1 version + 32 key + 2 checksum
        payload[0] = VERSION_ACCOUNT_ID
        System.arraycopy(publicKey, 0, payload, 1, 32)

        val checksum = crc16XModem(payload, 0, 33)
        payload[33] = (checksum and 0xFF).toByte()
        payload[34] = ((checksum shr 8) and 0xFF).toByte()

        return base32Encode(payload)
    }

    /** CRC-16-XModem (polynomial 0x1021, initial value 0x0000). */
    private fun crc16XModem(data: ByteArray, offset: Int, length: Int): Int {
        var crc = 0x0000
        for (i in offset until offset + length) {
            crc = crc xor ((data[i].toInt() and 0xFF) shl 8)
            for (j in 0 until 8) {
                crc = if (crc and 0x8000 != 0) {
                    (crc shl 1) xor 0x1021
                } else {
                    crc shl 1
                }
                crc = crc and 0xFFFF
            }
        }
        return crc
    }

    /** RFC 4648 Base32 encoding (no padding). */
    private fun base32Encode(data: ByteArray): String {
        val sb = StringBuilder()
        var buffer = 0
        var bitsLeft = 0

        for (byte in data) {
            buffer = (buffer shl 8) or (byte.toInt() and 0xFF)
            bitsLeft += 8
            while (bitsLeft >= 5) {
                bitsLeft -= 5
                sb.append(BASE32_ALPHABET[(buffer shr bitsLeft) and 0x1F])
            }
        }
        if (bitsLeft > 0) {
            sb.append(BASE32_ALPHABET[(buffer shl (5 - bitsLeft)) and 0x1F])
        }
        return sb.toString()
    }
}
