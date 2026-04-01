import SwiftUI
import SwiftMLS

@main
struct StellarChatApp: App {
    @State private var appState = AppState()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(appState)
                .task {
                    await appState.startInboxListener()
                }
                .onOpenURL { url in
                    // Handle stellarchat://join?code=<base64>
                    if url.scheme == "stellarchat", url.host == "join",
                       let code = URLComponents(url: url, resolvingAgainstBaseURL: false)?
                        .queryItems?.first(where: { $0.name == "code" })?.value {
                        appState.deepLinkInviteCode = code
                    }
                }
        }
    }
}

/// N-24: @MainActor isolation ensures all property mutations happen on the
/// main actor, preventing data races from concurrent relay callbacks.
@MainActor
@Observable
final class AppState {
    var keyManager: KeyManager
    var groups: [ChatGroup] = []
    let store: PersistenceStore
    /// Set by deep link handler; consumed by ContentView to navigate to join screen.
    var deepLinkInviteCode: String?
    let invitationTransport = InvitationTransport()
    var pendingInvitations: [PendingInvitation] = []
    var relayURLs: [URL] {
        didSet { Self.persistRelayURLs(relayURLs) }
    }

    // MARK: - On-Chain Integration

    var onChainService: OnChainService?
    var contractEndpoint: String {
        didSet { UserDefaults.standard.set(contractEndpoint, forKey: Self.contractEndpointKey) }
    }
    var contractID: String {
        didSet { UserDefaults.standard.set(contractID, forKey: Self.contractIDKey) }
    }
    var isContractConfigured: Bool { onChainService != nil }

    // MARK: - Relayer (Fee Decoupling)

    var relayerURL: String {
        didSet { UserDefaults.standard.set(relayerURL, forKey: Self.relayerURLKey) }
    }
    /// N-5: Auth token stored in Keychain instead of UserDefaults to prevent plaintext exposure.
    var relayerAuthToken: String {
        didSet { Self.saveToKeychain(relayerAuthToken, key: Self.relayerAuthTokenKey) }
    }
    var isRelayerConfigured: Bool {
        !relayerURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    // MARK: - Salt History (for offline recovery)

    /// Per-group salt history keyed by group ID, mapping epoch → salt.
    /// M-17: Persisted to UserDefaults so salt history survives app restarts.
    private var saltHistory: [String: [UInt64: Data]] = [:]

    private static let saltHistoryKey = "com.stellarmls.chat.saltHistory"

    private static let defaultRelays: [URL] = [
        URL(string: "wss://relay.damus.io")!,
        URL(string: "wss://nos.lol")!,
        URL(string: "wss://relay.nostr.band")!,
        URL(string: "wss://relay.snort.social")!,
        URL(string: "wss://nostr.wine")!,
    ]

    init() {
        self.keyManager = KeyManager()
        // N-15: Handle PersistenceStore init failure gracefully instead of crashing.
        // If first attempt fails (e.g., disk full), retry once; if both fail, create
        // an in-memory fallback so the app can still launch.
        if let persistedStore = try? PersistenceStore() {
            self.store = persistedStore
        } else if let retryStore = try? PersistenceStore() {
            self.store = retryStore
        } else {
            self.store = PersistenceStore.inMemory()
        }
        self.groups = store.loadGroups()
        self.relayURLs = Self.loadRelayURLs()
        self.contractEndpoint = UserDefaults.standard.string(forKey: Self.contractEndpointKey) ?? ""
        self.contractID = UserDefaults.standard.string(forKey: Self.contractIDKey) ?? ""
        self.relayerURL = UserDefaults.standard.string(forKey: Self.relayerURLKey) ?? ""
        // N-5: Load auth token from Keychain; migrate from UserDefaults if present
        if let legacyToken = UserDefaults.standard.string(forKey: Self.relayerAuthTokenKey), !legacyToken.isEmpty {
            Self.saveToKeychain(legacyToken, key: Self.relayerAuthTokenKey)
            UserDefaults.standard.removeObject(forKey: Self.relayerAuthTokenKey)
            self.relayerAuthToken = legacyToken
        } else {
            self.relayerAuthToken = Self.loadFromKeychain(key: Self.relayerAuthTokenKey) ?? ""
        }
        configureContractIfReady()

        // M-17: Load persisted salt history, then add current group salts
        saltHistory = Self.loadSaltHistory()
        for group in groups {
            storeSalt(groupID: group.id, epoch: group.epoch, salt: group.salt)
        }
    }

    func addGroup(_ group: ChatGroup) {
        groups.append(group)
        store.saveGroup(group)
    }

    /// Announce ourselves as a new member to the group over the Nostr transport.
    func announceMemberJoined(group: ChatGroup) async {
        do {
            let myLeaf = try keyManager.memberLeaf
            let announcement = SEPMemberJoined(member: myLeaf)
            let transport = NostrMessageTransport()
            await transport.connect(to: group.relayHints)
            try await transport.sendProtocolMessage(
                announcement,
                topic: group.topicTag,
                key: group.encryptionKey,
                keyManager: keyManager
            )
            await transport.disconnect()
        } catch {
            // Best-effort announcement — existing members will see us when we send a message
        }
    }

    func updateLastEventTimestamp(groupID: String, timestamp: Int64) {
        if let index = groups.firstIndex(where: { $0.id == groupID }) {
            groups[index].lastEventTimestamp = timestamp
            store.saveGroup(groups[index])
        }
    }

    func updateGroup(_ group: ChatGroup) {
        if let index = groups.firstIndex(where: { $0.id == group.id }) {
            groups[index] = group
        }
        store.saveGroup(group)
    }

    func removeGroup(id: String) {
        groups.removeAll { $0.id == id }
        store.deleteGroup(id: id)
    }

    func removePendingInvitation(id: String) {
        pendingInvitations.removeAll { $0.id == id }
    }

    /// Create a group with the local user as the first member, computing the initial commitment.
    func createGroup(name: String) throws -> (ChatGroup, String) {
        var groupIDBytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, 32, &groupIDBytes)
        let groupID = Data(groupIDBytes)

        var secretBytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, 32, &secretBytes)
        let groupSecret = Data(secretBytes)

