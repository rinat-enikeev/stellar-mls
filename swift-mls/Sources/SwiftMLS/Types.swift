import Foundation

/// Governance type selected at group creation. Mirrors the `group_type`
/// u32 persisted in the contract's `CommitmentEntryV2`. See
/// `docs/group-governance-types-design.md`.
public enum SEPGroupType: UInt32, Codable, CaseIterable, Sendable {
    case anarchy = 0
    case oneOnOne = 1
    case democracy = 2
    case oligarchy = 3
}

public enum SEPTier: Int, Codable, CaseIterable, Sendable {
    case small = 0
    case medium = 1
    case large = 2

    public var maxMembers: Int {
        switch self {
        case .small:
            return 32
        case .medium:
            return 256
        case .large:
            return 2048
        }
    }

    public var depth: Int {
        switch self {
        case .small:
            return 5
        case .medium:
            return 8
        case .large:
            return 11
        }
    }
}

public struct SEPPublicInputs: Codable, Equatable, Sendable {
    public let commitment: Data
    public let epoch: UInt64

    public init(commitment: Data, epoch: UInt64) {
        self.commitment = commitment
        self.epoch = epoch
    }
}

public struct SEPContractProofComponents: Codable, Equatable, Sendable {
    /// G1 uncompressed, 96 bytes
    public let proofA: Data
    /// G2 uncompressed, 192 bytes
    public let proofB: Data
    /// G1 uncompressed, 96 bytes
    public let proofC: Data

    public init(proofA: Data, proofB: Data, proofC: Data) {
        self.proofA = proofA
        self.proofB = proofB
        self.proofC = proofC
    }
}

public struct SEPMembershipProofBundle: Codable, Equatable, Sendable {
    public let proof: Data
    public let publicInputs: SEPPublicInputs

    public init(proof: Data, publicInputs: SEPPublicInputs) {
        self.proof = proof
        self.publicInputs = publicInputs
    }
}

/// Public inputs for the UpdateCircuit (#59 fix).
///
/// The contract binds `cNew` cryptographically via the proof, so the relayer
/// and contract no longer accept a client-supplied `new_commitment`. JSON keys
/// use snake_case to match the relayer payload schema.
public struct SEPUpdatePublicInputs: Codable, Equatable, Sendable {
    public let cOld: Data
    public let epochOld: UInt64
    public let cNew: Data

    public init(cOld: Data, epochOld: UInt64, cNew: Data) {
        self.cOld = cOld
        self.epochOld = epochOld
        self.cNew = cNew
    }

    enum CodingKeys: String, CodingKey {
        case cOld = "c_old"
        case epochOld = "epoch_old"
        case cNew = "c_new"
    }
}

public struct SEPUpdateProofBundle: Codable, Equatable, Sendable {
    public let proof: Data
    public let publicInputs: SEPUpdatePublicInputs

    public init(proof: Data, publicInputs: SEPUpdatePublicInputs) {
        self.proof = proof
        self.publicInputs = publicInputs
    }
}

public struct SEPGroupMemberLeaf: Codable, Equatable, Sendable {
    public let publicKeyCompressed: Data
    public let leafHash: Data

    public init(publicKeyCompressed: Data, leafHash: Data) {
        self.publicKeyCompressed = publicKeyCompressed
        self.leafHash = leafHash
    }
}

public struct SEPCommitmentEntry: Codable, Equatable, Sendable {
    public let commitment: Data
    public let epoch: UInt64
    public let timestamp: UInt64
    public let tier: UInt32
    public let active: Bool

    public init(commitment: Data, epoch: UInt64, timestamp: UInt64, tier: UInt32, active: Bool) {
        self.commitment = commitment
        self.epoch = epoch
        self.timestamp = timestamp
        self.tier = tier
        self.active = active
    }
}

/// On-chain state returned by `get_state_v2` — extends `SEPCommitmentEntry`
/// with governance-type metadata. Legacy groups stored under V1 are
/// projected to `groupType = .anarchy`, `memberCount = 0`.
public struct SEPCommitmentEntryV2: Codable, Equatable, Sendable {
    public let commitment: Data
    public let epoch: UInt64
    public let timestamp: UInt64
    public let tier: UInt32
    public let active: Bool
    public let groupType: SEPGroupType
    public let memberCount: UInt32

    public init(
        commitment: Data,
        epoch: UInt64,
        timestamp: UInt64,
        tier: UInt32,
        active: Bool,
        groupType: SEPGroupType,
        memberCount: UInt32
    ) {
        self.commitment = commitment
        self.epoch = epoch
        self.timestamp = timestamp
        self.tier = tier
        self.active = active
        self.groupType = groupType
        self.memberCount = memberCount
    }

    enum CodingKeys: String, CodingKey {
        case commitment
        case epoch
        case timestamp
        case tier
        case active
        case groupType = "group_type"
        case memberCount = "member_count"
    }
}

public struct SEPCreateGroupRequest: Codable, Equatable, Sendable {
    public let caller: String
    public let groupID: Data
    public let commitment: Data
    public let proof: Data
    public let publicInputs: SEPPublicInputs
    public let tier: UInt32

