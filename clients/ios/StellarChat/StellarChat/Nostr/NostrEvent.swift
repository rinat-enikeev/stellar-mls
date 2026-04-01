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

    /// Build a NIP-01 event, computing the event ID and signing it.
    static func build(
        kind: Int,
        tags: [[String]],
        content: String,
        keyManager: KeyManager
    ) throws -> NostrEvent {
        let pubkeyHex = keyManager.publicKeyHex
        let createdAt = Int64(Date().timeIntervalSince1970)

        // NIP-01 event ID: SHA256([0, pubkey, created_at, kind, tags, content])
        let canonical: [Any] = [0, pubkeyHex, createdAt, kind, tags, content]
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
            tags: tags,
            content: content,
            sig: sigHex
        )
    }
}
