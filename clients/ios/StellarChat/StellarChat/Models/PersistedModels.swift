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
    var pinnedEpoch: Int?
    var forkedFromGroupID: String?
    var forkedAtEpoch: Int?
    var removedByPubkeyHex: String?
    var pushNotificationsEnabled: Bool
    var lastMessageAt: Date?
    var isPinned: Bool = false
    /// Governance type id. Optional so existing SwiftData stores migrate in
    /// as `nil`, which we treat as `.anarchy` when projecting back to
    /// `ChatGroup` (see `PersistenceStore.decryptGroup`).
    var groupTypeRawValue: Int?
    /// Encrypted JSON-encoded `[String]` of admin BLS pubkey hex. Optional so
    /// existing SwiftData stores migrate in as `nil` (empty set).
    var encryptedAdminPubkeys: Data?
    /// Encrypted JPEG bytes of a locally-set group avatar. Optional so existing
    /// SwiftData stores migrate in as `nil`.
    var encryptedAvatar: Data?

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
        isPublishedOnChain: Bool = false,
        pinnedEpoch: Int? = nil,
        forkedFromGroupID: String? = nil,
        forkedAtEpoch: Int? = nil,
        removedByPubkeyHex: String? = nil,
        pushNotificationsEnabled: Bool = false,
        lastMessageAt: Date? = nil,
        isPinned: Bool = false,
        groupTypeRawValue: Int? = nil,
        encryptedAdminPubkeys: Data? = nil,
        encryptedAvatar: Data? = nil
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
        self.pinnedEpoch = pinnedEpoch
        self.forkedFromGroupID = forkedFromGroupID
        self.forkedAtEpoch = forkedAtEpoch
        self.removedByPubkeyHex = removedByPubkeyHex
        self.pushNotificationsEnabled = pushNotificationsEnabled
        self.lastMessageAt = lastMessageAt
        self.isPinned = isPinned
        self.groupTypeRawValue = groupTypeRawValue
        self.encryptedAdminPubkeys = encryptedAdminPubkeys
        self.encryptedAvatar = encryptedAvatar
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

/// Persisted invited contact — tracks people the user has sent a Telegram invite to.
/// Phone/name/inviteURL are encrypted (the phonebook should never leak to disk in cleartext).
/// The `handshakeNonce` stays cleartext so reconciliation can query by it.
@Model
final class PersistedInvitedContact {
    @Attribute(.unique) var phone: String       // E.164, cleartext key for upsert
    var encryptedDisplayName: Data
    var inviteKind: String                      // "group" | "onboard"
    var encryptedInviteURL: Data
    var handshakeNonce: String?
    var groupID: String?
    var invitedAt: Date
    var status: String                          // "pending" | "sent" | "joined"
    var resolvedPubkey: String?

    init(
        phone: String,
        encryptedDisplayName: Data,
        inviteKind: String,
        encryptedInviteURL: Data,
        handshakeNonce: String? = nil,
        groupID: String? = nil,
        invitedAt: Date = Date(),
        status: String = "pending",
        resolvedPubkey: String? = nil
    ) {
        self.phone = phone
        self.encryptedDisplayName = encryptedDisplayName
        self.inviteKind = inviteKind
        self.encryptedInviteURL = encryptedInviteURL
        self.handshakeNonce = handshakeNonce
        self.groupID = groupID
        self.invitedAt = invitedAt
        self.status = status
        self.resolvedPubkey = resolvedPubkey
    }
}

/// Persisted transport bundle — maps a group member's BLS pubkey to their authenticated
/// X25519 inbox encryption key. Encrypted at rest like other sensitive fields.
@Model
final class PersistedTransportBundle {
    var groupID: String
    var blsPubkeyHex: String
    var encryptedBundle: Data   // encrypted JSON of SEPMemberTransportBundle

    init(groupID: String, blsPubkeyHex: String, encryptedBundle: Data) {
        self.groupID = groupID
        self.blsPubkeyHex = blsPubkeyHex
        self.encryptedBundle = encryptedBundle
    }
}

/// Persisted pending rekey — tracks outgoing rekey envelopes awaiting acknowledgment.
@Model
final class PersistedPendingRekey {
    var groupID: String
    var epoch: Int
    var encryptedEnvelope: Data          // encrypted SEPRekeyEnvelope JSON
    var unackedMemberKeysJSON: Data      // JSON array of hex BLS pubkeys
    var retryCount: Int
    var createdAt: Date
    var isRemovalEpoch: Bool

    init(
        groupID: String,
        epoch: Int,
        encryptedEnvelope: Data,
        unackedMemberKeysJSON: Data,
        retryCount: Int = 0,
        createdAt: Date = Date(),
        isRemovalEpoch: Bool = false
    ) {
        self.groupID = groupID
        self.epoch = epoch
        self.encryptedEnvelope = encryptedEnvelope
        self.unackedMemberKeysJSON = unackedMemberKeysJSON
        self.retryCount = retryCount
        self.createdAt = createdAt
        self.isRemovalEpoch = isRemovalEpoch
    }
}

/// Persisted epoch snapshot — captures group state at a specific epoch for epoch branching.
/// Encrypted fields: members, salt, groupSecret (all sensitive).
@Model
final class PersistedEpochSnapshot {
    var groupID: String
    var epoch: Int
    var encryptedMembers: Data
    var encryptedSalt: Data
    var encryptedGroupSecret: Data
    var changeDescription: String

    init(
        groupID: String,
        epoch: Int,
        encryptedMembers: Data,
        encryptedSalt: Data,
        encryptedGroupSecret: Data,
        changeDescription: String
    ) {
        self.groupID = groupID
        self.epoch = epoch
        self.encryptedMembers = encryptedMembers
        self.encryptedSalt = encryptedSalt
        self.encryptedGroupSecret = encryptedGroupSecret
        self.changeDescription = changeDescription
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
    var epoch: Int?
    var replyToID: String?
    /// Raw value of `MessageStatus` (optional for SwiftData lightweight migration from older stores).
    var status: String?

    init(
        id: String,
        groupID: String,
        senderPubkey: String,
        encryptedText: Data,
        timestamp: Date,
        isMine: Bool,
        encryptedMediaAttachment: Data? = nil,
        isSystemMessage: Bool = false,
        epoch: Int? = nil,
        replyToID: String? = nil,
        status: String? = nil
    ) {
        self.id = id
        self.groupID = groupID
        self.senderPubkey = senderPubkey
        self.encryptedText = encryptedText
        self.timestamp = timestamp
        self.isMine = isMine
        self.encryptedMediaAttachment = encryptedMediaAttachment
        self.isSystemMessage = isSystemMessage
        self.epoch = epoch
        self.replyToID = replyToID
        self.status = status
    }
}
