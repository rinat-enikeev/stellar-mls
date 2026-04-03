import CryptoKit
import Foundation

// MARK: - Group State Update Protocol Messages

/// Distributed via the encrypted group channel (kind 24114) after a membership change.
/// Contains the new epoch, salt, and member delta so all members can update their local state.
public struct SEPGroupStateUpdate: Codable, Equatable, Sendable {
    public static let messageType = "sep_state_update"

    public let type: String
    public let epoch: UInt64
    public let salt: Data
    public let addedMembers: [SEPGroupMemberLeaf]
    public let removedMemberKeys: [Data]   // compressed G1 pubkeys of removed members
    public let commitment: Data?           // new Poseidon commitment (optional)
    public let senderAttestation: SEPKeyAttestationPayload?
    public let senderTransportBundle: SEPMemberTransportBundle?

    public init(
        epoch: UInt64,
        salt: Data,
        addedMembers: [SEPGroupMemberLeaf] = [],
        removedMemberKeys: [Data] = [],
        commitment: Data? = nil,
        senderAttestation: SEPKeyAttestationPayload? = nil,
        senderTransportBundle: SEPMemberTransportBundle? = nil
    ) {
        self.type = Self.messageType
        self.epoch = epoch
        self.salt = salt
        self.addedMembers = addedMembers
        self.removedMemberKeys = removedMemberKeys
        self.commitment = commitment
        self.senderAttestation = senderAttestation
        self.senderTransportBundle = senderTransportBundle
    }
}

/// Request a salt for a specific epoch from other online group members.
/// Sent via the encrypted group channel when a member discovers it missed an epoch.
public struct SEPSaltRequest: Codable, Equatable, Sendable {
    public static let messageType = "sep_salt_request"

    public let type: String
    public let epoch: UInt64

    public init(epoch: UInt64) {
        self.type = Self.messageType
        self.epoch = epoch
    }
}

/// Response to a salt request, providing the salt for the requested epoch.
public struct SEPSaltResponse: Codable, Equatable, Sendable {
    public static let messageType = "sep_salt_response"

    public let type: String
    public let epoch: UInt64
    public let salt: Data

    public init(epoch: UInt64, salt: Data) {
        self.type = Self.messageType
        self.epoch = epoch
        self.salt = salt
    }
}

/// A key attestation payload suitable for wire transmission (Codable).
/// Binds a BLS12-381 group membership key to a Stellar Ed25519 account key.
public struct SEPKeyAttestationPayload: Codable, Equatable, Sendable {
    public let blsPubkey: Data       // 48 bytes, compressed G1 point
    public let ed25519Pubkey: Data   // 32 bytes, Stellar Ed25519 public key
    public let signature: Data       // 64 bytes, Ed25519 signature

    public init(blsPubkey: Data, ed25519Pubkey: Data, signature: Data) {
        self.blsPubkey = blsPubkey
        self.ed25519Pubkey = ed25519Pubkey
        self.signature = signature
    }

    /// Validate the structural integrity of the attestation fields.
    /// Returns false if any field has an incorrect byte length.
    public var hasValidStructure: Bool {
        blsPubkey.count == 48 && ed25519Pubkey.count == 32 && signature.count == 64
    }

    /// Compute the binding message that the Ed25519 signature covers:
    /// `SHA-256("SEP-XXXX:key-binding" || bls_pubkey)`
    ///
    /// The caller MUST verify `signature` over this message using
    /// `ed25519Pubkey` via CryptoKit or equivalent. The SDK does not
    /// perform the Ed25519 verification itself.
    public func computeBindingMessage() -> Data {
        var hasher = SHA256()
        hasher.update(data: Data("SEP-XXXX:key-binding".utf8))
        hasher.update(data: blsPubkey)
        return Data(hasher.finalize())
    }
}

/// Broadcast by a new joiner after joining via invite code.
/// Existing members receive this, add the joiner to their member list,
/// and respond with a `SEPGroupStateUpdate` containing the updated state.
public struct SEPMemberJoined: Codable, Equatable, Sendable {
    public static let messageType = "sep_member_joined"

    public let type: String
    public let member: SEPGroupMemberLeaf
    public let transportBundle: SEPMemberTransportBundle?