        let groupIDHex = groupID.map { String(format: "%02x", $0) }.joined()

        let myLeaf = try keyManager.memberLeaf

        var group = ChatGroup(
            id: groupIDHex,
            name: name,
            groupSecret: groupSecret,
            createdAt: Date(),
            relayHints: relayURLs,
            members: [myLeaf],
            epoch: 0,
            salt: SEPCommitmentBuilder.generateSalt(),
            tier: .small
        )
        try group.recomputeCommitment()
        addGroup(group)

        let code = InviteCode(
            groupID: groupID,
            groupSecret: groupSecret,
            name: name,
            relayHints: relayURLs.map(\.absoluteString),
            members: group.members,
            epoch: group.epoch,
            salt: group.salt,
            commitment: group.commitment
        )
        return (group, code.encode())
    }

    // MARK: - On-Chain Operations

    /// Publish a newly created group's commitment on-chain.
    func publishGroupOnChain(_ group: ChatGroup) async throws {
        guard let service = onChainService else {
            throw ChatError.contractNotConfigured
        }

        let response = try await service.publishGroupCreation(
            groupIDData: group.groupIDData,
            members: group.members,
            blsSecretKey: keyManager.blsSecretKey,
            epoch: group.epoch,
            salt: group.salt,
            tier: group.tier,
            callerAddress: keyManager.stellarAccountID
        )

        if response.accepted {
            var updated = group
            updated.isPublishedOnChain = true
            updateGroup(updated)
        } else {
            throw ChatError.onChainPublishFailed(response.message ?? "Contract rejected submission")
        }
    }

    /// Publish a commitment update after membership change.
    func publishMemberUpdate(
        group: ChatGroup,
        oldMembers: [SEPGroupMemberLeaf],
        oldEpoch: UInt64,
        oldSalt: Data
    ) async throws {
        guard let service = onChainService else {
            throw ChatError.contractNotConfigured
        }

        let response = try await service.publishCommitmentUpdate(
            groupIDData: group.groupIDData,
            oldMembers: oldMembers,
            oldEpoch: oldEpoch,
            oldSalt: oldSalt,
            newMembers: group.members,
            newEpoch: group.epoch,
            newSalt: group.salt,
            blsSecretKey: keyManager.blsSecretKey,
            tier: group.tier
        )

        if response.accepted {
            var updated = group
            updated.isPublishedOnChain = true
            updateGroup(updated)
        } else {
            throw ChatError.onChainPublishFailed(response.message ?? "Contract rejected update")
        }
    }

    /// Verify a group's local state against its on-chain commitment.
    func verifyGroupOnChain(_ group: ChatGroup) async -> OnChainVerificationResult {
        guard let service = onChainService else {
            return .error("Contract not configured")
        }

        return await service.verifyCommitment(
            groupIDData: group.groupIDData,
            members: group.members,
            epoch: group.epoch,
            salt: group.salt,
            tier: group.tier
        )
    }

    /// Verify membership on-chain via the contract (generates proof and calls verify_membership).
    func verifyMembershipOnChain(_ group: ChatGroup) async throws -> Bool {
        guard let service = onChainService else {
            throw ChatError.contractNotConfigured
        }

        return try await service.verifyMembership(
            groupIDData: group.groupIDData,
            members: group.members,
            blsSecretKey: keyManager.blsSecretKey,
            epoch: group.epoch,
            salt: group.salt,
            tier: group.tier
        )
    }

    // MARK: - Group Deactivation

    /// M-18: Deactivation requires explicit confirmation since it is irreversible on-chain.
    /// The `confirmed` parameter must be `true` — callers should show a confirmation dialog first.
    func deactivateGroupOnChain(_ group: ChatGroup, confirmed: Bool = false) async throws {
        guard confirmed else {
            throw ChatError.verificationFailed("Deactivation requires explicit confirmation")
        }
        guard let service = onChainService else {
            throw ChatError.contractNotConfigured
        }

        let response = try await service.deactivateGroup(
            groupIDData: group.groupIDData,
            members: group.members,
            blsSecretKey: keyManager.blsSecretKey,
            epoch: group.epoch,
            salt: group.salt,
            tier: group.tier
        )

        if !response.accepted {
            throw ChatError.onChainPublishFailed(response.message ?? "Deactivation rejected")
        }
    }

    // MARK: - Salt Distribution

    /// Store a salt in the per-group history for offline recovery.
    private static let saltHistoryWindow = 64

    func storeSalt(groupID: String, epoch: UInt64, salt: Data) {
        if saltHistory[groupID] == nil {
            saltHistory[groupID] = [:]
        }
        saltHistory[groupID]?[epoch] = salt
        // Cap to last 64 epochs to prevent memory exhaustion
        if let history = saltHistory[groupID], history.count > Self.saltHistoryWindow {
            let sortedKeys = history.keys.sorted()
            let toRemove = sortedKeys.prefix(history.count - Self.saltHistoryWindow)
            for key in toRemove { saltHistory[groupID]?.removeValue(forKey: key) }
        }
        // M-17: Persist to disk
        Self.persistSaltHistory(saltHistory)
    }

    /// Retrieve a salt for a specific epoch from the local history.
    func getSalt(groupID: String, epoch: UInt64) -> Data? {
        saltHistory[groupID]?[epoch]
    }

    // MARK: - Salt History Persistence (M-17)

    private static func loadSaltHistory() -> [String: [UInt64: Data]] {
        guard let stored = UserDefaults.standard.data(forKey: saltHistoryKey),
              let decoded = try? JSONDecoder().decode([String: [String: Data]].self, from: stored)
        else { return [:] }
        // Convert String keys back to UInt64
        var result: [String: [UInt64: Data]] = [:]
        for (groupID, epochMap) in decoded {
            var converted: [UInt64: Data] = [:]
            for (epochStr, salt) in epochMap {
                if let epoch = UInt64(epochStr) { converted[epoch] = salt }
            }
            result[groupID] = converted
        }
        return result
    }

    private static func persistSaltHistory(_ history: [String: [UInt64: Data]]) {
        // Convert UInt64 keys to String for JSON encoding
        var encodable: [String: [String: Data]] = [:]
        for (groupID, epochMap) in history {
            var converted: [String: Data] = [:]
            for (epoch, salt) in epochMap { converted[String(epoch)] = salt }
            encodable[groupID] = converted
        }
        if let data = try? JSONEncoder().encode(encodable) {
            UserDefaults.standard.set(data, forKey: saltHistoryKey)
        }
    }

    /// Build a state update message for broadcasting after a membership change.
    func buildStateUpdate(
        group: ChatGroup,
        addedMembers: [SEPGroupMemberLeaf] = [],
        removedMemberKeys: [Data] = []
    ) -> SEPGroupStateUpdate {
        let attestation: SEPKeyAttestationPayload?
        if let att = try? keyManager.createAttestation() {
            attestation = SEPKeyAttestationPayload(
                blsPubkey: att.blsPubkey,
                ed25519Pubkey: att.ed25519Pubkey,
                signature: att.signature
            )
        } else {
            attestation = nil
        }

        return SEPGroupStateUpdate(
            epoch: group.epoch,
            salt: group.salt,
            addedMembers: addedMembers,
            removedMemberKeys: removedMemberKeys,
            commitment: group.commitment,
            senderAttestation: attestation
        )
    }

    /// Apply a received state update to a local group.
    func applyStateUpdate(_ update: SEPGroupStateUpdate, to groupID: String) {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        var group = groups[index]

        // Only apply if the update is newer
        guard update.epoch > group.epoch else { return }

        // Verify sender attestation BEFORE mutating state
        if let att = update.senderAttestation {
            let attestation = KeyAttestation(
                blsPubkey: att.blsPubkey,
                ed25519Pubkey: att.ed25519Pubkey,
                signature: att.signature
            )
            if !KeyManager.verifyAttestation(attestation) {
                SecurityLog.invalidAttestation(reason: "signature verification failed")
                return
            }
        }

        // Apply member changes
        for removed in update.removedMemberKeys {
            group.members.removeAll { $0.publicKeyCompressed == removed }
        }
        for added in update.addedMembers {
            if !group.members.contains(where: { $0.publicKeyCompressed == added.publicKeyCompressed }) {
                group.members.append(added)
            }
        }
        group.members.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }

        group.epoch = update.epoch
        group.salt = update.salt
        if let commitment = update.commitment {
            group.commitment = commitment
        }

        groups[index] = group
        store.saveGroup(group)
        storeSalt(groupID: groupID, epoch: update.epoch, salt: update.salt)
    }

    /// Apply a group rename received from the protocol channel.
    func applyGroupRenamed(_ renamed: SEPGroupRenamed, to groupID: String) {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        // ChatGroup.name is let — we need to create a new instance
        let old = groups[index]
        let updated = ChatGroup(
            id: old.id,
            name: renamed.name,
            groupSecret: old.groupSecret,
            createdAt: old.createdAt,
            relayHints: old.relayHints,
            members: old.members,
            epoch: old.epoch,
            salt: old.salt,
            commitment: old.commitment,
            tier: old.tier,
            isPublishedOnChain: old.isPublishedOnChain,
            lastEventTimestamp: old.lastEventTimestamp
        )
        groups[index] = updated
        store.saveGroup(updated)
    }

    /// Handle a member_joined announcement: add the joiner and broadcast updated state.
    func handleMemberJoined(
        _ joined: SEPMemberJoined,
        groupID: String,
        transport: NostrMessageTransport,
        keyManager: KeyManager
    ) async {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        var group = groups[index]

        // Skip if already a member
        guard !group.members.contains(where: { $0.publicKeyCompressed == joined.member.publicKeyCompressed }) else { return }

        // Add the joiner
        group.members.append(joined.member)
        group.members.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }
        group.epoch += 1
        group.salt = SEPCommitmentBuilder.generateSalt()
        try? group.recomputeCommitment()

        groups[index] = group
        store.saveGroup(group)
        storeSalt(groupID: groupID, epoch: group.epoch, salt: group.salt)

        // Broadcast state update so all members (including the joiner) converge
        let update = SEPGroupStateUpdate(
            epoch: group.epoch,
            salt: group.salt,
            addedMembers: [joined.member],
            commitment: group.commitment
        )
        try? await transport.sendProtocolMessage(
            update,
            topic: group.topicTag,
            key: group.encryptionKey,
            keyManager: keyManager
        )
    }

    // MARK: - Contract Configuration

    private static let contractEndpointKey = "com.stellarmls.chat.contractEndpoint"
    private static let contractIDKey = "com.stellarmls.chat.contractID"
    private static let relayerURLKey = "com.stellarmls.chat.relayerURL"
    private static let relayerAuthTokenKey = "com.stellarmls.chat.relayerAuthToken"

    func configureContract() {
        configureContractIfReady()
    }

    /// Known-good Soroban RPC endpoints (M-13). Users may configure custom endpoints,
    /// but a warning should be shown if the endpoint is not in this list.
    static let knownRPCEndpoints: [String] = [
        "https://soroban-testnet.stellar.org",
        "https://soroban.stellar.org",
        "https://rpc-futurenet.stellar.org",
    ]

    /// Check if a Soroban RPC endpoint URL is well-formed and uses HTTPS.
    static func isValidRPCEndpoint(_ urlString: String) -> Bool {
        guard let url = URL(string: urlString),
              let scheme = url.scheme,
              scheme == "https",
              url.host != nil
        else { return false }
        return true
    }

    private func configureContractIfReady() {
        let endpoint = contractEndpoint.trimmingCharacters(in: .whitespacesAndNewlines)
        let id = contractID.trimmingCharacters(in: .whitespacesAndNewlines)

        guard !endpoint.isEmpty,
              !id.isEmpty,
              Self.isValidRPCEndpoint(endpoint),
              let url = URL(string: endpoint)
        else {
            onChainService = nil
            return
        }

        if isRelayerConfigured, let relayerURL = URL(string: relayerURL.trimmingCharacters(in: .whitespacesAndNewlines)) {
            let token = relayerAuthToken.trimmingCharacters(in: .whitespacesAndNewlines)
            let config = SEPRelayerConfig(relayerURL: relayerURL, authToken: token.isEmpty ? nil : token)
            onChainService = OnChainService(contractID: id, relayerConfig: config)
        } else {
            onChainService = OnChainService(contractID: id, endpoint: url)
        }
    }

    // MARK: - Invitation Listener

    func startInboxListener() async {
        await invitationTransport.connect(to: relayURLs)

        invitationTransport.onInvitation = { [weak self] invitation in
            Task { @MainActor in
                guard let self else { return }
                // Dedup by event ID
                if !self.pendingInvitations.contains(where: { $0.id == invitation.id }) {
                    // Skip if already in this group
                    let groupIDHex = invitation.payload.groupID
                        .map { String(format: "%02x", $0) }.joined()
                    if !self.groups.contains(where: { $0.id == groupIDHex }) {
                        self.pendingInvitations.append(invitation)
                    }
                }
            }
        }

        invitationTransport.subscribeToInbox(
            inboxTag: keyManager.inboxTag,
            privateKey: keyManager.keyAgreementPrivateKey
        )
    }

    // MARK: - Relay Persistence

    private static let relayURLsKey = "com.stellarmls.chat.relayURLs"

    private static func loadRelayURLs() -> [URL] {
        guard let strings = UserDefaults.standard.stringArray(forKey: relayURLsKey),
              !strings.isEmpty
        else { return defaultRelays }
        return strings.compactMap(URL.init(string:))
    }

    private static func persistRelayURLs(_ urls: [URL]) {
        UserDefaults.standard.set(urls.map(\.absoluteString), forKey: relayURLsKey)
    }

    // MARK: - Relay Management

    func addRelay(urlString: String) -> Bool {
        guard let url = URL(string: urlString),
              let scheme = url.scheme,
              (scheme == "ws" || scheme == "wss"),
              !relayURLs.contains(url)
        else { return false }
        relayURLs.append(url)
        return true
    }

    func removeRelay(at offsets: IndexSet) {
        relayURLs.remove(atOffsets: offsets)
    }

    func moveRelay(from source: IndexSet, to destination: Int) {
        relayURLs.move(fromOffsets: source, toOffset: destination)
    }

    // MARK: - Keychain Helpers (N-5: secure credential storage)

    private static func loadFromKeychain(key: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecSuccess, let data = result as? Data {
            return String(data: data, encoding: .utf8)
        }
        return nil
    }

    private static func saveToKeychain(_ value: String, key: String) {
        let data = Data(value.utf8)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrAccount as String: key,
            kSecValueData as String: data,
        ]
        SecItemDelete(query as CFDictionary)
        SecItemAdd(query as CFDictionary, nil)
    }
}
