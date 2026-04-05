package chat.onym.android.crypto

import java.security.MessageDigest

/**
 * BLS ↔ Stellar Ed25519 key binding (SEP-XXXX §1.1).
 * Proves that the same entity controls both the BLS group membership key
 * and the Stellar on-chain identity.
 */
data class KeyAttestation(
    val blsPubkey: ByteArray,     // 48 bytes, compressed G1 point
    val ed25519Pubkey: ByteArray, // 32 bytes, Stellar Ed25519 public key
    val signature: ByteArray      // 64 bytes, Ed25519 signature
) {
    companion object {
        /** Compute the binding message: SHA256("SEP-XXXX:key-binding" || blsPubkey). */
        fun bindingMessage(blsPubkey: ByteArray): ByteArray {
            val digest = MessageDigest.getInstance("SHA-256")
            digest.update("SEP-XXXX:key-binding".toByteArray())
            digest.update(blsPubkey)
            return digest.digest()
        }

        /** Create an attestation binding a BLS public key to a Stellar Ed25519 key. */
        fun create(keyManager: KeyManager): KeyAttestation {
            val blsPub = keyManager.blsPublicKey()
            val message = bindingMessage(blsPub)
            val signature = keyManager.stellarSign(message)
            return KeyAttestation(
                blsPubkey = blsPub,
                ed25519Pubkey = keyManager.stellarPublicKey,
                signature = signature
            )
        }

        /** Verify that the attestation signature is valid. */
        fun verify(attestation: KeyAttestation): Boolean {
            val message = bindingMessage(attestation.blsPubkey)
            return KeyManager.verifyEd25519(
                publicKey = attestation.ed25519Pubkey,
                message = message,
                signature = attestation.signature
            )
        }
    }

    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is KeyAttestation) return false
        return blsPubkey.contentEquals(other.blsPubkey) &&
                ed25519Pubkey.contentEquals(other.ed25519Pubkey) &&
                signature.contentEquals(other.signature)
    }

    override fun hashCode(): Int {
        var result = blsPubkey.contentHashCode()
        result = 31 * result + ed25519Pubkey.contentHashCode()
        result = 31 * result + signature.contentHashCode()
        return result
    }
}
