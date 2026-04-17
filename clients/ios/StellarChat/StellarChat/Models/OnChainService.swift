import Foundation
import CryptoKit
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

enum OnChainError: LocalizedError {
    case provingKeyNotFound(tier: SEPTier, keyset: Int)
    case provingKeyHashMismatch(tier: SEPTier, expected: String, actual: String)

    var errorDescription: String? {
        switch self {
        case .provingKeyNotFound(let tier, let keyset):
            return "Proving key not found for tier \(tier.rawValue) in keyset-v\(keyset)"
        case .provingKeyHashMismatch(let tier, let expected, let actual):
            return "Proving key hash mismatch for tier \(tier.rawValue): expected \(expected), got \(actual)"
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

    /// Cached proving keys per tier (loaded from bundle on first use).
    private var provingKeys: [SEPTier: Data] = [:]

    /// Cached UpdateCircuit proving keys per tier. #59 fix: distinct circuit
    /// from the membership circuit, bundled in keyset-v2 after the ceremony.
    /// Until keyset-v2 lands (Phase 8), dev builds fall back to a testing key.
    private var updateProvingKeys: [SEPTier: Data] = [:]

    private static let maxRetries = 3
    private static let baseRetryDelay: TimeInterval = 1.0

    /// Current keyset version. Must match the resources in keyset-vN/.
    static let keysetVersion = 1

    /// Expected SHA-256 hashes of proving keys per tier.
    /// Update these after running scripts/generate-keyset.sh.
    private static let provingKeyHashes: [SEPTier: String] = [
        .small: "adca1962089d3f6bd89135f2cb1c20f44f7b5be3f83b279b8a8517ad5233f2d1",
        .medium: "630fbf2ad238f6153a143cf625c176b625870fd57535426133ed90e9fe03f215",
        .large: "f1e577cc9dde0cfa6cac569c66726199478a76c87b6c07067b3859783a4e355f",
    ]

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

    /// Load or return a cached proving key for the given tier.
    /// Proving keys are loaded from the app bundle's keyset resources.
    func ensureProvingKey(tier: SEPTier) throws -> Data {
        if let cached = provingKeys[tier] {
            return cached
        }
        let pk = try Self.loadProvingKeyFromBundle(tier: tier)
        provingKeys[tier] = pk
        return pk
    }

    /// Load or return a cached UpdateCircuit proving key for the given tier.
    /// TODO(Phase 8): load from `keyset-v2/update-<tier>.bin` with hash check.
    /// Dev fallback generates a deterministic testing key.
    func ensureUpdateProvingKey(tier: SEPTier) throws -> Data {
        if let cached = updateProvingKeys[tier] {
            return cached
        }
        let pk = try SEPProofGenerator.generateTestingUpdateProvingKey(tier: tier)
        updateProvingKeys[tier] = pk
        return pk
    }

    private static func tierResourceName(tier: SEPTier) -> String {
        switch tier {
        case .small: return "small"
        case .medium: return "medium"
        case .large: return "large"
        }
    }

    private static func loadProvingKeyFromBundle(tier: SEPTier) throws -> Data {
        let name = tierResourceName(tier: tier)
        let subdirectory = "keyset-v\(keysetVersion)"

        guard let url = Bundle.main.url(forResource: name, withExtension: "bin", subdirectory: subdirectory) else {
            throw OnChainError.provingKeyNotFound(tier: tier, keyset: keysetVersion)
        }

        let data = try Data(contentsOf: url)

        // Verify hash if configured
        if let expectedHash = provingKeyHashes[tier] {
            let actualHash = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
            guard actualHash == expectedHash else {
                throw OnChainError.provingKeyHashMismatch(tier: tier, expected: expectedHash, actual: actualHash)
            }
        }

        #if DEBUG
        print("[OnChainService] loaded proving key from bundle: \(subdirectory)/\(name).bin (\(data.count) bytes)")
        #endif
        return data
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
    /// #59: binds the new commitment inside the UpdateCircuit proof so the
    /// contract can cryptographically accept `c_new` as the persisted value.
    /// The new epoch is derived in-circuit as `epoch_old + 1`.
    func publishCommitmentUpdate(
        groupIDData: Data,
        oldMembers: [SEPGroupMemberLeaf],
        oldEpoch: UInt64,
        oldSalt: Data,
        newMembers: [SEPGroupMemberLeaf],
        newSalt: Data,
        blsSecretKey: Data,
        tier: SEPTier
    ) async throws -> SEPSubmissionResponse {
        let updatePK = try ensureUpdateProvingKey(tier: tier)
        let bundle = try SEPProofGenerator.generateUpdateProof(
            provingKey: updatePK,
            oldMembers: oldMembers,
            newMembers: newMembers,
            secretKey: blsSecretKey,
            epochOld: oldEpoch,
            saltOld: oldSalt,
            saltNew: newSalt,
            tier: tier
        )

        let uncompressedProof = try proofForContract(bundle.proof)

        let request = SEPUpdateCommitmentRequest(
            groupID: groupIDData,
            proof: uncompressedProof,
            publicInputs: bundle.publicInputs
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
