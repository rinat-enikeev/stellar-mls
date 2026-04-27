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

enum DemocracyProofError: LocalizedError {
    case coordinatorNotImplemented

    var errorDescription: String? {
        switch self {
        case .coordinatorNotImplemented:
            return "Democracy coordinator protocol not yet implemented: K voters' BLS secret keys must reach the finalizer to assemble the witness (§6.4.2)."
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
    /// from the membership circuit, bundled in keyset-v2.
    private var updateProvingKeys: [SEPTier: Data] = [:]

    private static let maxRetries = 3
    private static let baseRetryDelay: TimeInterval = 1.0

    /// Current keyset version. Must match the resources in keyset-vN/.
    static let keysetVersion = 2

    /// Expected SHA-256 hashes of membership proving keys per tier (keyset-v2).
    private static let provingKeyHashes: [SEPTier: String] = [
        .small: "88ae981de7f91d79caad9cdb9076dd990980beab575aba461777aae592ff599d",
        .medium: "206400e7d1e74f6cbe96ebdab989a3e79090bdd042ddf19dcc1dee02277c0cf0",
        .large: "2daafd9ab533daea483fe433d18922b2c7a63058f2c166e298d17b843ed45d16",
    ]

    /// Expected SHA-256 hashes of UpdateCircuit proving keys per tier (keyset-v2).
    private static let updateProvingKeyHashes: [SEPTier: String] = [
        .small: "6b83e2300430bb1bd37b929ccb7d135f81d4f966745da661df100d38d1e8dbcc",
        .medium: "ee468305f7566782448bc495f122ded42280299bad088c4f567b245205e6ed54",
        .large: "16ce9e2e712bfcc1c46bbd9d4274a9210bdcd13f7258294b96ccfd47a5f60900",
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
    /// Loads from `keyset-v<N>/update-<tier>.bin` with SHA-256 hash verification.
    func ensureUpdateProvingKey(tier: SEPTier) throws -> Data {
        if let cached = updateProvingKeys[tier] {
            return cached
        }
        let pk = try Self.loadUpdateProvingKeyFromBundle(tier: tier)
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

    private static func loadUpdateProvingKeyFromBundle(tier: SEPTier) throws -> Data {
        let name = "update-\(tierResourceName(tier: tier))"
        let subdirectory = "keyset-v\(keysetVersion)"

        guard let url = Bundle.main.url(forResource: name, withExtension: "bin", subdirectory: subdirectory) else {
            throw OnChainError.provingKeyNotFound(tier: tier, keyset: keysetVersion)
        }

        let data = try Data(contentsOf: url)

        if let expectedHash = updateProvingKeyHashes[tier] {
            let actualHash = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
            guard actualHash == expectedHash else {
                throw OnChainError.provingKeyHashMismatch(tier: tier, expected: expectedHash, actual: actualHash)
            }
        }

        #if DEBUG
        print("[OnChainService] loaded update proving key from bundle: \(subdirectory)/\(name).bin (\(data.count) bytes)")
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
    /// 3. Submits the correct creation entrypoint for the requested
    ///    governance type:
    ///     - `.anarchy` → `create_group` (V1)
    ///     - `.oneOnOne`, `.democracy` → `create_group_v2`
    ///     - `.oligarchy` → `create_oligarchy_group` (requires `adminLeaves`
    ///       and `adminSalt` to seed the admin tree)
    func publishGroupCreation(
        groupIDData: Data,
        members: [SEPGroupMemberLeaf],
        blsSecretKey: Data,
        epoch: UInt64,
        salt: Data,
        tier: SEPTier,
        callerAddress: String,
        groupType: SEPGroupType = .anarchy,
        memberCount: UInt32? = nil,
        adminLeaves: [SEPGroupMemberLeaf]? = nil,
        adminSalt: Data? = nil
    ) async throws -> SEPSubmissionResponse {
        #if DEBUG
        print("[OnChainService] publishGroupCreation start groupType=\(groupType.rawValue)")
        #endif
        let proofBundle = try generateProof(
            members: members,
            blsSecretKey: blsSecretKey,
            epoch: epoch,
            salt: salt,
            tier: tier
        )

        let uncompressedProof = try proofForContract(proofBundle.proof)
        let effectiveMemberCount = memberCount ?? UInt32(members.count)

        #if DEBUG
        let firstMember = members.first
        print(
            "[OnChainService] publishGroupCreation invoke caller=\(callerAddress.prefix(8)) " +
            "group=\(groupIDData.debugHexPrefix(12)) epoch=\(epoch) tier=\(tier.rawValue) " +
            "groupType=\(groupType.rawValue) memberCount=\(effectiveMemberCount) " +
            "members=\(members.count) salt=\(salt.debugHexPrefix(12)) " +
            "proofCommitment=\(proofBundle.publicInputs.commitment.debugHexPrefix(12)) " +
            "firstPk=\(firstMember?.publicKeyCompressed.debugHexPrefix(12) ?? "none") " +
            "firstLeaf=\(firstMember?.leafHash.debugHexPrefix(12) ?? "none")"
        )
        #endif

        switch groupType {
        case .anarchy:
            let request = SEPCreateGroupRequest(
                caller: callerAddress,
                groupID: groupIDData,
                commitment: proofBundle.publicInputs.commitment,
                proof: uncompressedProof,
                publicInputs: proofBundle.publicInputs,
                tier: UInt32(tier.rawValue)
            )
            return try await withRetry { try await self.contractClient.createGroup(request) }

        case .oneOnOne, .democracy:
            let request = SEPCreateGroupV2Request(
                caller: callerAddress,
                groupID: groupIDData,
                commitment: proofBundle.publicInputs.commitment,
                tier: UInt32(tier.rawValue),
                groupType: groupType,
                memberCount: effectiveMemberCount,
                proof: uncompressedProof,
                publicInputs: proofBundle.publicInputs
            )
            return try await withRetry { try await self.contractClient.createGroupV2(request) }

        case .oligarchy:
            guard let adminLeaves, !adminLeaves.isEmpty, let adminSalt else {
                throw ChatError.onChainPublishFailed(
                    "Oligarchy creation requires adminLeaves + adminSalt to seed the admin tree"
                )
            }
            let adminCommitment = try SEPCommitmentBuilder.computeAdminCommitment(
                admins: adminLeaves,
                salt: adminSalt
            )
            let request = SEPCreateOligarchyGroupRequest(
                caller: callerAddress,
                groupID: groupIDData,
                commitment: proofBundle.publicInputs.commitment,
                tier: UInt32(tier.rawValue),
                memberCount: effectiveMemberCount,
                adminRoot: adminCommitment,
                proof: uncompressedProof,
                publicInputs: proofBundle.publicInputs
            )
            return try await withRetry { try await self.contractClient.createOligarchyGroup(request) }
        }
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

    /// Bounded retry for on-chain state reads when the RPC node is expected
    /// to trail the network briefly. Used on the receive path: when a peer
    /// broadcasts an update for epoch N, our RPC may not yet have indexed
    /// the ledger close that committed epoch N to the contract. Polling
    /// once and rejecting on mismatch would silently drop a legitimate
    /// update; polling forever risks blocking on a genuinely-missing
    /// commitment.
    ///
    /// Returns the most recent entry seen — even when its epoch still
    /// trails `expectedEpoch` after exhausting `maxAttempts` — so the
    /// caller can still compare the stale entry and make a chain-authority
    /// decision. Re-throws the last error only if every fetch attempt
    /// threw. Extracted as a static helper (delegating `fetch`) so the
    /// retry semantics can be unit-tested without a live RPC.
    static func fetchOnChainStateAwaitingEpoch(
        fetch: () async throws -> SEPCommitmentEntry,
        expectedEpoch: UInt64,
        maxAttempts: Int = 4,
        initialDelayMs: UInt64 = 250,
        sleep: (UInt64) async -> Void = { ms in
            try? await Task.sleep(nanoseconds: ms * 1_000_000)
        }
    ) async throws -> SEPCommitmentEntry {
        precondition(maxAttempts >= 1, "maxAttempts must be >= 1")
        var delayMs: UInt64 = initialDelayMs
        var lastEntry: SEPCommitmentEntry?
        var lastError: Error?
        for attempt in 0..<maxAttempts {
            do {
                let entry = try await fetch()
                lastEntry = entry
                if entry.epoch >= expectedEpoch { return entry }
            } catch {
                lastError = error
            }
            if attempt < maxAttempts - 1 {
                await sleep(delayMs)
                delayMs *= 2
            }
        }
        if let entry = lastEntry { return entry }
        throw lastError ?? NSError(
            domain: "OnChainService",
            code: -1,
            userInfo: [NSLocalizedDescriptionKey: "fetchOnChainState failed after \(maxAttempts) attempts"]
        )
    }

    // MARK: - Democracy (#26)

    /// Delta kind for a Democracy update (add/remove/kick), matching the
    /// DemocracyUpdateCircuit witness encoding (§6.4.2).
    enum DemocracyDeltaKind: UInt8 {
        case add = 0
        case remove = 1
        case kick = 2
    }

    /// Generate a Groth16 proof for a quorum-signed Democracy update.
    ///
    /// **Status: Stub.** The witness requires K signer BLS secret keys plus
    /// per-signer Merkle paths into the member tree. Voters hold those keys
    /// locally; no coordinator protocol currently exists to assemble them at
    /// the finalizer. Throws `DemocracyProofError.coordinatorNotImplemented`
    /// until that design is resolved. See §6.4.2 and the #26 tracking note.
    func generateDemocracyUpdateProof(
        ballotID: String,
        oldMembers: [SEPGroupMemberLeaf],
        newMembers: [SEPGroupMemberLeaf],
        deltaKind: DemocracyDeltaKind,
        signerSecretKeys: [Data],
        signerPublicKeys: [Data],
        saltOld: Data,
        saltNew: Data,
        epochOld: UInt64,
        tier: SEPTier
    ) async throws -> Never {
        _ = (ballotID, oldMembers, newMembers, deltaKind, signerSecretKeys,
             signerPublicKeys, saltOld, saltNew, epochOld, tier)
        throw DemocracyProofError.coordinatorNotImplemented
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
