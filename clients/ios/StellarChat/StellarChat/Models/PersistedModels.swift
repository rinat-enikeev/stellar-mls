import Foundation
import SwiftData

/// Persisted group with field-level encryption on sensitive data.
/// Cleartext fields: id, epoch, tierRawValue, createdAt (needed for queries/display).
/// Encrypted fields: name, groupSecret, members, salt, commitment.
@Model
final class PersistedGroup {
    var id: String
    var encryptedName: Data
    var encryptedGroupSecret: Data
    var createdAt: Date
    var relayHintsJSON: Data       // JSON-encoded [String], not secret
    var encryptedMembers: Data     // encrypted JSON of member leaf data
    var epoch: Int
    var encryptedSalt: Data
    var encryptedCommitment: Data?
    var tierRawValue: Int
    var isPublishedOnChain: Bool

    init(
        id: String,
        encryptedName: Data,
        encryptedGroupSecret: Data,
        createdAt: Date,
        relayHintsJSON: Data,
        encryptedMembers: Data,
        epoch: Int,
        encryptedSalt: Data,
        encryptedCommitment: Data?,
        tierRawValue: Int,
        isPublishedOnChain: Bool = false
    ) {
        self.id = id
        self.encryptedName = encryptedName
        self.encryptedGroupSecret = encryptedGroupSecret
        self.createdAt = createdAt
        self.relayHintsJSON = relayHintsJSON
        self.encryptedMembers = encryptedMembers
        self.epoch = epoch
        self.encryptedSalt = encryptedSalt
        self.encryptedCommitment = encryptedCommitment
        self.tierRawValue = tierRawValue
        self.isPublishedOnChain = isPublishedOnChain
    }
}

/// Persisted contact alias — maps a Nostr pubkey to an encrypted human-readable name.
/// Pubkey stays cleartext (it's already visible on relays) for lookups.
@Model
final class PersistedContactAlias {
    @Attribute(.unique) var pubkey: String
    var encryptedName: Data
    var updatedAt: Date

    init(pubkey: String, encryptedName: Data, updatedAt: Date) {
        self.pubkey = pubkey
        self.encryptedName = encryptedName
        self.updatedAt = updatedAt
    }
}

/// Persisted message with encrypted text content.
/// Cleartext fields: id, groupID, senderPubkey, timestamp, isMine (all visible on relay anyway).
/// Encrypted fields: text (the actual private message content), mediaAttachment.
@Model
final class PersistedMessage {
    var id: String
    var groupID: String
    var senderPubkey: String
    var encryptedText: Data
    var timestamp: Date
    var isMine: Bool
    var encryptedMediaAttachment: Data?
    var isSystemMessage: Bool?

    init(
        id: String,
        groupID: String,
        senderPubkey: String,
        encryptedText: Data,
        timestamp: Date,
        isMine: Bool,
        encryptedMediaAttachment: Data? = nil,
        isSystemMessage: Bool = false
    ) {
        self.id = id
        self.groupID = groupID
        self.senderPubkey = senderPubkey
        self.encryptedText = encryptedText
        self.timestamp = timestamp
        self.isMine = isMine
        self.encryptedMediaAttachment = encryptedMediaAttachment
        self.isSystemMessage = isSystemMessage
    }
}
