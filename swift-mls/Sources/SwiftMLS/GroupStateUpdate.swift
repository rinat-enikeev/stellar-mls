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
    public let commitment: Data?           // new SHA-256 commitment (optional)
    public let senderAttestation: SEPKeyAttestationPayload?

    public init(
        epoch: UInt64,
        salt: Data,
        addedMembers: [SEPGroupMemberLeaf] = [],
        removedMemberKeys: [Data] = [],
        commitment: Data? = nil,
        senderAttestation: SEPKeyAttestationPayload? = nil
    ) {
        self.type = Self.messageType
        self.epoch = epoch
        self.salt = salt
        self.addedMembers = addedMembers
        self.removedMemberKeys = removedMemberKeys
        self.commitment = commitment
        self.senderAttestation = senderAttestation
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

    public init(member: SEPGroupMemberLeaf) {
        self.type = Self.messageType
        self.member = member
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
