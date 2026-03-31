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
        if poseidonRoot.count != 32 {
            throw SEPError.invalidFieldByteLength(expected: 32, actual: poseidonRoot.count)
        }
        if salt.count != 32 {
            throw SEPError.invalidSaltLength(actual: salt.count)
        }

        var preimage = Data()
        preimage.append(poseidonRoot)

        var epochBytes = epoch.bigEndian
        withUnsafeBytes(of: &epochBytes) { preimage.append(contentsOf: $0) }
        preimage.append(salt)

        return Data(SHA256.hash(data: preimage))
    }

    public static func computePoseidonCommitment(poseidonRoot: Data, epoch: UInt64, salt: Data) throws -> Data {
        try RustBridge.computePoseidonCommitment(poseidonRoot: poseidonRoot, epoch: epoch, salt: salt)
    }
}
