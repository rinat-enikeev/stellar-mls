import Foundation

public enum SEPProofGenerator {
    public static func generateTestingProvingKey(tier: SEPTier, seed: UInt64 = 42) throws -> Data {
        try RustBridge.generateTestingProvingKey(depth: tier.depth, seed: seed)
    }

    public static func generateMembershipProof(
        provingKey: Data,
        members: [SEPGroupMemberLeaf],
        secretKey: Data,
        epoch: UInt64,
        salt: Data,
        tier: SEPTier
    ) throws -> SEPMembershipProofBundle {
        try RustBridge.generateMembershipProof(
            provingKey: provingKey,
            members: members,
            secretKey: secretKey,
            epoch: epoch,
            salt: salt,
            depth: tier.depth
        )
    }
}
