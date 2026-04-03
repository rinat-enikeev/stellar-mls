import CryptoKit
import Foundation

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
    static func build(
        kind: Int,
        tags: [[String]],
        content: String,
        keyManager: KeyManager
    ) throws -> NostrEvent {
        let pubkeyHex = keyManager.publicKeyHex
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

        let signature = try keyManager.signEventID(eventID)
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
