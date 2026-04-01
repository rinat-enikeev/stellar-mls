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

    // MARK: - Persistent Chat & Protocol Transport

    /// Single transport for ALL group communication (chat + protocol).
    /// Lives for the entire app session, independent of any chat screen.
    private let chatTransport = NostrMessageTransport()
    /// Chat messages keyed by group ID — always in memory, persisted to store.
    var chatMessages: [String: [ChatMessage]] = [:]
    /// Dedup set for chat message event IDs, keyed by group ID.
    private var seenMessageIDs: [String: Set<String>] = [:]
    /// Tracks processed protocol event IDs to prevent replay (H-7).
    private var processedProtocolEventIDs: Set<String> = []
    /// Tracks (senderPubkey, epoch) pairs for salt request rate limiting (H-5).
    private var saltRequestsResponded: Set<String> = []
    private static let maxDedupSetSize = 10_000
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

        // Load persisted chat messages for all groups
        for group in groups {
            let msgs = store.loadMessages(groupID: group.id)
            chatMessages[group.id] = msgs
            seenMessageIDs[group.id] = Set(msgs.map(\.id))
        }

        // Set up persistent transport: handles both chat messages and protocol
        // messages for ALL groups, alive for the entire app session.
        chatTransport.currentMembers = groups.flatMap(\.members)
        setupChatHandler()
        setupProtocolHandler()
        Task { await connectAndSubscribeAllGroups() }
    }

    func addGroup(_ group: ChatGroup) {
        groups.append(group)
        store.saveGroup(group)
        chatMessages[group.id] = []
        seenMessageIDs[group.id] = []
        // Subscribe new group on persistent transport
        subscribeGroup( group)
    }

    /// Announce ourselves as a new member and wait for the creator's `SEPGroupStateUpdate`.
    /// Returns once the state update is received and applied, or after timeout.
    func announceMemberJoined(group: ChatGroup, timeout: TimeInterval = 30) async {
        do {
            let myLeaf = try keyManager.memberLeaf
            let myBlsPubkey = myLeaf.publicKeyCompressed
            let announcement = SEPMemberJoined(member: myLeaf)
            let transport = NostrMessageTransport()
            await transport.connect(to: group.relayHints)

            // Listen for the state update that includes us
            let confirmed = await withCheckedContinuation { (continuation: CheckedContinuation<Bool, Never>) in
                var resumed = false

                transport.onProtocolMessage = { [weak self] json, _ in
                    guard let self, !resumed else { return }
                    guard let data = json.data(using: .utf8),
                          let update = try? JSONDecoder().decode(SEPGroupStateUpdate.self, from: data),
                          update.addedMembers.contains(where: { $0.publicKeyCompressed == myBlsPubkey })
                    else { return }

                    // Apply the state update so we have the new epoch/salt/key
                    // when opening the chat.
                    self.applyStateUpdate(update, to: group.id)
                    resumed = true
                    continuation.resume(returning: true)
                }

                transport.subscribe(
                    topic: group.topicTag,
                    groupID: group.id,
                    key: group.encryptionKey
                )

                // Send announcement after subscribing so we don't miss the response
                Task {
                    try? await transport.sendProtocolMessage(
                        announcement,
                        topic: group.topicTag,
                        key: group.encryptionKey,
                        keyManager: self.keyManager
                    )
                }

                // Timeout fallback
                Task {
                    try? await Task.sleep(for: .seconds(timeout))
                    guard !resumed else { return }
                    resumed = true
                    continuation.resume(returning: false)
                }
            }

            await transport.disconnect()

            if !confirmed {
                // Timed out — group creator may be offline. We can still chat
                // since we added ourselves locally; creator will process our
                // announcement when they come online.
            }
        } catch {
            // Best-effort — existing members will see us when we send a message
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
        chatMessages.removeValue(forKey: id)
        seenMessageIDs.removeValue(forKey: id)
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

    // MARK: - Persistent Chat & Protocol Transport

    /// Set up the chat message handler on the persistent transport (runs once at init).
    private func setupChatHandler() {
        chatTransport.onMessage = { [weak self] plaintext, event in
            guard let self else { return }
            Task { @MainActor in
                // Find which group by topic tag
                let topicTag = event.tags.first(where: { $0.first == "t" }).flatMap { $0.dropFirst().first }
                guard let group = self.groups.first(where: { $0.topicTag == topicTag }) else { return }
                let groupID = group.id

                guard !(self.seenMessageIDs[groupID]?.contains(event.id) ?? false) else { return }
                self.seenMessageIDs[groupID, default: []].insert(event.id)

                let msg = ChatMessage(
                    id: event.id,
                    groupID: groupID,
                    senderPubkey: event.pubkey,
                    text: plaintext,
                    timestamp: Date(timeIntervalSince1970: TimeInterval(event.createdAt)),
                    isMine: event.pubkey == self.keyManager.publicKeyHex
                )
                self.chatMessages[groupID, default: []].append(msg)
                self.chatMessages[groupID]?.sort { $0.timestamp < $1.timestamp }
                self.store.saveMessage(msg)

                if event.createdAt > group.lastEventTimestamp {
                    self.updateLastEventTimestamp(groupID: groupID, timestamp: event.createdAt)
                }
            }
        }

        chatTransport.onError = { _ in
            // Transport errors are non-fatal for background operation
        }
    }

    /// Send a chat message in a group.
    func sendMessage(text: String, groupID: String) async throws {
        guard let group = groups.first(where: { $0.id == groupID }) else { return }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        try await chatTransport.send(
            text: trimmed,
            topic: group.topicTag,
            key: group.encryptionKey,
            keyManager: keyManager
        )

        let msg = ChatMessage(
            id: UUID().uuidString,
            groupID: groupID,
            senderPubkey: keyManager.publicKeyHex,
            text: trimmed,
            timestamp: Date(),
            isMine: true
        )
        chatMessages[groupID, default: []].append(msg)
        seenMessageIDs[groupID, default: []].insert(msg.id)
        store.saveMessage(msg)
    }

    /// Set up the protocol message handler (runs once at init).
    private func setupProtocolHandler() {
        chatTransport.onProtocolMessage = { [weak self] json, event in
            guard let self,
                  let data = json.data(using: .utf8) else { return }

            Task { @MainActor in
                // Replay protection (H-7)
                guard !self.processedProtocolEventIDs.contains(event.id) else { return }
                if self.processedProtocolEventIDs.count >= Self.maxDedupSetSize {
                    self.processedProtocolEventIDs.removeFirst()
                }
                self.processedProtocolEventIDs.insert(event.id)

                let decoder = JSONDecoder()
                let msgType = SEPProtocolMessage.parse(json)

                // Find which group this event belongs to (by matching topic tag)
                let topicTag = event.tags.first(where: { $0.first == "t" }).flatMap { $0.dropFirst().first }
                guard let groupID = self.groups.first(where: { $0.topicTag == topicTag })?.id else { return }

                switch msgType {
                case SEPMemberJoined.messageType:
                    if let joined = try? decoder.decode(SEPMemberJoined.self, from: data) {
                        await self.handleMemberJoined(joined, groupID: groupID)
                        // Resubscribe with new key after epoch change
                        if let updated = self.groups.first(where: { $0.id == groupID }) {
                            self.chatTransport.currentMembers = updated.members
                            self.subscribeGroup( updated)
                        }
                    }
                case SEPGroupStateUpdate.messageType:
                    if let update = try? decoder.decode(SEPGroupStateUpdate.self, from: data) {
                        self.applyStateUpdate(update, to: groupID)
                        if let updated = self.groups.first(where: { $0.id == groupID }) {
                            self.chatTransport.currentMembers = updated.members
                            self.subscribeGroup( updated)
                        }
                    }
                case SEPSaltRequest.messageType:
                    if let request = try? decoder.decode(SEPSaltRequest.self, from: data) {
                        let rateKey = "\(event.pubkey):\(request.epoch)"
                        guard !self.saltRequestsResponded.contains(rateKey) else { break }
                        self.saltRequestsResponded.insert(rateKey)
                        if let group = self.groups.first(where: { $0.id == groupID }),
                           let salt = self.getSalt(groupID: groupID, epoch: request.epoch) {
                            let response = SEPSaltResponse(epoch: request.epoch, salt: salt)
                            try? await self.chatTransport.sendProtocolMessage(
                                response, topic: group.topicTag, key: group.encryptionKey, keyManager: self.keyManager)
                        }
                    }
                case SEPSaltResponse.messageType:
                    if let response = try? decoder.decode(SEPSaltResponse.self, from: data) {
                        self.storeSalt(groupID: groupID, epoch: response.epoch, salt: response.salt)
                    }
                case SEPGroupRenamed.messageType:
                    if let renamed = try? decoder.decode(SEPGroupRenamed.self, from: data) {
                        self.applyGroupRenamed(renamed, to: groupID)
                    }
                default:
                    break
                }
            }
        }

        chatTransport.onError = { _ in
            // Protocol transport errors are non-fatal; chat transport shows errors to user
        }
    }

    /// Connect to relays and subscribe all persisted groups for chat + protocol messages.
    private func connectAndSubscribeAllGroups() async {
        await chatTransport.connect(to: relayURLs)
        for group in groups {
            subscribeGroup(group)
        }
    }

    /// Subscribe a single group on the persistent transport (chat + protocol).
    private func subscribeGroup(_ group: ChatGroup) {
        chatTransport.currentMembers = groups.flatMap(\.members)
        chatTransport.subscribe(
            topic: group.topicTag,
            groupID: group.id,
            key: group.encryptionKey,
            sinceTimestamp: group.lastEventTimestamp > 0 ? group.lastEventTimestamp : nil
        )
    }

    /// Handle a member_joined announcement: add the joiner and broadcast updated state.
    private func handleMemberJoined(_ joined: SEPMemberJoined, groupID: String) async {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        var group = groups[index]

        // Skip if already a member
        guard !group.members.contains(where: { $0.publicKeyCompressed == joined.member.publicKeyCompressed }) else { return }

        // Capture old encryption key BEFORE bumping epoch/salt.
        // The state update must be encrypted with the current key so all
        // existing members (including the joiner) can decrypt it.
        let previousKey = group.encryptionKey

        // Add the joiner
        group.members.append(joined.member)
        group.members.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }
        group.epoch += 1
        group.salt = SEPCommitmentBuilder.generateSalt()
        try? group.recomputeCommitment()

        groups[index] = group
        store.saveGroup(group)
        storeSalt(groupID: groupID, epoch: group.epoch, salt: group.salt)

        // Sync transport member list so BLS authentication accepts the new member
        chatTransport.currentMembers = group.members

        // Broadcast state update so all members (including the joiner) converge.
        // Encrypted with the PREVIOUS key so everyone can read it.
        let update = SEPGroupStateUpdate(
            epoch: group.epoch,
            salt: group.salt,
            addedMembers: [joined.member],
            commitment: group.commitment
        )
        try? await chatTransport.sendProtocolMessage(
            update,
            topic: group.topicTag,
            key: previousKey,
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
