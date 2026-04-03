import CryptoKit
import Foundation

public enum SEPCommitmentBuilder {
    public static func generateSalt() -> Data {
        var bytes = [UInt8](repeating: 0, count: 32)
        for index in bytes.indices {
            bytes[index] = UInt8.random(in: UInt8.min ... UInt8.max)
        }
        return Data(bytes)
    }

    /// Deterministic salt derivation for member-join events.
    /// All members processing the same SEPMemberJoined will derive the same salt,
    /// preventing epoch/key divergence.
    public static func deriveSalt(previousSalt: Data, memberKey: Data) -> Data {
        var data = Data()
        data.append(previousSalt)
        data.append(memberKey)
        let hash = SHA256.hash(data: data)
        return Data(hash)
    }

    public static func computeLeafHash(secretKey: Data) throws -> Data {
        try RustBridge.computeLeafHash(secretKey: secretKey)
    }

    public static func computePublicKey(secretKey: Data) throws -> Data {
        try RustBridge.computePublicKey(secretKey: secretKey)
    }

    public static func computeMerkleRoot(members: [SEPGroupMemberLeaf], tier: SEPTier) throws -> Data {
        try RustBridge.computeMerkleRoot(members: members, depth: tier.depth)
    }

    public static func computeSHA256Commitment(poseidonRoot: Data, epoch: UInt64, salt: Data) throws -> Data {
        try RustBridge.computeSHA256Commitment(poseidonRoot: poseidonRoot, epoch: epoch, salt: salt)
    }

    public static func computePoseidonCommitment(poseidonRoot: Data, epoch: UInt64, salt: Data) throws -> Data {
        try RustBridge.computePoseidonCommitment(poseidonRoot: poseidonRoot, epoch: epoch, salt: salt)
    }
}
