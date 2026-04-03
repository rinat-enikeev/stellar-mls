import Foundation

public struct SEPInvitationBootstrap: Codable, Equatable, Sendable {
    public let groupID: Data
    public let epoch: UInt64
    public let stellarContractID: String
    public let relayHints: [URL]
    public let welcomePayload: Data
    public let sepBootstrapMaterial: Data

    public init(
        groupID: Data,
        epoch: UInt64,
        stellarContractID: String,
        relayHints: [URL],
        welcomePayload: Data,
        sepBootstrapMaterial: Data
    ) {
        self.groupID = groupID
        self.epoch = epoch
        self.stellarContractID = stellarContractID
        self.relayHints = relayHints
        self.welcomePayload = welcomePayload
        self.sepBootstrapMaterial = sepBootstrapMaterial
    }
}

public struct SEPSealedInvitationEnvelope: Codable, Equatable, Sendable {
    public let version: UInt32
    public let scheme: String
    public let ephemeralPublicKey: Data?
    public let nonce: Data?
    public let ciphertext: Data
    public let authenticationTag: Data?

    public init(
        version: UInt32,
        scheme: String,
        ephemeralPublicKey: Data? = nil,
        nonce: Data? = nil,
        ciphertext: Data,
        authenticationTag: Data? = nil
    ) {
        self.version = version
        self.scheme = scheme
        self.ephemeralPublicKey = ephemeralPublicKey
        self.nonce = nonce
        self.ciphertext = ciphertext
        self.authenticationTag = authenticationTag
    }
}

public struct SEPInvitationSendOptions: Equatable, Sendable {
    public let kind: Int
    public let createdAt: Int64?
    public let additionalTags: [[String]]

    public init(
        kind: Int = 34_113,
        createdAt: Int64? = nil,
        additionalTags: [[String]] = []
    ) {
        self.kind = kind
        self.createdAt = createdAt
        self.additionalTags = additionalTags
    }
}

public struct SEPInvitationSendResult: Equatable, Sendable {
    public let event: SEPNostrEvent
    public let relayResults: [SEPNostrRelaySendResult]

    public init(event: SEPNostrEvent, relayResults: [SEPNostrRelaySendResult]) {
        self.event = event
        self.relayResults = relayResults
    }
}

public struct SEPNostrEvent: Codable, Equatable, Sendable {
    public let id: String
    public let pubkey: String
    public let createdAt: Int64
    public let kind: Int
    public let tags: [[String]]
    public let content: String
    public let sig: String

    enum CodingKeys: String, CodingKey {
        case id
        case pubkey
        case createdAt = "created_at"
        case kind
        case tags
        case content
        case sig
    }

    public init(
        id: String,
        pubkey: String,
        createdAt: Int64,
        kind: Int,
        tags: [[String]],
        content: String,
        sig: String
    ) {
        self.id = id
        self.pubkey = pubkey
        self.createdAt = createdAt
        self.kind = kind
        self.tags = tags
        self.content = content
        self.sig = sig
    }
}

public struct SEPNostrRelaySendResult: Equatable, Sendable {
    public let relayURL: URL
    public let accepted: Bool
    public let message: String?

    public init(relayURL: URL, accepted: Bool, message: String? = nil) {
        self.relayURL = relayURL
        self.accepted = accepted
        self.message = message
    }
}

public protocol SEPInvitationCryptoProvider: Sendable {
    func hiddenInboxTag(recipientPublicKey: Data) throws -> String
    func sealInvitation(_ plaintext: Data, recipientPublicKey: Data) throws -> SEPSealedInvitationEnvelope
}

public protocol SEPNostrEventSigner: Sendable {
    func publicKey() throws -> Data
    func signEventID(_ eventID: Data) throws -> Data
}

public protocol SEPNostrRelayTransport: Sendable {
    func publish(event: SEPNostrEvent, to relayURL: URL) async throws -> SEPNostrRelaySendResult
}