    public init(member: SEPGroupMemberLeaf, transportBundle: SEPMemberTransportBundle? = nil) {
        self.type = Self.messageType
        self.member = member
        self.transportBundle = transportBundle
    }
}

/// Broadcast when a member renames the group.
public struct SEPGroupRenamed: Codable, Equatable, Sendable {
    public static let messageType = "sep_group_renamed"

    public let type: String
    public let name: String

    public init(name: String) {
        self.type = Self.messageType
        self.name = name
    }
}

/// Post-removal re-key message. Sent immediately after a member removal state update.
/// Contains the real salt (the state update carries a poisoned/placeholder salt so the
/// removed member cannot derive the new encryption key). Encrypted with the key derived
/// from the poisoned salt — only members still subscribed will receive and process it.
public struct SEPRekey: Codable, Equatable, Sendable {
    public static let messageType = "sep_rekey"

    public let type: String
    public let epoch: UInt64
    public let salt: Data

    public init(epoch: UInt64, salt: Data) {
        self.type = Self.messageType
        self.epoch = epoch
        self.salt = salt
    }
}

/// Delivery acknowledgment sent when a member receives and decrypts a message.
/// Each device sends at most one ACK per original event ID.
public struct SEPMessageAck: Codable, Equatable, Sendable {
    public static let messageType = "sep_message_ack"

    public let type: String
    public let eventID: String   // the original Nostr event ID that was delivered

    public init(eventID: String) {
        self.type = Self.messageType
        self.eventID = eventID
    }
}

// MARK: - Authenticated Member Transport Bundle

/// Authenticated mapping from a group member to their X25519 inbox encryption key.
/// Enables per-member asymmetric delivery of rekey material during removals.
/// The signature covers a domain-separated hash of all fields, signed by the member's
/// Stellar Ed25519 key, ensuring the inbox key cannot be spoofed.
public struct SEPMemberTransportBundle: Codable, Equatable, Sendable {
    public static let currentVersion: UInt16 = 1

    public let blsPubkey: Data             // 48 bytes, compressed G1
    public let stellarEd25519Pubkey: Data   // 32 bytes
    public let x25519InboxPubkey: Data      // 32 bytes
    public let version: UInt16
    public let signature: Data              // 64 bytes, Ed25519

    public init(
        blsPubkey: Data,
        stellarEd25519Pubkey: Data,
        x25519InboxPubkey: Data,
        version: UInt16 = currentVersion,
        signature: Data
    ) {
        self.blsPubkey = blsPubkey
        self.stellarEd25519Pubkey = stellarEd25519Pubkey
        self.x25519InboxPubkey = x25519InboxPubkey
        self.version = version
        self.signature = signature
    }

    public var hasValidStructure: Bool {
        blsPubkey.count == 48 &&
        stellarEd25519Pubkey.count == 32 &&
        x25519InboxPubkey.count == 32 &&
        signature.count == 64
    }

    /// Domain-separated binding message:
    /// SHA-256("SEP-XXXX:member-transport-v1" || bls || stellar || x25519 || version)
    public func computeBindingMessage() -> Data {
        var hasher = CryptoKit.SHA256()
        hasher.update(data: Data("SEP-XXXX:member-transport-v1".utf8))
        hasher.update(data: blsPubkey)
        hasher.update(data: stellarEd25519Pubkey)
        hasher.update(data: x25519InboxPubkey)
        var vBytes = version.bigEndian
        withUnsafeBytes(of: &vBytes) { hasher.update(bufferPointer: $0) }
        return Data(hasher.finalize())
    }
}

/// Non-secret notice broadcast on the old group channel during a secure removal.
/// Does NOT contain new groupSecret or salt — only metadata.
public struct SEPRemovalNotice: Codable, Equatable, Sendable {
    public static let messageType = "sep_removal_notice"

    public let type: String
    public let groupID: Data
    public let epoch: UInt64
    public let removedMemberKeys: [Data]
    public let commitment: Data?

    public init(
        groupID: Data,
        epoch: UInt64,
        removedMemberKeys: [Data],
        commitment: Data? = nil
    ) {
        self.type = Self.messageType
        self.groupID = groupID
        self.epoch = epoch
        self.removedMemberKeys = removedMemberKeys
        self.commitment = commitment
    }
}

