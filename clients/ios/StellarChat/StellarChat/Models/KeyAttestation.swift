import CryptoKit
import Foundation

/// SEP-XXXX §1.1 Key Attestation — binds a BLS12-381 group membership key
/// to a Stellar Ed25519 account key.
///
/// ```
/// KeyAttestation {
///     bls_pubkey:     BytesN<48>   -- compressed G1 point
///     ed25519_pubkey: BytesN<32>   -- Stellar account public key
///     signature:      BytesN<64>   -- Ed25519 signature over SHA-256("SEP-XXXX:key-binding" || bls_pubkey)
/// }
/// ```
///
/// This attestation is a group-level artifact shared among members via the
/// encrypted channel. It is never submitted on-chain.
struct KeyAttestation: Codable, Equatable {
    let blsPubkey: Data       // 48 bytes, compressed G1 point
    let ed25519Pubkey: Data   // 32 bytes, Stellar Ed25519 public key
    let signature: Data       // 64 bytes, Ed25519 signature

    /// The binding message: SHA-256("SEP-XXXX:key-binding" || bls_pubkey).
    static func bindingMessage(blsPubkey: Data) -> Data {
        var hasher = SHA256()
        hasher.update(data: Data("SEP-XXXX:key-binding".utf8))
        hasher.update(data: blsPubkey)
        return Data(hasher.finalize())
    }
}
