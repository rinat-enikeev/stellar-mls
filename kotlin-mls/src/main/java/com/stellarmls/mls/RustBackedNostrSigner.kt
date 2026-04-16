package com.stellarmls.mls

/**
 * Nostr event signer using real secp256k1 Schnorr signatures via Rust FFI.
 * Drop-in replacement for the demo HMAC-based signer.
 */
class RustBackedNostrSigner(private val secretKey: ByteArray) {
    init {
        require(secretKey.size == 32) { "secret key must be 32 bytes" }
    }

    /** Derive the secp256k1 x-only public key (32 bytes). */
    fun publicKey(): ByteArray = RustBridge.nostrDerivePublicKey(secretKey)

    /** Sign a 32-byte event ID. Returns 64-byte Schnorr signature. */
    fun signEventId(eventId: ByteArray): ByteArray {
        require(eventId.size == 32) { "event ID must be 32 bytes" }
        return RustBridge.nostrSignEventId(secretKey, eventId)
    }

    companion object {
        /** Fresh per-event signer backed by a random 32-byte secp256k1 scalar from the JCE CSPRNG. */
        fun ephemeral(): RustBackedNostrSigner {
            val secret = ByteArray(32)
            java.security.SecureRandom().nextBytes(secret)
            return RustBackedNostrSigner(secret)
        }
    }
}