    public init(caller: String, groupID: Data, commitment: Data, proof: Data, publicInputs: SEPPublicInputs, tier: UInt32) {
        self.caller = caller
        self.groupID = groupID
        self.commitment = commitment
        self.proof = proof
        self.publicInputs = publicInputs
        self.tier = tier
    }
}

/// Payload for the `create_group_v2` contract entrypoint. Supports
/// Anarchy (`groupType = .anarchy`), 1v1 (`groupType = .oneOnOne`), and
/// Democracy (`groupType = .democracy`). Oligarchy creation uses the
/// dedicated `SEPCreateOligarchyGroupRequest` because it seeds an extra
/// admin root.
public struct SEPCreateGroupV2Request: Codable, Equatable, Sendable {
    public let caller: String
    public let groupID: Data
    public let commitment: Data
    public let tier: UInt32
    public let groupType: SEPGroupType
    public let memberCount: UInt32
    public let proof: Data
    public let publicInputs: SEPPublicInputs

    public init(
        caller: String,
        groupID: Data,
        commitment: Data,
        tier: UInt32,
        groupType: SEPGroupType,
        memberCount: UInt32,
        proof: Data,
        publicInputs: SEPPublicInputs
    ) {
        self.caller = caller
        self.groupID = groupID
        self.commitment = commitment
        self.tier = tier
        self.groupType = groupType
        self.memberCount = memberCount
        self.proof = proof
        self.publicInputs = publicInputs
    }

    enum CodingKeys: String, CodingKey {
        case caller
        case groupID = "group_id"
        case commitment
        case tier
        case groupType = "group_type"
        case memberCount = "member_count"
        case proof
        case publicInputs = "public_inputs"
    }
}

/// Payload for the `create_oligarchy_group` contract entrypoint.
/// `adminRoot` is the Poseidon-hashed commitment to the admin tree and
/// is pinned at creation; later admin rotations are ceremony-gated.
public struct SEPCreateOligarchyGroupRequest: Codable, Equatable, Sendable {
    public let caller: String
    public let groupID: Data
    public let commitment: Data
    public let tier: UInt32
    public let memberCount: UInt32
    public let adminRoot: Data
    public let proof: Data
    public let publicInputs: SEPPublicInputs

    public init(
        caller: String,
        groupID: Data,
        commitment: Data,
        tier: UInt32,
        memberCount: UInt32,
        adminRoot: Data,
        proof: Data,
        publicInputs: SEPPublicInputs
    ) {
        self.caller = caller
        self.groupID = groupID
        self.commitment = commitment
        self.tier = tier
        self.memberCount = memberCount
        self.adminRoot = adminRoot
        self.proof = proof
        self.publicInputs = publicInputs
    }

    enum CodingKeys: String, CodingKey {
        case caller
        case groupID = "group_id"
        case commitment
        case tier
        case memberCount = "member_count"
        case adminRoot = "admin_root"
        case proof
        case publicInputs = "public_inputs"
    }
}

public struct SEPUpdateCommitmentRequest: Codable, Equatable, Sendable {
    public let groupID: Data
    public let proof: Data
    public let publicInputs: SEPUpdatePublicInputs

    public init(groupID: Data, proof: Data, publicInputs: SEPUpdatePublicInputs) {
        self.groupID = groupID
        self.proof = proof
        self.publicInputs = publicInputs
    }
}

public struct SEPVerifyMembershipRequest: Codable, Equatable, Sendable {
    public let groupID: Data
    public let proof: Data
    public let publicInputs: SEPPublicInputs

    public init(groupID: Data, proof: Data, publicInputs: SEPPublicInputs) {
        self.groupID = groupID
        self.proof = proof
        self.publicInputs = publicInputs
    }
}

public struct SEPDeactivateGroupRequest: Codable, Equatable, Sendable {
    public let groupID: Data
    public let proof: Data
    public let publicInputs: SEPPublicInputs

    public init(groupID: Data, proof: Data, publicInputs: SEPPublicInputs) {
        self.groupID = groupID
        self.proof = proof
        self.publicInputs = publicInputs
    }
}

public struct SEPGetStateRequest: Codable, Equatable, Sendable {
    public let groupID: Data

    public init(groupID: Data) {
        self.groupID = groupID
    }
}

public struct SEPGetHistoryRequest: Codable, Equatable, Sendable {
    public let groupID: Data
    public let maxEntries: UInt32

    public init(groupID: Data, maxEntries: UInt32) {
        self.groupID = groupID
        self.maxEntries = maxEntries
    }
}

public struct SEPSubmissionResponse: Codable, Equatable, Sendable {
    public let accepted: Bool
    public let transactionHash: String?
    public let message: String?

    public init(accepted: Bool, transactionHash: String? = nil, message: String? = nil) {
        self.accepted = accepted
        self.transactionHash = transactionHash
        self.message = message
    }
}

public struct SEPVerifyMembershipResponse: Codable, Equatable, Sendable {
    public let valid: Bool

    public init(valid: Bool) {
        self.valid = valid
    }
}
