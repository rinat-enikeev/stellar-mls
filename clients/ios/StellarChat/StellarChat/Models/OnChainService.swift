import Foundation
import SwiftMLS

/// Result of verifying local group state against on-chain commitment.
enum OnChainVerificationResult: Equatable {
    case notPublished
    case verified
    case epochMismatch(local: UInt64, onChain: UInt64)
    case commitmentMismatch
    case inactive
    case error(String)

    var displayText: String {
        switch self {
        case .notPublished:
            return "Not published on-chain"
        case .verified:
            return "Verified on-chain"
        case .epochMismatch(let local, let onChain):
            return "Epoch mismatch: local \(local), on-chain \(onChain)"
        case .commitmentMismatch:
            return "Commitment mismatch"
        case .inactive:
            return "Group deactivated on-chain"
        case .error(let message):
            return "Error: \(message)"
        }
    }
}

/// Orchestrates ZK proof generation and Soroban contract interaction.
///
/// Uses the existing `SEPContractClient` from SwiftMLS for HTTP-based
/// contract invocations and `SEPProofGenerator` for Groth16 proof generation.
///
/// M-10: Network operations are retried with exponential backoff (up to 3 attempts).
/// Groups continue to function in "unverified" mode when the endpoint is unreachable.
actor OnChainService {
    let contractClient: SEPContractClient

    /// Cached proving keys per tier (generated on first use).
    private var provingKeys: [SEPTier: Data] = [:]

    private static let maxRetries = 3
    private static let baseRetryDelay: TimeInterval = 1.0

    /// Execute an async block with exponential backoff retry on URLError.
    private func withRetry<T>(_ block: () async throws -> T) async throws -> T {
        var lastError: Error?
        for attempt in 0..<Self.maxRetries {
            do {
                return try await block()
            } catch let error as URLError {
                lastError = error
                if attempt < Self.maxRetries - 1 {
                    let delay = Self.baseRetryDelay * pow(2.0, Double(attempt))
                    try? await Task.sleep(for: .seconds(delay))
                }
            }
        }
        throw lastError!
    }

    init(contractID: String, endpoint: URL) {
        let transport = URLSessionSEPContractTransport(endpoint: endpoint)
        self.contractClient = SEPContractClient(contractID: contractID, transport: transport)
    }

    /// Initialize with a relayer transport for fee-decoupled submission.
    init(contractID: String, relayerConfig: SEPRelayerConfig) {
        let transport = SEPRelayerTransport(config: relayerConfig)
        self.contractClient = SEPContractClient(contractID: contractID, transport: transport)
    }

    // MARK: - Proving Key Management

    /// Generate or return a cached proving key for the given tier.
    /// Proving key generation is expensive; results are cached in memory.
    func ensureProvingKey(tier: SEPTier) throws -> Data {
        if let cached = provingKeys[tier] {
            #if DEBUG
            print("[OnChainService] using cached proving key tier=\(tier.rawValue)")
            #endif
            return cached
        }
        #if DEBUG
        print("[OnChainService] generating proving key tier=\(tier.rawValue)")
        #endif
        let pk = try SEPProofGenerator.generateTestingProvingKey(tier: tier)
        provingKeys[tier] = pk
        #if DEBUG
        print("[OnChainService] generated proving key bytes=\(pk.count)")
        #endif
        return pk
    }

    // MARK: - Proof Generation

    /// Generate a Groth16 membership proof for the given group state.
    func generateProof(
        members: [SEPGroupMemberLeaf],
        blsSecretKey: Data,
        epoch: UInt64,
        salt: Data,
        tier: SEPTier
    ) throws -> SEPMembershipProofBundle {
        #if DEBUG
        print("[OnChainService] generateProof start epoch=\(epoch) members=\(members.count) tier=\(tier.rawValue)")
        #endif
        let pk = try ensureProvingKey(tier: tier)
        let bundle = try SEPProofGenerator.generateMembershipProof(
            provingKey: pk,
            members: members,
            secretKey: blsSecretKey,
            epoch: epoch,
            salt: salt,
            tier: tier
        )
        #if DEBUG
        print("[OnChainService] generateProof success proofBytes=\(bundle.proof.count) commitmentBytes=\(bundle.publicInputs.commitment.count)")
        #endif
        return bundle
    }

    // MARK: - Proof Format Conversion

    /// Convert a compressed Groth16 proof (192 bytes) to the uncompressed
    /// contract format (384 bytes = proofA‖proofB‖proofC) expected by the
    /// Soroban contract's `Groth16Proof { a: BytesN<96>, b: BytesN<192>, c: BytesN<96> }`.
    private func proofForContract(_ compressedProof: Data) throws -> Data {
        #if DEBUG
        print("[OnChainService] proofForContract start compressedBytes=\(compressedProof.count)")
        #endif
        let components = try SEPProofGenerator.proofToContractFormat(compressedProof: compressedProof)
        // Concatenate: A (96 bytes) || B (192 bytes) || C (96 bytes) = 384 bytes
        var uncompressed = Data(capacity: 384)
        uncompressed.append(components.proofA)
        uncompressed.append(components.proofB)
        uncompressed.append(components.proofC)
        #if DEBUG
        print("[OnChainService] proofForContract success uncompressedBytes=\(uncompressed.count)")
        #endif
        return uncompressed
    }

    // MARK: - Contract Operations

    /// Publish a new group creation on-chain.
    ///
    /// 1. Generates a membership proof at the initial state (epoch 0).
    /// 2. Decompresses the proof to uncompressed BLS12-381 points (384 bytes).
    /// 3. Submits `create_group` to the Soroban contract.
    func publishGroupCreation(
        groupIDData: Data,
        members: [SEPGroupMemberLeaf],
        blsSecretKey: Data,
        epoch: UInt64,
        salt: Data,
        tier: SEPTier,
        callerAddress: String
    ) async throws -> SEPSubmissionResponse {
        #if DEBUG
        print("[OnChainService] publishGroupCreation start")
        #endif
        let proofBundle = try generateProof(
            members: members,
            blsSecretKey: blsSecretKey,
            epoch: epoch,
            salt: salt,
            tier: tier
        )

        let uncompressedProof = try proofForContract(proofBundle.proof)

        let request = SEPCreateGroupRequest(
            caller: callerAddress,
            groupID: groupIDData,
            commitment: proofBundle.publicInputs.commitment,
            proof: uncompressedProof,
            publicInputs: proofBundle.publicInputs,
            tier: UInt32(tier.rawValue)
        )
        #if DEBUG
        let firstMember = members.first
        print(
            "[OnChainService] publishGroupCreation invoke caller=\(callerAddress.prefix(8)) " +
            "group=\(groupIDData.debugHexPrefix(12)) epoch=\(epoch) tier=\(tier.rawValue) " +
            "members=\(members.count) salt=\(salt.debugHexPrefix(12)) " +
            "proofCommitment=\(proofBundle.publicInputs.commitment.debugHexPrefix(12)) " +
            "firstPk=\(firstMember?.publicKeyCompressed.debugHexPrefix(12) ?? "none") " +
            "firstLeaf=\(firstMember?.leafHash.debugHexPrefix(12) ?? "none")"
        )
        #endif
        return try await withRetry { try await self.contractClient.createGroup(request) }
    }

    /// Publish a commitment update after a membership change.
    ///
    /// 1. Generates a membership proof at the OLD state (proving current membership).
    /// 2. Decompresses the proof to uncompressed format (384 bytes).
    /// 3. Computes the new Poseidon commitment from the new state.
    /// 4. Submits `update_commitment` to the Soroban contract.
    func publishCommitmentUpdate(
        groupIDData: Data,
        oldMembers: [SEPGroupMemberLeaf],
        oldEpoch: UInt64,
        oldSalt: Data,
        newMembers: [SEPGroupMemberLeaf],
        newEpoch: UInt64,
        newSalt: Data,
        blsSecretKey: Data,
        tier: SEPTier
    ) async throws -> SEPSubmissionResponse {
        // Proof is generated against the OLD (current on-chain) state
        let oldProofBundle = try generateProof(
            members: oldMembers,
            blsSecretKey: blsSecretKey,
            epoch: oldEpoch,
            salt: oldSalt,
            tier: tier
        )

        let uncompressedProof = try proofForContract(oldProofBundle.proof)

        // Compute the new Poseidon commitment for the updated state
        let newRoot = try SEPCommitmentBuilder.computeMerkleRoot(members: newMembers, tier: tier)
        let newPoseidonCommitment = try SEPCommitmentBuilder.computePoseidonCommitment(
            poseidonRoot: newRoot,
            epoch: newEpoch,
            salt: newSalt
        )

        let request = SEPUpdateCommitmentRequest(
            groupID: groupIDData,
            newCommitment: newPoseidonCommitment,
            newEpoch: newEpoch,
            proof: uncompressedProof,
            publicInputs: oldProofBundle.publicInputs
        )
        return try await withRetry { try await self.contractClient.updateCommitment(request) }
    }

    /// Fetch the current on-chain state for a group.
    func fetchOnChainState(groupIDData: Data) async throws -> SEPCommitmentEntry {
        try await withRetry { try await self.contractClient.getState(groupID: groupIDData) }
    }

    // MARK: - Verification

    /// Verify that a group's local state matches its on-chain commitment.
    ///
    /// Computes the Poseidon commitment from local (members, epoch, salt)
    /// and compares it against the on-chain stored commitment.
    func verifyCommitment(
        groupIDData: Data,
        members: [SEPGroupMemberLeaf],
        epoch: UInt64,
        salt: Data,
        tier: SEPTier
    ) async -> OnChainVerificationResult {
        do {
            let entry = try await fetchOnChainState(groupIDData: groupIDData)

            if !entry.active {
                return .inactive
            }

            if entry.epoch != epoch {
                return .epochMismatch(local: epoch, onChain: entry.epoch)
            }

            // Compute local Poseidon commitment and compare
            let root = try SEPCommitmentBuilder.computeMerkleRoot(members: members, tier: tier)
            let localPoseidonCommitment = try SEPCommitmentBuilder.computePoseidonCommitment(
                poseidonRoot: root,
                epoch: epoch,
                salt: salt
            )
            #if DEBUG
            let firstMember = members.first
            print(
                "[OnChainService] verifyCommitment group=\(groupIDData.debugHexPrefix(12)) " +
                "epoch=\(epoch) onChainEpoch=\(entry.epoch) tier=\(tier.rawValue) " +
                "members=\(members.count) salt=\(salt.debugHexPrefix(12)) " +
                "localPoseidon=\(localPoseidonCommitment.debugHexPrefix(12)) " +
                "onChainCommitment=\(entry.commitment.debugHexPrefix(12)) " +
                "firstPk=\(firstMember?.publicKeyCompressed.debugHexPrefix(12) ?? "none") " +
                "firstLeaf=\(firstMember?.leafHash.debugHexPrefix(12) ?? "none")"
            )
            #endif

            if localPoseidonCommitment == entry.commitment {
                return .verified
            } else {
                return .commitmentMismatch
            }
        } catch let error as SEPError where error.errorDescription?.contains("GroupNotFound") == true {
            return .notPublished
        } catch {
            // Distinguish "group not found" from real errors
            let message = error.localizedDescription
            if message.contains("404") || message.contains("not found") || message.contains("GroupNotFound") {
                return .notPublished
            }
            return .error(message)
        }
    }

    /// Deactivate a group on-chain. Requires a valid ZK proof of membership.
    ///
    /// Any authorized member can deactivate. The contract sets `active = false`,
    /// preventing further commitment updates.
    func deactivateGroup(
        groupIDData: Data,
        members: [SEPGroupMemberLeaf],
        blsSecretKey: Data,
        epoch: UInt64,
        salt: Data,
        tier: SEPTier
    ) async throws -> SEPSubmissionResponse {
        let proofBundle = try generateProof(
            members: members,
            blsSecretKey: blsSecretKey,
            epoch: epoch,
            salt: salt,
            tier: tier
        )

        let uncompressedProof = try proofForContract(proofBundle.proof)

        let request = SEPDeactivateGroupRequest(
            groupID: groupIDData,
            proof: uncompressedProof,
            publicInputs: proofBundle.publicInputs
        )
        return try await withRetry { try await self.contractClient.deactivateGroup(request) }
    }

    /// Verify membership via the on-chain contract.
    ///
    /// Generates a fresh proof, decompresses to 384-byte contract format,
    /// and submits `verify_membership` to the contract.
    func verifyMembership(
        groupIDData: Data,
        members: [SEPGroupMemberLeaf],
        blsSecretKey: Data,
        epoch: UInt64,
        salt: Data,
        tier: SEPTier
    ) async throws -> Bool {
        let proofBundle = try generateProof(
            members: members,
            blsSecretKey: blsSecretKey,
            epoch: epoch,
            salt: salt,
            tier: tier
        )

        let uncompressedProof = try proofForContract(proofBundle.proof)

        let request = SEPVerifyMembershipRequest(
            groupID: groupIDData,
            proof: uncompressedProof,
            publicInputs: proofBundle.publicInputs
        )
        let response = try await withRetry { try await self.contractClient.verifyMembership(request) }
        return response.valid
    }
}

#if DEBUG
private extension Data {
    func debugHexPrefix(_ bytes: Int = 8) -> String {
        prefix(bytes).map { String(format: "%02x", $0) }.joined()
    }
}
#endif
