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
    var contactAliasStore: ContactAliasStore!
    /// Set by deep link handler; consumed by ContentView to navigate to join screen.
    var deepLinkInviteCode: String?
    /// Set after group creation; consumed by ContentView to navigate to the new chat.
    var navigateToGroupID: String?
    let invitationTransport = InvitationTransport()
    var pendingInvitations: [PendingInvitation] = []

    // MARK: - Calls

    let callManager = CallManager()
    private(set) var callKitProvider: CallKitProvider?

    // MARK: - Persistent Chat & Protocol Transport

    /// Single transport for ALL group communication (chat + protocol).
    /// Lives for the entire app session, independent of any chat screen.
    private let chatTransport = NostrMessageTransport()
    /// Chat messages keyed by group ID — always in memory, persisted to store.
    var chatMessages: [String: [ChatMessage]] = [:]
    /// Dedup set for chat message event IDs, keyed by group ID.
    private var seenMessageIDs: [String: Set<String>] = [:]
    /// Unread message count per group. Reset when the user opens the chat.
    var unreadCounts: [String: Int] = [:]
    /// The group ID currently being viewed, used to suppress unread increments.
    var activeGroupID: String?
    /// Tracks processed protocol event IDs to prevent replay (H-7).
    private var processedProtocolEventIDs: Set<String> = []
    /// Tracks (senderPubkey, epoch) pairs for salt request rate limiting (H-5).
    private var saltRequestsResponded: Set<String> = []
    private static let maxDedupSetSize = 10_000
    /// Pending incoming messages awaiting batch insertion into chatMessages.
    private var pendingIncomingMessages: [(msg: ChatMessage, groupID: String, event: NostrEvent)] = []
    /// Task that flushes pending messages in a single UI update.
    private var messageFlushTask: Task<Void, Never>?
    var relayURLs: [URL] {
        didSet { Self.persistRelayURLs(relayURLs) }
    }
    var blossomServerURLs: [URL] {
        didSet { Self.persistBlossomServerURLs(blossomServerURLs) }
    }

    /// Whether at least one relay is connected. Updated periodically.
    var isRelayConnected = true

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

    // MARK: - Default Group Capacity

    var defaultGroupTier: SEPTier {
        didSet { UserDefaults.standard.set(defaultGroupTier.rawValue, forKey: Self.defaultGroupTierKey) }
    }
    private static let defaultGroupTierKey = "com.stellarmls.chat.defaultGroupTier"

    // MARK: - TURN Server Configuration

    var turnEnabled: Bool {
        didSet { UserDefaults.standard.set(turnEnabled, forKey: Self.turnEnabledKey) }
    }
    var turnURLs: [String] {
        didSet { UserDefaults.standard.set(turnURLs, forKey: Self.turnURLsKey) }
    }
    var turnUsername: String {
        didSet { Self.saveToKeychain(turnUsername, key: Self.turnUsernameKey) }
    }
    var turnPassword: String {
        didSet { Self.saveToKeychain(turnPassword, key: Self.turnPasswordKey) }
    }
    private static let turnEnabledKey = "com.stellarmls.chat.turnEnabled"
    private static let turnURLsKey = "com.stellarmls.chat.turnURLs"
    private static let turnUsernameKey = "com.stellarmls.chat.turnUsername"
    private static let turnPasswordKey = "com.stellarmls.chat.turnPassword"

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
        do {
            self.keyManager = try KeyManager()
        } catch {
            fatalError("Failed to initialize identity keys: \(error.localizedDescription)")
        }
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
        self.contactAliasStore = ContactAliasStore(store: store)
        self.groups = store.loadGroups()
        self.relayURLs = Self.loadRelayURLs()
        self.blossomServerURLs = Self.loadBlossomServerURLs()
        self.contractEndpoint = UserDefaults.standard.string(forKey: Self.contractEndpointKey) ?? ""
        self.contractID = UserDefaults.standard.string(forKey: Self.contractIDKey) ?? ""
        self.defaultGroupTier = SEPTier(rawValue: UserDefaults.standard.integer(forKey: Self.defaultGroupTierKey)) ?? .large
        self.relayerURL = UserDefaults.standard.string(forKey: Self.relayerURLKey) ?? ""
        // N-5: Load auth token from Keychain; migrate from UserDefaults if present
        if let legacyToken = UserDefaults.standard.string(forKey: Self.relayerAuthTokenKey), !legacyToken.isEmpty {
            Self.saveToKeychain(legacyToken, key: Self.relayerAuthTokenKey)
            UserDefaults.standard.removeObject(forKey: Self.relayerAuthTokenKey)
            self.relayerAuthToken = legacyToken
        } else {
            self.relayerAuthToken = Self.loadFromKeychain(key: Self.relayerAuthTokenKey) ?? ""
        }
        // Load TURN server config
        self.turnEnabled = UserDefaults.standard.bool(forKey: Self.turnEnabledKey)
        self.turnURLs = UserDefaults.standard.stringArray(forKey: Self.turnURLsKey) ?? []
        self.turnUsername = Self.loadFromKeychain(key: Self.turnUsernameKey) ?? ""
        self.turnPassword = Self.loadFromKeychain(key: Self.turnPasswordKey) ?? ""
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
        setupCallSignalHandler()
        callKitProvider = CallKitProvider(callManager: callManager)
        callManager.callKit = callKitProvider
        syncTurnConfig()
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

    /// Announce ourselves as a new member via the persistent chatTransport.
    /// The creator's `SEPGroupStateUpdate` response will be handled by the
    /// existing `setupProtocolHandler` callback — no need to wait here.
    func announceMemberJoined(group: ChatGroup) async {
        do {
            let myLeaf = try keyManager.memberLeaf
            let announcement = SEPMemberJoined(member: myLeaf)

            // Send via the already-connected chatTransport
            try await chatTransport.sendProtocolMessage(
                announcement,
                topic: group.topicTag,
                key: group.encryptionKey,
                keyManager: keyManager
            )
            #if DEBUG
            print("[AppState] announceMemberJoined sent for group=\(group.id.prefix(8))")
            #endif
        } catch {
            #if DEBUG
            print("[AppState] announceMemberJoined failed: \(error)")
            #endif
        }
    }

    func updateLastEventTimestamp(groupID: String, timestamp: Int64) {
        if let index = groups.firstIndex(where: { $0.id == groupID }) {
            groups[index].lastEventTimestamp = timestamp
            store.saveGroupAsync(groups[index])
        }
    }

    func updateGroup(_ group: ChatGroup) {
        if let index = groups.firstIndex(where: { $0.id == group.id }) {
            groups[index] = group
        }
        store.saveGroup(group)
    }

    private static func insertIntoArray(_ messages: inout [ChatMessage], message: ChatMessage) {
        if let last = messages.last {
            let isInOrder =
                last.timestamp < message.timestamp ||
                (last.timestamp == message.timestamp && last.id < message.id)
            if isInOrder {
                messages.append(message)
            } else {
                var low = 0
                var high = messages.count
                while low < high {
                    let mid = (low + high) / 2
                    let existing = messages[mid]
                    let shouldInsertBefore =
                        existing.timestamp > message.timestamp ||
                        (existing.timestamp == message.timestamp && existing.id >= message.id)
                    if shouldInsertBefore {
                        high = mid
                    } else {
                        low = mid + 1
                    }
                }
                messages.insert(message, at: low)
            }
        } else {
            messages.append(message)
        }
    }

    private func insertMessage(_ message: ChatMessage, into groupID: String) {
        var messages = chatMessages[groupID, default: []]
        Self.insertIntoArray(&messages, message: message)
        chatMessages[groupID] = messages
    }

    /// Queue an incoming message for batch insertion. Multiple messages arriving
    /// within the same run-loop tick are coalesced into a single `chatMessages`
    /// update, preventing per-message SwiftUI re-renders.
    private func queueIncomingMessage(_ msg: ChatMessage, groupID: String, event: NostrEvent) {
        pendingIncomingMessages.append((msg: msg, groupID: groupID, event: event))
        guard messageFlushTask == nil else { return }
        messageFlushTask = Task { @MainActor in
            // Wait one frame so all concurrent main-actor tasks can queue their messages.
            // Task.yield() is unreliable (returns immediately when no peers are pending).
            try? await Task.sleep(for: .milliseconds(16))
            self.flushPendingMessages()
        }
    }

    private func flushPendingMessages() {
        let batch = pendingIncomingMessages
        pendingIncomingMessages.removeAll()
        messageFlushTask = nil
        guard !batch.isEmpty else { return }

        // Batch-insert all messages into a local copy, then assign once (single observation trigger)
        var localMessages = chatMessages
        for (msg, groupID, _) in batch {
            var arr = localMessages[groupID, default: []]
            Self.insertIntoArray(&arr, message: msg)
            localMessages[groupID] = arr
        }
        chatMessages = localMessages

        // Persistence, unread counts, timestamps — these don't trigger chat view re-renders
        var localUnread = unreadCounts
        for (msg, groupID, event) in batch {
            store.saveMessageAsync(msg)
            if !msg.isMine && activeGroupID != groupID {
                localUnread[groupID, default: 0] += 1
            }
            if let group = groups.first(where: { $0.id == groupID }),
               event.createdAt > group.lastEventTimestamp {
                updateLastEventTimestamp(groupID: groupID, timestamp: event.createdAt)
            }
            // Send delivery ACK for non-mine messages (fire-and-forget)
            if !msg.isMine, let group = groups.first(where: { $0.id == groupID }) {
                let ack = SEPMessageAck(eventID: event.id)
                Task {
                    try? await chatTransport.sendProtocolMessage(
                        ack, topic: group.topicTag, key: group.encryptionKey, keyManager: keyManager)
                }
            }
        }
        unreadCounts = localUnread
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
            tier: defaultGroupTier
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
            commitment: group.commitment,
            tierRawValue: group.tier.rawValue
        )
        return (group, code.encode())
    }

    // MARK: - On-Chain Operations

    /// Publish a newly created group's commitment on-chain.
    func publishGroupOnChain(_ group: ChatGroup) async throws {
        guard let service = onChainService else {
            #if DEBUG
            print("[OnChain] publishGroupOnChain skipped: service is nil")
            #endif
            throw ChatError.contractNotConfigured
        }

        #if DEBUG
        print("[OnChain] publishGroupOnChain start group=\(group.id.prefix(8)) epoch=\(group.epoch) members=\(group.members.count)")
        #endif

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
            #if DEBUG
            print("[OnChain] publishGroupOnChain accepted group=\(group.id.prefix(8))")
            #endif
        } else {
            #if DEBUG
            print("[OnChain] publishGroupOnChain rejected: \(response.message ?? "<no message>")")
            #endif
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
    /// Handles three cases:
    /// - update.epoch > local: straightforward apply (normal case)
    /// - update.epoch == local: epoch fork — deterministic merge to resolve conflict
    /// - update.epoch < local: stale update, ignored
    func applyStateUpdate(_ update: SEPGroupStateUpdate, to groupID: String) {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        var group = groups[index]

        // Stale update — ignore
        guard update.epoch >= group.epoch else { return }

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

        if update.epoch == group.epoch {
            // Same epoch + same salt = our own update echoed back from the relay — ignore.
            if update.salt == group.salt {
                #if DEBUG
                print("[AppState] Ignoring own echo at epoch=\(update.epoch) for group=\(groupID.prefix(8))")
                #endif
                return
            }

            // Epoch fork: two members made concurrent changes at the same epoch.
            // Deterministic merge: union members, lexicographic-smaller salt wins.
            let remoteSalt = update.salt
            let localSalt = group.salt

            // Merge: add remote's added members, remove remote's removed members
            for removed in update.removedMemberKeys {
                group.members.removeAll { $0.publicKeyCompressed == removed }
            }
            for added in update.addedMembers {
                if !group.members.contains(where: { $0.publicKeyCompressed == added.publicKeyCompressed }) {
                    group.members.append(added)
                }
            }
            group.members.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }

            // Deterministic salt selection: lexicographically smaller wins
            let useRemoteSalt = remoteSalt.lexicographicallyPrecedes(localSalt)
            group.epoch += 1
            group.salt = useRemoteSalt ? remoteSalt : localSalt
            try? group.recomputeCommitment()

            groups[index] = group
            store.saveGroup(group)
            storeSalt(groupID: groupID, epoch: group.epoch, salt: group.salt)
            subscribeGroup(group)

            // Broadcast the merged state so all members converge
            let mergedUpdate = buildStateUpdate(group: group)
            Task {
                try? await chatTransport.sendProtocolMessage(
                    mergedUpdate, topic: group.topicTag, key: group.encryptionKey, keyManager: keyManager)
            }
            #if DEBUG
            print("[AppState] Epoch fork resolved: merged to epoch=\(group.epoch) for group=\(groupID.prefix(8))")
            #endif
        } else {
            // Normal case: update.epoch > group.epoch
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
            subscribeGroup(group)
        }
    }

    /// Rotate the group key without membership changes. Provides forward secrecy against compromised keys.
    func rotateGroupKey(groupID: String) {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        var group = groups[index]

        let previousKey = group.encryptionKey

        group.epoch += 1
        group.salt = SEPCommitmentBuilder.generateSalt()
        try? group.recomputeCommitment()

        groups[index] = group
        store.saveGroup(group)
        storeSalt(groupID: groupID, epoch: group.epoch, salt: group.salt)
        subscribeGroup(group)

        // Broadcast to peers in the background — don't block the UI
        let update = buildStateUpdate(group: group)
        Task {
            try? await chatTransport.sendProtocolMessage(
                update, topic: group.topicTag, key: previousKey, keyManager: keyManager)
        }
    }

    /// Remove a member from a group, broadcast the state update, and optionally update on-chain.
    func removeMember(blsPubkey: Data, from groupID: String) {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        var group = groups[index]

        // Must be a current member
        guard group.members.contains(where: { $0.publicKeyCompressed == blsPubkey }) else { return }

        // Capture old key for broadcasting state update
        let previousKey = group.encryptionKey

        group.members.removeAll { $0.publicKeyCompressed == blsPubkey }
        group.epoch += 1
        group.salt = SEPCommitmentBuilder.generateSalt()
        try? group.recomputeCommitment()

        groups[index] = group
        store.saveGroup(group)
        storeSalt(groupID: groupID, epoch: group.epoch, salt: group.salt)
        chatTransport.currentMembers = group.members
        subscribeGroup(group)

        // Broadcast removal state update in the background — don't block the UI
        let update = buildStateUpdate(group: group, removedMemberKeys: [blsPubkey])
        Task {
            try? await chatTransport.sendProtocolMessage(
                update, topic: group.topicTag, key: previousKey, keyManager: keyManager)
        }
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

    /// Rename a group and broadcast the change to all members.
    func renameGroup(groupID: String, newName: String) {
        let trimmed = newName.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        let group = groups[index]

        // Apply locally (name is let, so reconstruct)
        applyGroupRenamed(SEPGroupRenamed(name: trimmed), to: groupID)

        // Broadcast to peers
        let renamed = SEPGroupRenamed(name: trimmed)
        Task {
            try? await chatTransport.sendProtocolMessage(
                renamed, topic: group.topicTag, key: group.encryptionKey, keyManager: keyManager)
        }
    }

    // MARK: - Persistent Chat & Protocol Transport

    /// Set up the chat message handler on the persistent transport (runs once at init).
    private func setupChatHandler() {
        chatTransport.onMessage = { [weak self] plaintext, event in
            guard let self else { return }
            Task { @MainActor in
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
                    timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
                    isMine: event.pubkey == self.keyManager.publicKeyHex
                )
                self.queueIncomingMessage(msg, groupID: groupID, event: event)
            }
        }

        chatTransport.onImageMessage = { [weak self] plaintext, media, event in
            guard let self else { return }
            Task { @MainActor in
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
                    timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
                    isMine: event.pubkey == self.keyManager.publicKeyHex,
                    mediaAttachment: media
                )
                self.queueIncomingMessage(msg, groupID: groupID, event: event)
            }
        }

        chatTransport.onError = { _ in
            // Transport errors are non-fatal for background operation
        }

        chatTransport.onOK = { [weak self] eventID, accepted in
            guard let self else { return }
            Task { @MainActor in
                // Find and update the message status
                for (groupID, messages) in self.chatMessages {
                    if let index = messages.firstIndex(where: { $0.id == eventID }) {
                        self.chatMessages[groupID]?[index].status = accepted ? .sent : .failed
                        break
                    }
                }
            }
        }
    }

    /// Send a chat message in a group.
    func sendMessage(text: String, groupID: String) async throws {
        guard let group = groups.first(where: { $0.id == groupID }) else { return }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        // Build the event synchronously so we have its deterministic ID for the local message.
        let blsPubkey = try keyManager.blsPublicKey
        let ts = Int64(Date().timeIntervalSince1970)
        let wrapper: [String: Any] = [
            "v": 2, "type": "chat", "text": trimmed,
            "senderBlsPubkey": blsPubkey.base64EncodedString(), "ts": ts
        ]
        let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
        let authenticatedText = String(data: wrapperData, encoding: .utf8)!
        let envelope = try GroupCrypto.encrypt(authenticatedText, key: group.encryptionKey)
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()
        let event = try NostrEvent.build(
            kind: 44114, tags: [["t", group.topicTag]], content: content, keyManager: keyManager
        )

        // Optimistic UI: show the message locally BEFORE waiting for relay publish.
        let msg = ChatMessage(
            id: event.id,
            groupID: groupID,
            senderPubkey: keyManager.publicKeyHex,
            text: trimmed,
            timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
            isMine: true,
            status: .sending
        )
        insertMessage(msg, into: groupID)
        seenMessageIDs[groupID, default: []].insert(msg.id)
        store.saveMessageAsync(msg)

        // Publish the same event to relays in the background — UI is already updated.
        let transport = chatTransport
        Task {
            do {
                try await transport.publishToRelays(event)
            } catch {
                // Mark as failed if all relays reject
                await MainActor.run {
                    if let idx = self.chatMessages[groupID]?.firstIndex(where: { $0.id == event.id }) {
                        self.chatMessages[groupID]?[idx].status = .failed
                    }
                }
            }
        }
    }

    /// Retry a failed message by re-encrypting and re-publishing.
    func retryMessage(groupID: String, messageID: String) {
        guard let group = groups.first(where: { $0.id == groupID }),
              let idx = chatMessages[groupID]?.firstIndex(where: { $0.id == messageID }),
              chatMessages[groupID]?[idx].status == .failed
        else { return }

        let text = chatMessages[groupID]![idx].text
        chatMessages[groupID]?[idx].status = .sending

        Task {
            do {
                let blsPubkey = try keyManager.blsPublicKey
                let ts = Int64(Date().timeIntervalSince1970)
                let wrapper: [String: Any] = [
                    "v": 2, "type": "chat", "text": text,
                    "senderBlsPubkey": blsPubkey.base64EncodedString(), "ts": ts
                ]
                let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
                let authenticatedText = String(data: wrapperData, encoding: .utf8)!
                let envelope = try GroupCrypto.encrypt(authenticatedText, key: group.encryptionKey)
                let envelopeData = try JSONEncoder().encode(envelope)
                let content = envelopeData.base64EncodedString()
                let event = try NostrEvent.build(
                    kind: 44114, tags: [["t", group.topicTag]], content: content, keyManager: keyManager
                )
                try await chatTransport.publishToRelays(event)
                await MainActor.run {
                    // Replace the message with updated ID and sent status
                    if let i = self.chatMessages[groupID]?.firstIndex(where: { $0.id == messageID }) {
                        let old = self.chatMessages[groupID]![i]
                        self.chatMessages[groupID]?[i] = ChatMessage(
                            id: event.id, groupID: old.groupID, senderPubkey: old.senderPubkey,
                            text: old.text, timestamp: old.timestamp, isMine: old.isMine, status: .sent
                        )
                    }
                }
            } catch {
                await MainActor.run {
                    if let i = self.chatMessages[groupID]?.firstIndex(where: { $0.id == messageID }) {
                        self.chatMessages[groupID]?[i].status = .failed
                    }
                }
            }
        }
    }

    /// Send an encrypted image in a group via Blossom.
    func sendImage(imageData: Data, groupID: String) async throws {
        guard let group = groups.first(where: { $0.id == groupID }) else { return }

        // Compress and extract metadata
        guard let compressed = MediaCrypto.compressImage(imageData) else { return }
        guard let dimensions = MediaCrypto.imageDimensions(compressed) else { return }
        let thumbData = MediaCrypto.generateThumbnail(compressed)

        // Encrypt image with a fresh per-file key
        let (encryptedBlob, fileKey) = try MediaCrypto.encryptMedia(compressed)

        // Encrypt thumbnail with the same file key
        let encryptedThumbnail: Data?
        if let thumbData {
            encryptedThumbnail = try MediaCrypto.encryptMedia(thumbData, key: fileKey)
        } else {
            encryptedThumbnail = nil
        }

        // Upload encrypted blob to Blossom
        let blobHash = try await BlossomClient.upload(encryptedBlob, servers: blossomServerURLs, keyManager: keyManager)

        // Build MediaAttachment
        let media = MediaAttachment(
            blobHash: blobHash,
            fileKey: fileKey,
            mimeType: "image/jpeg",
            width: dimensions.width,
            height: dimensions.height,
            size: encryptedBlob.count,
            blossomServers: blossomServerURLs.map(\.absoluteString),
            encryptedThumbnail: encryptedThumbnail,
            duration: nil
        )

        // Build v2 wrapper with type "image" and media dict
        let blsPubkey = try keyManager.blsPublicKey
        let ts = Int64(Date().timeIntervalSince1970)
        var mediaDict: [String: Any] = [
            "blobHash": media.blobHash,
            "fileKey": media.fileKey.base64EncodedString(),
            "mimeType": media.mimeType,
            "width": media.width,
            "height": media.height,
            "size": media.size,
            "blossomServers": media.blossomServers,
        ]
        if let encThumb = media.encryptedThumbnail {
            mediaDict["thumbnail"] = encThumb.base64EncodedString()
        }
        let wrapper: [String: Any] = [
            "v": 2,
            "type": "image",
            "text": "Sent an image",
            "senderBlsPubkey": blsPubkey.base64EncodedString(),
            "ts": ts,
            "media": mediaDict,
        ]
        let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
        let wrapperText = String(data: wrapperData, encoding: .utf8)!

        // Build the Nostr event locally
        let envelope = try GroupCrypto.encrypt(wrapperText, key: group.encryptionKey)
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()
        let event = try NostrEvent.build(
            kind: 44114, tags: [["t", group.topicTag]], content: content, keyManager: keyManager
        )

        // Optimistic UI: show locally before relay publish
        let msg = ChatMessage(
            id: event.id,
            groupID: groupID,
            senderPubkey: keyManager.publicKeyHex,
            text: "Sent an image",
            timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
            isMine: true,
            status: .sending,
            mediaAttachment: media
        )
        insertMessage(msg, into: groupID)
        seenMessageIDs[groupID, default: []].insert(msg.id)
        store.saveMessageAsync(msg)

        // Publish to relays in the background
        let transport = chatTransport
        Task {
            do {
                try await transport.publishToRelays(event)
            } catch {
                await MainActor.run {
                    if let idx = self.chatMessages[groupID]?.firstIndex(where: { $0.id == event.id }) {
                        self.chatMessages[groupID]?[idx].status = .failed
                    }
                }
            }
        }
    }

    func sendVideo(videoURL: URL, groupID: String) async throws {
        guard let group = groups.first(where: { $0.id == groupID }) else { return }

        // Compress video (or verify size)
        guard let videoData = try await MediaCrypto.compressVideo(videoURL) else {
            throw MediaCryptoError.encryptionFailed
        }

        // Extract metadata
        guard let meta = await MediaCrypto.videoMetadata(videoURL) else { return }
        let thumbData = await MediaCrypto.generateVideoThumbnail(videoURL)

        // Encrypt video with a fresh per-file key
        let (encryptedBlob, fileKey) = try MediaCrypto.encryptMedia(videoData)

        // Encrypt thumbnail with the same file key
        let encryptedThumbnail: Data?
        if let thumbData {
            encryptedThumbnail = try MediaCrypto.encryptMedia(thumbData, key: fileKey)
        } else {
            encryptedThumbnail = nil
        }

        // Upload encrypted blob to Blossom
        let blobHash = try await BlossomClient.upload(encryptedBlob, servers: blossomServerURLs, keyManager: keyManager)

        // Build MediaAttachment
        let media = MediaAttachment(
            blobHash: blobHash,
            fileKey: fileKey,
            mimeType: "video/mp4",
            width: meta.width,
            height: meta.height,
            size: encryptedBlob.count,
            blossomServers: blossomServerURLs.map(\.absoluteString),
            encryptedThumbnail: encryptedThumbnail,
            duration: meta.duration
        )

        // Build v2 wrapper with type "video" and media dict
        let blsPubkey = try keyManager.blsPublicKey
        let ts = Int64(Date().timeIntervalSince1970)
        var mediaDict: [String: Any] = [
            "blobHash": media.blobHash,
            "fileKey": media.fileKey.base64EncodedString(),
            "mimeType": media.mimeType,
            "width": media.width,
            "height": media.height,
            "size": media.size,
            "blossomServers": media.blossomServers,
            "duration": meta.duration,
        ]
        if let encThumb = media.encryptedThumbnail {
            mediaDict["thumbnail"] = encThumb.base64EncodedString()
        }
        let wrapper: [String: Any] = [
            "v": 2,
            "type": "video",
            "text": "Sent a video",
            "senderBlsPubkey": blsPubkey.base64EncodedString(),
            "ts": ts,
            "media": mediaDict,
        ]
        let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
        let wrapperText = String(data: wrapperData, encoding: .utf8)!

        // Build the Nostr event locally
        let envelope = try GroupCrypto.encrypt(wrapperText, key: group.encryptionKey)
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()
        let event = try NostrEvent.build(
            kind: 44114, tags: [["t", group.topicTag]], content: content, keyManager: keyManager
        )

        // Optimistic UI: show locally before relay publish
        let msg = ChatMessage(
            id: event.id,
            groupID: groupID,
            senderPubkey: keyManager.publicKeyHex,
            text: "Sent a video",
            timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
            isMine: true,
            status: .sending,
            mediaAttachment: media
        )
        insertMessage(msg, into: groupID)
        seenMessageIDs[groupID, default: []].insert(msg.id)
        store.saveMessageAsync(msg)

        // Publish to relays in the background
        let transport = chatTransport
        Task {
            do {
                try await transport.publishToRelays(event)
            } catch {
                await MainActor.run {
                    if let idx = self.chatMessages[groupID]?.firstIndex(where: { $0.id == event.id }) {
                        self.chatMessages[groupID]?[idx].status = .failed
                    }
                }
            }
        }
    }

    /// Send an encrypted voice message in a group via Blossom.
    func sendVoice(audioURL: URL, groupID: String) async throws {
        guard let group = groups.first(where: { $0.id == groupID }) else { return }

        let audioData = try Data(contentsOf: audioURL)
        guard audioData.count <= MediaCrypto.maxAudioBytes else { return }

        guard let audioDuration = await MediaCrypto.audioMetadata(audioURL) else { return }

        // Encrypt audio with a fresh per-file key
        let (encryptedBlob, fileKey) = try MediaCrypto.encryptMedia(audioData)

        // Upload encrypted blob to Blossom
        let blobHash = try await BlossomClient.upload(encryptedBlob, servers: blossomServerURLs, keyManager: keyManager)

        let media = MediaAttachment(
            blobHash: blobHash,
            fileKey: fileKey,
            mimeType: "audio/aac",
            width: 0,
            height: 0,
            size: encryptedBlob.count,
            blossomServers: blossomServerURLs.map(\.absoluteString),
            encryptedThumbnail: nil,
            duration: audioDuration
        )

        // Build v2 wrapper with type "audio"
        let blsPubkey = try keyManager.blsPublicKey
        let ts = Int64(Date().timeIntervalSince1970)
        let mediaDict: [String: Any] = [
            "blobHash": media.blobHash,
            "fileKey": media.fileKey.base64EncodedString(),
            "mimeType": media.mimeType,
            "width": 0,
            "height": 0,
            "size": media.size,
            "blossomServers": media.blossomServers,
            "duration": audioDuration,
        ]
        let wrapper: [String: Any] = [
            "v": 2,
            "type": "audio",
            "text": "Sent a voice message",
            "senderBlsPubkey": blsPubkey.base64EncodedString(),
            "ts": ts,
            "media": mediaDict,
        ]
        let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
        let wrapperText = String(data: wrapperData, encoding: .utf8)!

        let envelope = try GroupCrypto.encrypt(wrapperText, key: group.encryptionKey)
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()
        let event = try NostrEvent.build(
            kind: 44114, tags: [["t", group.topicTag]], content: content, keyManager: keyManager
        )

        // Optimistic UI
        let msg = ChatMessage(
            id: event.id,
            groupID: groupID,
            senderPubkey: keyManager.publicKeyHex,
            text: "Sent a voice message",
            timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
            isMine: true,
            status: .sending,
            mediaAttachment: media
        )
        insertMessage(msg, into: groupID)
        seenMessageIDs[groupID, default: []].insert(msg.id)
        store.saveMessageAsync(msg)

        // Publish to relays in the background
        let transport = chatTransport
        Task {
            do {
                try await transport.publishToRelays(event)
            } catch {
                await MainActor.run {
                    if let idx = self.chatMessages[groupID]?.firstIndex(where: { $0.id == event.id }) {
                        self.chatMessages[groupID]?[idx].status = .failed
                    }
                }
            }
        }

        // Clean up temp file
        try? FileManager.default.removeItem(at: audioURL)
    }

    // MARK: - Call Signaling

    private func setupCallSignalHandler() {
        chatTransport.onCallSignal = { [weak self] groupID, callDict, senderBlsPubkey, event in
            Task { @MainActor in
                guard let self else { return }
                // Configure signaling channel for incoming calls before handling
                self.callManager.sendSignal = { [weak self] callDict in
                    try await self?.sendCallSignal(callDict, groupID: groupID)
                }
                await self.callManager.handleSignal(callDict, senderBlsPubkey: senderBlsPubkey)
            }
        }
    }

    /// Send a call signaling message (offer/answer/ice/hangup) to the active group.
    func sendCallSignal(_ callDict: [String: Any], groupID: String) async throws {
        guard let group = groups.first(where: { $0.id == groupID }) else { return }
        let blsPubkey = try keyManager.blsPublicKey
        let ts = Int64(Date().timeIntervalSince1970)
        let wrapper: [String: Any] = [
            "v": 2,
            "type": "call",
            "text": "",
            "senderBlsPubkey": blsPubkey.base64EncodedString(),
            "ts": ts,
            "call": callDict,
        ]
        let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
        let wrapperText = String(data: wrapperData, encoding: .utf8)!
        try await chatTransport.sendWrapper(wrapperText, topic: group.topicTag, key: group.encryptionKey, keyManager: keyManager)
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
                guard let groupID = self.groups.first(where: { $0.topicTag == topicTag })?.id else {
                    return
                }
                #if DEBUG
                print("[AppState] Protocol msg type=\(msgType ?? "nil") group=\(groupID.prefix(8))")
                #endif

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
                    do {
                        let update = try decoder.decode(SEPGroupStateUpdate.self, from: data)
                        #if DEBUG
                        print("[AppState] Received state update epoch=\(update.epoch) for group=\(groupID.prefix(8))")
                        #endif
                        self.applyStateUpdate(update, to: groupID)
                        if let updated = self.groups.first(where: { $0.id == groupID }) {
                            self.chatTransport.currentMembers = updated.members
                            self.subscribeGroup( updated)
                        }
                    } catch {
                        #if DEBUG
                        print("[AppState] FAILED to decode state update: \(error) json=\(json.prefix(200))")
                        #endif
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
                case SEPMessageAck.messageType:
                    if let ack = try? decoder.decode(SEPMessageAck.self, from: data) {
                        // Update message status to .delivered if we sent it
                        if let idx = self.chatMessages[groupID]?.firstIndex(where: { $0.id == ack.eventID && $0.isMine }) {
                            self.chatMessages[groupID]?[idx].status = .delivered
                        }
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

    /// Reconnect to relays and resubscribe all groups (used by pull-to-refresh).
    func reconnectRelays() async {
        await chatTransport.disconnect()
        await connectAndSubscribeAllGroups()
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
        #if DEBUG
        let keyData = group.encryptionKey.withUnsafeBytes { Data($0) }
        print("[AppState] subscribeGroup id=\(group.id.prefix(8)) epoch=\(group.epoch) key=\(keyData.prefix(6).base64EncodedString()) salt=\(group.salt.prefix(6).base64EncodedString())")
        #endif
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

        // Resubscribe with the new encryption key after epoch change
        subscribeGroup(group)
    }

    // MARK: - TURN Configuration

    func syncTurnConfig() {
        callManager.turnEnabled = turnEnabled
        callManager.turnURLs = turnURLs
        callManager.turnUsername = turnUsername
        callManager.turnPassword = turnPassword
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
            #if DEBUG
            print("[OnChain] configuration invalid endpoint=\(endpoint) contractID=\(id) relayerURL=\(relayerURL)")
            #endif
            return
        }

        if isRelayerConfigured, let relayerURL = URL(string: relayerURL.trimmingCharacters(in: .whitespacesAndNewlines)) {
            let token = relayerAuthToken.trimmingCharacters(in: .whitespacesAndNewlines)
            let config = SEPRelayerConfig(relayerURL: relayerURL, authToken: token.isEmpty ? nil : token)
            onChainService = OnChainService(contractID: id, relayerConfig: config)
            #if DEBUG
            print("[OnChain] configured relayer transport rpc=\(endpoint) contractID=\(id) relayer=\(relayerURL.absoluteString)")
            #endif
        } else {
            onChainService = OnChainService(contractID: id, endpoint: url)
            #if DEBUG
            print("[OnChain] configured direct transport rpc=\(endpoint) contractID=\(id)")
            #endif
        }
    }

    // MARK: - Invitation Listener

    func startInboxListener() async {
        await invitationTransport.connect(to: relayURLs)
        #if DEBUG
        print("[Invite] startInboxListener inboxTag=\(keyManager.inboxTag) relays=\(relayURLs.map(\.absoluteString))")
        #endif

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

        // Periodically check relay connectivity
        Task {
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(5))
                let connected = await chatTransport.isAnyRelayConnected
                if isRelayConnected != connected {
                    isRelayConnected = connected
                }
            }
        }
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

    // MARK: - Blossom Server Persistence

    private static let blossomServerURLsKey = "com.stellarmls.chat.blossomServerURLs"
    private static let defaultBlossomServers = [URL(string: "https://nostr.download")!]

    private static func loadBlossomServerURLs() -> [URL] {
        guard let strings = UserDefaults.standard.stringArray(forKey: blossomServerURLsKey),
              !strings.isEmpty
        else { return defaultBlossomServers }
        return strings.compactMap(URL.init(string:))
    }

    private static func persistBlossomServerURLs(_ urls: [URL]) {
        UserDefaults.standard.set(urls.map(\.absoluteString), forKey: blossomServerURLsKey)
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
