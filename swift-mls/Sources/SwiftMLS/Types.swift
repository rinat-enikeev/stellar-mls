import Foundation

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

public struct SEPMembershipProofBundle: Codable, Equatable, Sendable {
    public let proof: Data
    public let publicInputs: SEPPublicInputs

    public init(proof: Data, publicInputs: SEPPublicInputs) {
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

public struct SEPCreateGroupRequest: Codable, Equatable, Sendable {
    public let groupID: Data
    public let commitment: Data
    public let proof: Data
    public let publicInputs: SEPPublicInputs
    public let tier: UInt32

    public init(groupID: Data, commitment: Data, proof: Data, publicInputs: SEPPublicInputs, tier: UInt32) {
        self.groupID = groupID
        self.commitment = commitment
        self.proof = proof
        self.publicInputs = publicInputs
        self.tier = tier
    }
}

public struct SEPUpdateCommitmentRequest: Codable, Equatable, Sendable {
    public let groupID: Data
    public let newCommitment: Data
    public let newEpoch: UInt64
    public let proof: Data
    public let publicInputs: SEPPublicInputs

    public init(groupID: Data, newCommitment: Data, newEpoch: UInt64, proof: Data, publicInputs: SEPPublicInputs) {
        self.groupID = groupID
        self.newCommitment = newCommitment
        self.newEpoch = newEpoch
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
