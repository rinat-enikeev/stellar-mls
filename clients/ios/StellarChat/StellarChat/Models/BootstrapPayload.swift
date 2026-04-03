import Foundation
import SwiftMLS

/// NIP-XX bootstrap payload — the decrypted content of a SEP invitation event
/// (kind 34113 in the current transport, with legacy 24113 receive support).
/// Contains everything a recipient needs to join the group.
struct BootstrapPayload: Codable, Equatable {
    let groupID: Data           // 32 bytes
    let groupSecret: Data       // 32 bytes
    let name: String
    let epoch: UInt64
    let members: [SEPGroupMemberLeaf]
    let relayHints: [String]
    let salt: Data              // 32 bytes
    let tierRawValue: Int
    let commitment: Data?
    let senderNostrPubkey: String
    let senderAttestation: KeyAttestation?
    let senderTransportBundle: SEPMemberTransportBundle?

    /// Build a bootstrap payload from a ChatGroup, including the sender's key attestation and transport bundle.
    static func from(
        group: ChatGroup,
        senderPubkey: String,
        attestation: KeyAttestation? = nil,
        transportBundle: SEPMemberTransportBundle? = nil
    ) -> BootstrapPayload {
        BootstrapPayload(
            groupID: Data(hexString: group.id),
            groupSecret: group.groupSecret,
            name: group.name,
            epoch: group.epoch,
            members: group.members,
            relayHints: group.relayHints.map(\.absoluteString),
            salt: group.salt,
            tierRawValue: group.tier.rawValue,
            commitment: group.commitment,
            senderNostrPubkey: senderPubkey,
            senderAttestation: attestation,
            senderTransportBundle: transportBundle
        )
    }

    /// Convert to a ChatGroup for local storage.
    /// Members are re-sorted by compressed G1 key per SEP-XXXX §2.1
    /// to guard against out-of-order payloads.
    func toChatGroup() -> ChatGroup {
        let groupIDHex = groupID.map { String(format: "%02x", $0) }.joined()
        let relayURLs = relayHints.compactMap(URL.init(string:))
        let sortedMembers = members.sorted {
            $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed)
        }
        return ChatGroup(
            id: groupIDHex,
            name: name,
            groupSecret: groupSecret,
            createdAt: Date(),
            relayHints: relayURLs,
            members: sortedMembers,
            epoch: epoch,
            salt: salt,
            commitment: commitment,
            tier: SEPTier(rawValue: tierRawValue) ?? .large
        )
    }
}

/// Represents a received invitation before the user accepts it.
struct PendingInvitation: Identifiable, Equatable {
    let id: String                  // Nostr event ID
    let payload: BootstrapPayload
    let receivedAt: Date
}

private extension Data {
    init(hexString: String) {
        let bytes = stride(from: 0, to: hexString.count, by: 2).compactMap { i -> UInt8? in
            let start = hexString.index(hexString.startIndex, offsetBy: i)
            let end = hexString.index(start, offsetBy: 2)
            return UInt8(hexString[start..<end], radix: 16)
        }
        self.init(bytes)
    }
}