/// Per-member rekey envelope delivered via X25519 inbox transport.
/// Contains fresh groupSecret and salt that the removed member never receives.
public struct SEPRekeyEnvelope: Codable, Equatable, Sendable {
    public static let messageType = "sep_rekey_envelope"

    public let type: String
    public let groupID: Data              // 32 bytes
    public let epoch: UInt64
    public let groupSecret: Data          // 32 bytes — the NEW group secret
    public let salt: Data                 // 32 bytes — the NEW salt
    public let commitment: Data?
    public let memberBundles: [SEPMemberTransportBundle]
    public let removedMemberKeys: [Data]
    public let relayHints: [String]
    public let senderBundle: SEPMemberTransportBundle

    public init(
        groupID: Data,
        epoch: UInt64,
        groupSecret: Data,
        salt: Data,
        commitment: Data? = nil,
        memberBundles: [SEPMemberTransportBundle],
        removedMemberKeys: [Data],
        relayHints: [String],
        senderBundle: SEPMemberTransportBundle
    ) {
        self.type = Self.messageType
        self.groupID = groupID
        self.epoch = epoch
        self.groupSecret = groupSecret
        self.salt = salt
        self.commitment = commitment
        self.memberBundles = memberBundles
        self.removedMemberKeys = removedMemberKeys
        self.relayHints = relayHints
        self.senderBundle = senderBundle
    }
}

/// Acknowledgment that a member received and installed a rekey envelope.
public struct SEPRekeyAck: Codable, Equatable, Sendable {
    public static let messageType = "sep_rekey_ack"

    public let type: String
    public let groupID: Data
    public let epoch: UInt64
    public let senderBlsPubkey: Data

    public init(groupID: Data, epoch: UInt64, senderBlsPubkey: Data) {
        self.type = Self.messageType
        self.groupID = groupID
        self.epoch = epoch
        self.senderBlsPubkey = senderBlsPubkey
    }
}

/// Request for a rekey envelope resend (sent via inbox when a member misses the original).
public struct SEPRekeyResendRequest: Codable, Equatable, Sendable {
    public static let messageType = "sep_rekey_resend_request"

    public let type: String
    public let groupID: Data
    public let epoch: UInt64
    public let requesterBundle: SEPMemberTransportBundle

    public init(groupID: Data, epoch: UInt64, requesterBundle: SEPMemberTransportBundle) {
        self.type = Self.messageType
        self.groupID = groupID
        self.epoch = epoch
        self.requesterBundle = requesterBundle
    }
}

// MARK: - Protocol Message Envelope

/// Wrapper for protocol messages sent over the encrypted group channel.
/// Allows distinguishing between plain text chat messages and protocol messages.
public struct SEPProtocolMessage: Codable, Sendable {
    public let type: String

    /// Attempt to parse a decrypted message as a protocol message.
    /// Returns nil if the message is plain text (not a protocol message).
    public static func parse(_ json: String) -> String? {
        guard let data = json.data(using: .utf8),
              let obj = try? JSONDecoder().decode(SEPProtocolMessage.self, from: data)
        else { return nil }
        return obj.type
    }
}

// MARK: - Relayer Configuration

/// Configuration for fee-decoupled transaction submission via a relayer.
///
/// The relayer receives the same contract invocation payload but wraps it
/// in a Stellar transaction signed by its own key, paying the network fee
/// on behalf of the group member. This prevents identity leakage through
/// the transaction signer address.
public struct SEPRelayerConfig: Codable, Equatable, Sendable {
    public let relayerURL: URL
    public let authToken: String?
    /// SHA-256 hashes of the relayer's TLS certificate public keys (base64-encoded).
    /// When non-empty, the transport will reject connections whose server certificate
    /// does not match any of the pinned hashes (H-14: TLS certificate pinning).
    public let pinnedCertificateHashes: [String]

    public init(relayerURL: URL, authToken: String? = nil, pinnedCertificateHashes: [String] = []) {
        self.relayerURL = relayerURL
        self.authToken = authToken
        self.pinnedCertificateHashes = pinnedCertificateHashes
    }
}
