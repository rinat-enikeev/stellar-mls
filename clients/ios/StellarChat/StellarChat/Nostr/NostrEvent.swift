import CryptoKit
import Foundation
import SwiftMLS

struct NostrEvent: Codable, Identifiable {
    let id: String
    let pubkey: String
    let createdAt: Int64
    let kind: Int
    let tags: [[String]]
    let content: String
    let sig: String

    enum CodingKeys: String, CodingKey {
        case id, pubkey, kind, tags, content, sig
        case createdAt = "created_at"
    }

    var jsonObject: [String: Any] {
        [
            "id": id,
            "pubkey": pubkey,
            "created_at": createdAt,
            "kind": kind,
            "tags": tags,
            "content": content,
            "sig": sig,
        ]
    }

    /// N-7: Verify the event ID integrity by recomputing from canonical JSON.
    /// This catches relay-level event tampering (modified content, pubkey, or tags).
    /// Full Schnorr signature verification requires secp256k1 which is not available
    /// in CryptoKit; the event ID check provides content integrity.
    func verifyEventID() -> Bool {
        let canonical: [Any] = [0, pubkey, createdAt, kind, tags, content]
        guard let serialized = try? JSONSerialization.data(withJSONObject: canonical, options: []) else {
            return false
        }
        let hash = SHA256.hash(data: serialized)
        let computedID = Data(hash).map { String(format: "%02x", $0) }.joined()
        return computedID == id
    }

    /// App-specific millisecond timestamp from `["ms", "..."]` tag.
    /// Falls back to `createdAt * 1000` if absent or invalid.
    /// This is an app-local convention — relays and third-party clients may ignore it.
    var displayMilliseconds: Int64 {
        if let msTag = tags.first(where: { $0.first == "ms" && $0.count >= 2 }),
           let ms = Int64(msTag[1]), ms >= 0 {
            return ms
        }
        return createdAt * 1000
    }

    /// Build a NIP-01 event, computing the event ID and signing it.
    /// Appends an app-specific `["ms", "<unix_ms>"]` tag for sub-second ordering.
    ///
    /// When `ephemeralSigner` is non-nil, the outer pubkey and Schnorr signature come
    /// from that per-event key instead of the device identity. Used for kinds 44114/34113
    /// to prevent relays from clustering co-membership via stable sender pubkeys —
    /// inner sender authentication is proven by BLS inside ciphertext.
    static func build(
        kind: Int,
        tags: [[String]],
        content: String,
        keyManager: KeyManager,
        ephemeralSigner: RustBackedNostrSigner? = nil
    ) throws -> NostrEvent {
        let pubkeyHex: String
        if let signer = ephemeralSigner {
            pubkeyHex = try signer.publicKey().map { String(format: "%02x", $0) }.joined()
        } else {
            pubkeyHex = keyManager.publicKeyHex
        }
        let unixMs = Int64(Date().timeIntervalSince1970 * 1000)
        let createdAt = unixMs / 1000

        // Append app-specific ms tag for sub-second ordering
        var allTags = tags
        allTags.append(["ms", String(unixMs)])

        // NIP-01 event ID: SHA256([0, pubkey, created_at, kind, tags, content])
        let canonical: [Any] = [0, pubkeyHex, createdAt, kind, allTags, content]
        let serialized = try JSONSerialization.data(withJSONObject: canonical, options: [])
        let hash = SHA256.hash(data: serialized)
        let eventID = Data(hash)
        let eventIDHex = eventID.map { String(format: "%02x", $0) }.joined()

        let signature: Data
        if let signer = ephemeralSigner {
            signature = try signer.signEventID(eventID)
        } else {
            signature = try keyManager.signEventID(eventID)
        }
        let sigHex = signature.map { String(format: "%02x", $0) }.joined()

        return NostrEvent(
            id: eventIDHex,
            pubkey: pubkeyHex,
            createdAt: createdAt,
            kind: kind,
            tags: allTags,
            content: content,
            sig: sigHex
        )
    }
}
