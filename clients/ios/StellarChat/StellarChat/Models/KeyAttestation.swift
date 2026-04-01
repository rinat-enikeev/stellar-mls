import CryptoKit
import Foundation

/// Binds a BLS12-381 group membership key to a secp256k1 Nostr identity key.
///
/// Per SEP-XXXX §1.1, the attestation proves that the holder of the Nostr signing key
/// also controls the BLS key used in the group Merkle tree. The signature covers
/// `SHA-256("SEP-XXXX:key-binding" || bls_pubkey)`.
///
/// The spec defines Ed25519 signatures for Stellar address binding; this implementation
/// uses secp256k1 Schnorr signatures for Nostr identity binding.
struct KeyAttestation: Codable, Equatable {
    let blsPubkey: Data      // 48 bytes, compressed G1 point
    let nostrPubkey: Data    // 32 bytes, secp256k1 x-only public key
    let signature: Data      // 64 bytes, Schnorr signature

    static func bindingMessage(blsPubkey: Data) -> Data {
        var hasher = SHA256()
        hasher.update(data: Data("SEP-XXXX:key-binding".utf8))
        hasher.update(data: blsPubkey)
        return Data(hasher.finalize())
    }
}
