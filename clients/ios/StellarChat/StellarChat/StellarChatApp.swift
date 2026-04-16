import CryptoKit
import SwiftUI
import SwiftMLS
import UserNotifications

// MARK: - Blockchain-First Epoch Authority Types

/// Result of attempting an epoch transition through the chain-confirmed helper.
enum EpochTransitionResult: Equatable {
    case success
    case chainRejected(reason: String)
    case chainUnavailable
    case chainMismatch(localEpoch: UInt64, chainEpoch: UInt64)
    case syncRequired
    case bundlesMissing
}

/// The kind of epoch transition being performed.
enum EpochTransitionKind {
    case memberAdd(member: SEPGroupMemberLeaf)
    case memberRemove(blsPubkey: Data)
    case keyRotation
}

/// Per-group pending transition state. While awaiting chain confirmation,
/// no additional epoch-changing actions are permitted for the group.
enum PendingTransitionState: Equatable {
    case idle
    case awaitingChainConfirmation(targetEpoch: UInt64)
}

/// AppDelegate handles APNs registration callbacks and notification tap handling.
class StellarChatAppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    weak var pushManager: PushNotificationManager?
    weak var appState: AppState?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        return true
    }

    func application(
        _ application: UIApplication,
        didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data
    ) {
        pushManager?.setAPNsToken(deviceToken)
        Task { @MainActor in
            // Trigger registration after receiving token
            NotificationCenter.default.post(name: .apnsTokenReceived, object: nil)
        }
    }

    func application(
        _ application: UIApplication,
        didFailToRegisterForRemoteNotificationsWithError error: Error
    ) {
        #if DEBUG
        print("[Push] Failed to register for remote notifications: \(error)")
        #endif
    }

    // MARK: - UNUserNotificationCenterDelegate

    /// Handle notification tap — navigate to the specific chat.
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let userInfo = response.notification.request.content.userInfo
        if let groupID = userInfo["groupID"] as? String {
            Task { @MainActor in
                appState?.navigateToGroupID = groupID
            }
        }
        completionHandler()
    }

    /// Suppress all notifications when the app is in the foreground.
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([])
    }
}

extension Notification.Name {
    static let apnsTokenReceived = Notification.Name("apnsTokenReceived")
}

@main
struct StellarChatApp: App {
    @UIApplicationDelegateAdaptor(StellarChatAppDelegate.self) var appDelegate
    @State private var appState = AppState()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(appState)
                .task {
                    await appState.startInboxListener()
                    appDelegate.pushManager = appState.pushManager
                    appDelegate.appState = appState
                    appState.configurePushRelay()
                }
                .onOpenURL { url in
                    // Handle https://onym.chat/join?code=<base64> (Universal Link)
                    // and legacy onym://join?code=<base64> / stellarchat://join?code=<base64>
                    let isUniversalLink = url.scheme == "https" && url.host == "onym.chat" && url.path.hasPrefix("/join")
                    let isCustomScheme = (url.scheme == "stellarchat" || url.scheme == "onym") && url.host == "join"
                    if isUniversalLink || isCustomScheme,
                       let code = URLComponents(url: url, resolvingAgainstBaseURL: false)?
                        .queryItems?.first(where: { $0.name == "code" })?.value {
                        appState.deepLinkInviteCode = code
                    }
                }
                .onChange(of: scenePhase) { _, newPhase in
                    if newPhase == .active {
                        // Clear badge when app comes to foreground
                        UNUserNotificationCenter.current().setBadgeCount(0)
                        UserDefaults(suiteName: "group.chat.onym.ios")?.set(0, forKey: "pn_badge_count")
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
    /// Group IDs that the user has declined — persisted so duplicates are suppressed across restarts.
    private var declinedGroupIDs: Set<String> {
        didSet { UserDefaults.standard.set(Array(declinedGroupIDs), forKey: Self.declinedGroupIDsKey) }
    }
    private static let declinedGroupIDsKey = "chat.onym.ios.declinedGroupIDs"
    let pushManager = PushNotificationManager()

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
    /// Per-group pending epoch transition state (for UI status).
    private var pendingTransitions: [String: PendingTransitionState] = [:]
    /// Per-group chain publish tasks — stored so they can be cancelled when a newer transition supersedes.
    private var chainPublishTasks: [String: Task<Void, Never>] = [:]
    /// Last successfully published on-chain state per group — used as proof baseline for the next chain publish.
    private var chainBaseline: [String: (members: [SEPGroupMemberLeaf], epoch: UInt64, salt: Data)] = [:]
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
    private static let defaultGroupTierKey = "chat.onym.ios.defaultGroupTier"

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
    private static let turnEnabledKey = "chat.onym.ios.turnEnabled"
    private static let turnURLsKey = "chat.onym.ios.turnURLs"
    private static let turnUsernameKey = "chat.onym.ios.turnUsername"
    private static let turnPasswordKey = "chat.onym.ios.turnPassword"

    // MARK: - Salt History (for offline recovery)

    /// Per-group salt history keyed by group ID, mapping epoch → salt.
    /// M-17: Persisted to UserDefaults so salt history survives app restarts.
    private var saltHistory: [String: [UInt64: Data]] = [:]

    /// Per-group epoch snapshots for epoch branching (git-like epoch switching).
    /// Captures members + salt + groupSecret at each epoch transition.
    var epochSnapshots: [String: [UInt64: EpochSnapshot]] = [:]
    private static let epochSnapshotWindow = 64

    /// In-memory transport bundle directory: groupID → (blsPubkeyHex → bundle).
    /// Used for secure per-member inbox rekey during removals.
    private var transportBundles: [String: [String: SEPMemberTransportBundle]] = [:]

    private static let saltHistoryKey = "chat.onym.ios.saltHistory"

    private static let defaultRelays: [URL] = {
        var relays: [URL] = []
        if let url = URL(string: RelayerDefaults.defaultNostrRelay), !RelayerDefaults.defaultNostrRelay.isEmpty {
            relays.append(url)
        }
        relays.append(contentsOf: [
            URL(string: "wss://relay.damus.io")!,
            URL(string: "wss://nos.lol")!,
            URL(string: "wss://relay.nostr.band")!,
            URL(string: "wss://relay.snort.social")!,
            URL(string: "wss://nostr.wine")!,
        ])
        return relays
    }()

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
        self.contractEndpoint = UserDefaults.standard.string(forKey: Self.contractEndpointKey)
            ?? RelayerDefaults.contractEndpoint
        self.contractID = UserDefaults.standard.string(forKey: Self.contractIDKey)
            ?? RelayerDefaults.contractID
        let savedTierRaw = UserDefaults.standard.object(forKey: Self.defaultGroupTierKey) as? Int
        self.defaultGroupTier = savedTierRaw.flatMap { SEPTier(rawValue: $0) } ?? .medium
        self.relayerURL = UserDefaults.standard.string(forKey: Self.relayerURLKey)
            ?? RelayerDefaults.relayerURL
        // N-5: Load auth token from Keychain; migrate from UserDefaults if present
        if let legacyToken = UserDefaults.standard.string(forKey: Self.relayerAuthTokenKey), !legacyToken.isEmpty {
            Self.saveToKeychain(legacyToken, key: Self.relayerAuthTokenKey)
            UserDefaults.standard.removeObject(forKey: Self.relayerAuthTokenKey)
            self.relayerAuthToken = legacyToken
        } else {
            self.relayerAuthToken = Self.loadFromKeychain(key: Self.relayerAuthTokenKey)
                ?? RelayerDefaults.relayerAuthToken
        }
        // Load declined invitation group IDs
        self.declinedGroupIDs = Set(UserDefaults.standard.stringArray(forKey: Self.declinedGroupIDsKey) ?? [])
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

        // Load epoch snapshots for epoch branching
        for group in groups {
            epochSnapshots[group.id] = store.loadEpochSnapshots(groupID: group.id)
        }

        // Load persisted chat messages and transport bundles for all groups
        for group in groups {
            let msgs = store.loadMessages(groupID: group.id)
            chatMessages[group.id] = msgs
            seenMessageIDs[group.id] = Set(msgs.map(\.id))
            transportBundles[group.id] = store.loadTransportBundles(groupID: group.id)
        }

        // Store local BLS pubkey for NSE self-message filtering (must happen before PNs arrive)
        if let blsPub = try? keyManager.blsPublicKey {
            pushManager.subscriptionStore.setLocalBlsPubkey(blsPub.base64EncodedString())
        }

        // Import messages received via push notifications while app was backgrounded
        importPendingPushMessages()

        // Clean up expired pending rekeys (>24h) on startup
        let allPending = store.loadAllPendingRekeys()
        let expiryInterval: TimeInterval = 24 * 60 * 60
        for record in allPending where Date().timeIntervalSince(record.createdAt) > expiryInterval {
            store.deletePendingRekey(groupID: record.groupID, epoch: record.epoch)
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
            let myBundle = try? keyManager.createTransportBundle()
            let announcement = SEPMemberJoined(member: myLeaf, transportBundle: myBundle)
            // Persist own bundle for this group
            ensureOwnTransportBundle(groupID: group.id)

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

    func markGroupPublished(groupID: String) {
        if let index = groups.firstIndex(where: { $0.id == groupID }) {
            groups[index].isPublishedOnChain = true
            store.saveGroup(groups[index])
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

        // Sort batch by timestamp so messages within the batch are ordered correctly,
        // then append to existing list. Appending (arrival order) prevents messages
        // received after an epoch transition from jumping into the middle of the chat.
        let sortedBatch = batch.sorted {
            $0.msg.timestamp != $1.msg.timestamp
                ? $0.msg.timestamp < $1.msg.timestamp
                : $0.msg.id < $1.msg.id
        }
        var localMessages = chatMessages
        for (msg, groupID, _) in sortedBatch {
            var arr = localMessages[groupID, default: []]
            arr.append(msg)
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

    func declinePendingInvitation(id: String) {
        if let invitation = pendingInvitations.first(where: { $0.id == id }) {
            let groupIDHex = invitation.payload.groupID
                .map { String(format: "%02x", $0) }.joined()
            declinedGroupIDs.insert(groupIDHex)
        }
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
        captureEpochSnapshot(group: group, changeDescription: "Group created")
        ensureOwnTransportBundle(groupID: groupIDHex)

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
            // Store on-chain baseline for future epoch transitions
            chainBaseline[group.id] = (members: group.members, epoch: group.epoch, salt: group.salt)
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

        if !response.accepted {
            throw ChatError.onChainPublishFailed(response.message ?? "Contract rejected update")
        }
    }

    private func publishMemberUpdateIfNeeded(oldGroup: ChatGroup, newGroup: ChatGroup) async throws {
        guard oldGroup.isPublishedOnChain else { return }
        try await publishMemberUpdate(
            group: newGroup,
            oldMembers: oldGroup.members,
            oldEpoch: oldGroup.epoch,
            oldSalt: oldGroup.salt
        )
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

    // MARK: - Blockchain-First Epoch Transition

    /// The single entry point for all epoch-advancing operations on published groups.
    /// Computes candidate next state, submits to chain, confirms, then persists locally
    /// and broadcasts. For unpublished groups, applies the transition locally without
    /// chain interaction.
    ///
    /// - Parameters:
    ///   - groupID: The group to transition.
    ///   - kind: What kind of transition (memberAdd, memberRemove, keyRotation).
    ///   - previousKey: The encryption key BEFORE the transition, used to encrypt the
    ///     broadcast so current members can decrypt it.
    /// - Returns: The result of the transition attempt.
    func performEpochTransition(
        groupID: String,
        kind: EpochTransitionKind,
        previousKey: SymmetricKey
    ) async -> EpochTransitionResult {
        #if DEBUG
        print("[EpochTransition] START kind=\(kind) group=\(groupID.prefix(8))")
        #endif
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else {
            return .chainRejected(reason: "Group not found")
        }
        let currentGroup = groups[index]

        // Note: we no longer block transitions when a chain publish is pending.
        // Local transitions proceed immediately; chain publishes are cancelled and
        // restarted with the latest state (coalesced).

        // Compute candidate next state
        var candidate = currentGroup
        var addedMembers: [SEPGroupMemberLeaf] = []
        var removedMemberKeys: [Data] = []

        switch kind {
        case .memberAdd(let member):
            guard !candidate.members.contains(where: { $0.publicKeyCompressed == member.publicKeyCompressed }) else {
                // Already a member — ensure system message exists (dedup-safe)
                let name = memberDisplayName(blsPubkey: member.publicKeyCompressed)
                insertSystemMessage(groupID: groupID, text: "\(name) joined the group", event: "member-add", epoch: currentGroup.epoch)
                return .success
            }
            guard candidate.members.count < candidate.tier.maxMembers else {
                return .chainRejected(reason: "Group is full")
            }
            candidate.members.append(member)
            candidate.members.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }
            candidate.epoch += 1
            candidate.salt = SEPCommitmentBuilder.deriveSalt(previousSalt: candidate.salt, memberKey: member.publicKeyCompressed)
            addedMembers = [member]

        case .memberRemove(let blsPubkey):
            guard candidate.members.contains(where: { $0.publicKeyCompressed == blsPubkey }) else {
                return .chainRejected(reason: "Not a member")
            }
            candidate.members.removeAll { $0.publicKeyCompressed == blsPubkey }
            candidate.epoch += 1
            candidate.salt = SEPCommitmentBuilder.generateSalt()
            removedMemberKeys = [blsPubkey]

        case .keyRotation:
            candidate.epoch += 1
            candidate.salt = SEPCommitmentBuilder.generateSalt()
        }

        do {
            try candidate.recomputeCommitment()
        } catch {
            return .chainRejected(reason: "Failed to recompute commitment: \(error.localizedDescription)")
        }

        // Determine secure rekey BEFORE chain publish so we publish the final state
        let isRemoval = !removedMemberKeys.isEmpty
        let useSecureRekey = isRemoval && allMembersHaveBundles(groupID: groupID, members: candidate.members)
        #if DEBUG
        let bundleKeys = transportBundles[groupID]?.keys.map { $0.prefix(8) } ?? []
        let memberKeys = candidate.members.map { $0.publicKeyCompressed.map { String(format: "%02x", $0) }.joined().prefix(8) }
        print("[AppState] Removal flow: useSecureRekey=\(useSecureRekey) isRemoval=\(isRemoval) bundles=\(bundleKeys) members=\(memberKeys)")
        #endif

        // If secure rekey, apply fresh groupSecret + salt now so the chain publish
        // uses the final commitment (one publish instead of two).
        // Save pre-rekey candidate for broadcasting on the old channel later.
        if useSecureRekey {
            var freshSecret = [UInt8](repeating: 0, count: 32)
            _ = SecRandomCopyBytes(kSecRandomDefault, 32, &freshSecret)
            let freshSalt = SEPCommitmentBuilder.generateSalt()
            var rekeyCandidate = ChatGroup(
                id: candidate.id,
                name: candidate.name,
                groupSecret: Data(freshSecret),
                createdAt: candidate.createdAt,
                relayHints: candidate.relayHints,
                members: candidate.members,
                epoch: candidate.epoch,
                salt: freshSalt,
                commitment: candidate.commitment,
                tier: candidate.tier,
                isPublishedOnChain: candidate.isPublishedOnChain,
                lastEventTimestamp: candidate.lastEventTimestamp
            )
            rekeyCandidate.pinnedEpoch = candidate.pinnedEpoch
            rekeyCandidate.forkedFromGroupID = candidate.forkedFromGroupID
            rekeyCandidate.forkedAtEpoch = candidate.forkedAtEpoch
            rekeyCandidate.removedByPubkeyHex = candidate.removedByPubkeyHex
            rekeyCandidate.pushNotificationsEnabled = candidate.pushNotificationsEnabled
            try? rekeyCandidate.recomputeCommitment()
            candidate = rekeyCandidate
        }

        // Capture epoch snapshot BEFORE persisting the new state
        let snapshotDescription: String
        switch kind {
        case .memberAdd(let member):
            let name = memberDisplayName(blsPubkey: member.publicKeyCompressed)
            snapshotDescription = "\(name) joined the group"
        case .memberRemove(let blsPubkey):
            let name = memberDisplayName(blsPubkey: blsPubkey)
            snapshotDescription = "\(name) was removed from the group"
        case .keyRotation:
            snapshotDescription = "Encryption key was rotated"
        }
        captureEpochSnapshot(group: currentGroup, changeDescription: snapshotDescription)

        // --- Persist locally FIRST (optimistic), then sync chain in background ---
        candidate.isPublishedOnChain = currentGroup.isPublishedOnChain
        groups[index] = candidate
        store.saveGroup(candidate)
        storeSalt(groupID: groupID, epoch: candidate.epoch, salt: candidate.salt)

        // --- Async chain sync (non-blocking, coalesced) ---
        // Launch BEFORE the Nostr broadcast so the chain publish isn't gated
        // by relay round-trips. Cancel any in-flight publish and restart with
        // the latest state.
        #if DEBUG
        print("[EpochTransition] Chain sync check: isPublishedOnChain=\(currentGroup.isPublishedOnChain) onChainService=\(onChainService != nil) group=\(groupID.prefix(8)) newEpoch=\(candidate.epoch)")
        #endif
        if currentGroup.isPublishedOnChain, onChainService != nil {
            chainPublishTasks[groupID]?.cancel()
            let targetEpoch = candidate.epoch
            pendingTransitions[groupID] = .awaitingChainConfirmation(targetEpoch: targetEpoch)
            let chainCandidate = candidate
            let baseline = chainBaseline[groupID]
                ?? (members: currentGroup.members, epoch: currentGroup.epoch, salt: currentGroup.salt)
            #if DEBUG
            print("[EpochTransition] Chain sync STARTING: baselineEpoch=\(baseline.epoch) targetEpoch=\(targetEpoch) baselineMembers=\(baseline.members.count) candidateMembers=\(chainCandidate.members.count) hasStoredBaseline=\(chainBaseline[groupID] != nil)")
            #endif
            chainPublishTasks[groupID] = Task {
                #if DEBUG
                print("[EpochTransition] Chain sync Task EXECUTING for epoch=\(targetEpoch) group=\(groupID.prefix(8))")
                #endif
                defer {
                    if case .awaitingChainConfirmation(let t) = self.pendingTransitions[groupID], t == targetEpoch {
                        self.pendingTransitions[groupID] = .idle
                    }
                    if self.chainPublishTasks[groupID]?.isCancelled != false {
                        self.chainPublishTasks[groupID] = nil
                    }
                }
                do {
                    try await self.publishMemberUpdate(
                        group: chainCandidate,
                        oldMembers: baseline.members,
                        oldEpoch: baseline.epoch,
                        oldSalt: baseline.salt
                    )
                    self.chainBaseline[groupID] = (
                        members: chainCandidate.members,
                        epoch: chainCandidate.epoch,
                        salt: chainCandidate.salt
                    )
                    #if DEBUG
                    print("[EpochTransition] Chain sync SUCCEEDED epoch=\(chainCandidate.epoch) group=\(groupID.prefix(8))")
                    #endif
                } catch is CancellationError {
                    #if DEBUG
                    print("[EpochTransition] Chain sync CANCELLED (superseded) epoch=\(chainCandidate.epoch) group=\(groupID.prefix(8))")
                    #endif
                } catch {
                    #if DEBUG
                    print("[EpochTransition] Chain sync FAILED epoch=\(chainCandidate.epoch) baselineEpoch=\(baseline.epoch) group=\(groupID.prefix(8)) error=\(error)")
                    #endif
                }
            }
        } else {
            #if DEBUG
            print("[EpochTransition] Chain sync SKIPPED: isPublishedOnChain=\(currentGroup.isPublishedOnChain) onChainService=\(onChainService != nil) group=\(groupID.prefix(8))")
            #endif
        }

        // Sync transport and ensure subscription uses the latest key.
        // This must happen BEFORE the secure rekey / legacy blocks so A can
        // at least communicate on the current state even if the blocks fail.
        chatTransport.currentMembers = groups.flatMap(\.members)
        if useSecureRekey {
            // Secure rekey: unsubscribe old topic now, subscribe new topic
            #if !DEBUG
            chatTransport.unsubscribe(topic: currentGroup.topicTag)
            #endif
            subscribeGroup(candidate)
        } else {
            subscribeGroup(candidate)
        }

        if useSecureRekey {
            #if DEBUG
            print("[Rekey] START groupID=\(groupID.prefix(8)) epoch=\(candidate.epoch) members=\(candidate.members.count) secure=true")
            #endif

            // 1. Broadcast SEPRemovalNotice on old channel (no secrets)
            let notice = SEPRemovalNotice(
                groupID: candidate.groupIDData,
                epoch: candidate.epoch,
                removedMemberKeys: removedMemberKeys,
                commitment: candidate.commitment
            )
            // Fire-and-forget: don't block rekey envelope delivery
            let noticeTopic = currentGroup.topicTag
            let noticeKey = previousKey
            Task { [weak self] in
                guard let self else { return }
                do {
                    try await self.chatTransport.sendProtocolMessage(
                        notice, topic: noticeTopic, key: noticeKey, keyManager: self.keyManager
                    )
                } catch {
                    #if DEBUG
                    print("[Rekey] NOTICE_FAILED groupID=\(groupID.prefix(8)) epoch=\(candidate.epoch) error=\(error)")
                    #endif
                }
            }
            #if DEBUG
            print("[Rekey] NOTICE_SENT groupID=\(groupID.prefix(8)) epoch=\(candidate.epoch) (fire-and-forget)")
            #endif

            // 2. Build SEPRekeyEnvelope with fresh secrets
            let myBundle = try! keyManager.createTransportBundle()
            let memberBundles = candidate.members.compactMap { member -> SEPMemberTransportBundle? in
                let hex = member.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
                return transportBundles[groupID]?[hex]
            }
            let envelope = SEPRekeyEnvelope(
                groupID: candidate.groupIDData,
                epoch: candidate.epoch,
                groupSecret: candidate.groupSecret,
                salt: candidate.salt,
                commitment: candidate.commitment,
                memberBundles: memberBundles,
                removedMemberKeys: removedMemberKeys,
                relayHints: candidate.relayHints.map(\.absoluteString),
                senderBundle: myBundle
            )

            // 3. Send per-member rekey envelopes via inbox
            // CRITICAL PATH: rekey envelope delivery — bounded 30s timeout
            let envelopeData = try! JSONEncoder().encode(envelope)
            let totalCount = candidate.members.count
            var sentCount = 0

            // Capture @MainActor-isolated values before entering task group
            let bundles = transportBundles[groupID] ?? [:]
            let signingKey = keyManager.ed25519SigningKey
            let km = keyManager
            let urls = relayURLs

            let rekeyDeadline = Date().addingTimeInterval(30)

            for member in candidate.members {
                // Bail if timeout exceeded — retry mechanism handles the rest
                if Date() > rekeyDeadline {
                    #if DEBUG
                    print("[Rekey] TIMEOUT groupID=\(groupID.prefix(8)) epoch=\(candidate.epoch) sent=\(sentCount)/\(totalCount) — retry handles rest")
                    #endif
                    break
                }

                let hex = member.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
                guard let bundle = bundles[hex] else { continue }
                // Don't send to self
                if let myLeaf = try? km.memberLeaf,
                   myLeaf.publicKeyCompressed == member.publicKeyCompressed { continue }
                do {
                    let sealed = try GroupCrypto.encryptInvitation(
                        envelopeData,
                        recipientKeyAgreementPubkey: bundle.x25519InboxPubkey,
                        senderSigningKey: signingKey
                    )
                    let sealedData = try JSONEncoder().encode(sealed)
                    let content = sealedData.base64EncodedString()
                    let recipientInboxTag = GroupCrypto.hiddenInboxTag(recipientPublicKey: bundle.x25519InboxPubkey)
                    let tags = InvitationTransport.eventTags(recipientInboxTag: recipientInboxTag)
                    let event = try NostrEvent.build(kind: 34113, tags: tags, content: content, keyManager: km,
                                                     ephemeralSigner: try RustBackedNostrSigner.ephemeral())
                    try await publishEvent(event, relayURLs: urls)
                    sentCount += 1
                    #if DEBUG
                    print("[Rekey] ENVELOPE_SENT groupID=\(groupID.prefix(8)) epoch=\(candidate.epoch) recipient=\(hex.prefix(8)) eventID=\(event.id.prefix(12)) relays=\(urls.count)")
                    #endif
                } catch {
                    #if DEBUG
                    print("[Rekey] ENVELOPE_FAILED groupID=\(groupID.prefix(8)) epoch=\(candidate.epoch) recipient=\(hex.prefix(8)) error=\(error)")
                    #endif
                }
            }

            #if DEBUG
            print("[Rekey] COMPLETE groupID=\(groupID.prefix(8)) epoch=\(candidate.epoch) sent=\(sentCount)/\(totalCount)")
            #endif

            // Track as removal epoch for salt request filtering
            store.savePendingRekey(
                groupID: groupID, epoch: Int(candidate.epoch),
                envelope: envelopeData,
                unackedKeys: candidate.members.compactMap { m in
                    let h = m.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
                    if let myLeaf = try? keyManager.memberLeaf, myLeaf.publicKeyCompressed == m.publicKeyCompressed { return nil }
                    return h
                },
                isRemovalEpoch: true
            )
        } else if isRemoval {
            // Removals REQUIRE inbox delivery (secure rekey). If bundles are missing,
            // the remover must first do a key rotation to distribute bundles.
            #if DEBUG
            print("[EpochTransition] Removal blocked: not all remaining members have transport bundles. Rotate key first.")
            #endif
            // Revert the optimistic local state
            groups[index] = currentGroup
            store.saveGroup(currentGroup)
            return .bundlesMissing
        } else {
            // --- LEGACY FLOW: non-removal operations only (add member, key rotation) ---
            let update = buildStateUpdate(
                group: candidate,
                addedMembers: addedMembers,
                removedMemberKeys: removedMemberKeys
            )
            do {
                try await chatTransport.sendProtocolMessage(
                    update, topic: candidate.topicTag, key: previousKey, keyManager: keyManager
                )
            } catch {
                #if DEBUG
                print("[EpochTransition] Failed to broadcast state update: \(error)")
                #endif
            }
        }

        // Insert system message for the transition
        switch kind {
        case .memberAdd(let member):
            let name = memberDisplayName(blsPubkey: member.publicKeyCompressed)
            insertSystemMessage(groupID: groupID, text: "\(name) joined the group", event: "member-add", epoch: candidate.epoch)
        case .memberRemove(let blsPubkey):
            let name = memberDisplayName(blsPubkey: blsPubkey)
            insertSystemMessage(groupID: groupID, text: "\(name) was removed from the group", event: "member-remove", epoch: candidate.epoch)
        case .keyRotation:
            insertSystemMessage(groupID: groupID, text: "Encryption key was rotated", event: "key-rotation", epoch: candidate.epoch)
        }

        return .success
    }

    /// Check if a group has a pending epoch transition (for UI locking).
    func pendingTransitionState(for groupID: String) -> PendingTransitionState {
        pendingTransitions[groupID] ?? .idle
    }

    // MARK: - System Messages

    /// Display name for a BLS member key: "You" for self, alias if set, or truncated hex.
    private func memberDisplayName(blsPubkey: Data) -> String {
        if let myLeaf = try? keyManager.memberLeaf,
           myLeaf.publicKeyCompressed == blsPubkey {
            return "You"
        }
        let hex = blsPubkey.map { String(format: "%02x", $0) }.joined()
        return contactAliasStore?.displayName(for: hex) ?? (String(hex.prefix(8)) + "...")
    }

    /// Insert a system message into chatMessages (and persist). Uses deterministic IDs for dedup.
    private func insertSystemMessage(groupID: String, text: String, event: String, epoch: UInt64) {
        let id = "sys-\(event)-\(String(groupID.prefix(8)))-\(epoch)"
        guard !(chatMessages[groupID]?.contains(where: { $0.id == id }) ?? false) else { return }
        let msg = ChatMessage(
            id: id,
            groupID: groupID,
            senderPubkey: "",
            text: text,
            timestamp: Date(),
            isMine: false,
            isSystemMessage: true
        )
        var messages = chatMessages[groupID, default: []]
        messages.append(msg)
        chatMessages[groupID] = messages
        store.saveMessageAsync(msg)
    }

    // MARK: - Transport Bundle Management

    /// Verify and persist a transport bundle for a group member.
    private func processTransportBundle(_ bundle: SEPMemberTransportBundle, groupID: String) {
        guard bundle.hasValidStructure else { return }
        guard KeyManager.verifyTransportBundle(bundle) else {
            SecurityLog.invalidAttestation(reason: "transport bundle signature verification failed")
            return
        }
        let hex = bundle.blsPubkey.map { String(format: "%02x", $0) }.joined()
        // Only update if new or higher version
        if let existing = transportBundles[groupID]?[hex], existing.version >= bundle.version { return }
        transportBundles[groupID, default: [:]][hex] = bundle
        store.saveTransportBundleAsync(bundle, groupID: groupID)
    }

    /// Create and persist own transport bundle for a group.
    private func ensureOwnTransportBundle(groupID: String) {
        guard let bundle = try? keyManager.createTransportBundle() else { return }
        let hex = bundle.blsPubkey.map { String(format: "%02x", $0) }.joined()
        transportBundles[groupID, default: [:]][hex] = bundle
        store.saveTransportBundleAsync(bundle, groupID: groupID)
    }

    /// Check if all remaining members (excluding removed ones) have transport bundles.
    private func allMembersHaveBundles(groupID: String, members: [SEPGroupMemberLeaf]) -> Bool {
        guard let bundles = transportBundles[groupID] else { return false }
        return members.allSatisfy { member in
            let hex = member.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
            return bundles[hex] != nil
        }
    }

    /// Kind-aware event router. Prevents wrong-transport bugs by routing based on event kind.
    /// - kind 34113 → invitationTransport (inbox, per-recipient)
    /// - kind 44114 → chatTransport (group broadcast)
    private func publishEvent(_ event: NostrEvent, relayURLs: [URL] = []) async throws {
        switch event.kind {
        case 34113:
            try await invitationTransport.publishToRelays(event, relayURLs: relayURLs)
        case 44114:
            try await chatTransport.publishToRelays(event)
        default:
            assertionFailure("publishEvent: unknown event kind \(event.kind)")
            throw ChatError.relayPublishFailed
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

    // MARK: - Epoch Snapshots (epoch branching)

    /// Capture an epoch snapshot for branching. Call BEFORE advancing to a new epoch.
    func captureEpochSnapshot(group: ChatGroup, changeDescription: String) {
        let snapshot = EpochSnapshot(
            epoch: group.epoch,
            members: group.members,
            salt: group.salt,
            groupSecret: group.groupSecret,
            changeDescription: changeDescription
        )
        if epochSnapshots[group.id] == nil {
            epochSnapshots[group.id] = [:]
        }
        epochSnapshots[group.id]?[group.epoch] = snapshot
        // Cap to window size
        if let history = epochSnapshots[group.id], history.count > Self.epochSnapshotWindow {
            let sortedKeys = history.keys.sorted()
            let toRemove = sortedKeys.prefix(history.count - Self.epochSnapshotWindow)
            for key in toRemove { epochSnapshots[group.id]?.removeValue(forKey: key) }
        }
        store.saveEpochSnapshotAsync(snapshot, groupID: group.id)
        store.trimEpochSnapshots(groupID: group.id, maxCount: Self.epochSnapshotWindow)
    }

    /// Resolve an encryption key for a specific epoch of a group (for multi-epoch decryption).
    func epochKey(groupID: String, epoch: UInt64) -> SymmetricKey? {
        // Try epoch snapshot first (has groupSecret which may differ across rekey boundaries)
        if let snapshot = epochSnapshots[groupID]?[epoch] {
            return snapshot.encryptionKey()
        }
        // Fallback: use current group's groupSecret + salt history
        guard let group = groups.first(where: { $0.id == groupID }),
              let salt = getSalt(groupID: groupID, epoch: epoch) else { return nil }
        return GroupCrypto.deriveMessageKey(groupSecret: group.groupSecret, epoch: epoch, salt: salt)
    }

    // MARK: - Epoch Pinning

    /// Pin the user's view to a specific epoch (epoch branching).
    /// Sending uses the pinned epoch's key/topic, member list shows the pinned epoch's members.
    /// Protocol processing always tracks the latest epoch.
    func pinEpoch(groupID: String, epoch: UInt64) {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        groups[index].pinnedEpoch = epoch
        store.saveGroup(groups[index])

        // If pinned epoch has a different groupSecret (secure rekey boundary),
        // subscribe to the old topic too so messages are visible.
        if let snapshot = epochSnapshots[groupID]?[epoch] {
            let pinnedTopic = snapshot.topicTag
            let currentTopic = groups[index].topicTag
            if pinnedTopic != currentTopic {
                let gid = groupID
                chatTransport.subscribe(
                    topic: pinnedTopic,
                    groupID: gid,
                    keyResolver: { [weak self] e in self?.epochKey(groupID: gid, epoch: e) },
                    defaultKey: snapshot.encryptionKey(),
                    sinceTimestamp: groups[index].lastEventTimestamp > 0 ? groups[index].lastEventTimestamp : nil
                )
            }
        }

        // Update member list to include pinned epoch members
        updateCurrentMembers()
    }

    /// Unpin and return to following the latest epoch.
    func unpinEpoch(groupID: String) {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        let oldPinnedEpoch = groups[index].pinnedEpoch
        groups[index].pinnedEpoch = nil
        store.saveGroup(groups[index])

        // Unsubscribe from old topic if it was across a rekey boundary
        if let oldEpoch = oldPinnedEpoch,
           let snapshot = epochSnapshots[groupID]?[oldEpoch] {
            let pinnedTopic = snapshot.topicTag
            let currentTopic = groups[index].topicTag
            if pinnedTopic != currentTopic {
                chatTransport.unsubscribe(topic: pinnedTopic)
            }
        }

        updateCurrentMembers()
    }

    /// Update transport's currentMembers to union of all groups' current + pinned epoch members.
    private func updateCurrentMembers() {
        var allMembers = groups.flatMap(\.members)
        for group in groups {
            if let pinned = group.pinnedEpoch,
               let snapshot = epochSnapshots[group.id]?[pinned] {
                for member in snapshot.members {
                    if !allMembers.contains(where: { $0.publicKeyCompressed == member.publicKeyCompressed }) {
                        allMembers.append(member)
                    }
                }
            }
        }
        chatTransport.currentMembers = allMembers
    }

    /// The effective encryption key for sending in a group (respects pinnedEpoch).
    func effectiveEncryptionKey(for group: ChatGroup) -> SymmetricKey {
        if let pinned = group.pinnedEpoch,
           let snapshot = epochSnapshots[group.id]?[pinned] {
            return snapshot.encryptionKey()
        }
        return group.encryptionKey
    }

    /// The effective topic tag for sending in a group (respects pinnedEpoch).
    func effectiveTopicTag(for group: ChatGroup) -> String {
        if let pinned = group.pinnedEpoch,
           let snapshot = epochSnapshots[group.id]?[pinned] {
            return snapshot.topicTag
        }
        return group.topicTag
    }

    /// The effective epoch for sending in a group (respects pinnedEpoch).
    func effectiveEpoch(for group: ChatGroup) -> UInt64 {
        group.pinnedEpoch ?? group.epoch
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

        let myBundle = try? keyManager.createTransportBundle()

        // Include all known transport bundles so every member can collect bundles
        // from members who joined after them — required for secure rekey.
        let allBundles = transportBundles[group.id].map { Array($0.values) }

        return SEPGroupStateUpdate(
            epoch: group.epoch,
            salt: group.salt,
            addedMembers: addedMembers,
            removedMemberKeys: removedMemberKeys,
            commitment: group.commitment,
            senderAttestation: attestation,
            senderTransportBundle: myBundle,
            memberBundles: allBundles
        )
    }

    /// Apply a received state update to a local group.
    /// Handles three cases:
    /// Apply a received state update. For published groups, chain-confirm-first:
    /// the update is only applied if on-chain epoch/commitment matches.
    /// For unpublished groups, applies directly (local-first).
    ///
    /// - update.epoch > local: verify against chain (published) or apply directly (unpublished)
    /// - update.epoch == local + same salt: own echo, ignored
    /// - update.epoch == local + different salt: epoch fork — for published groups, adopt
    ///   chain-confirmed state; for unpublished, discard local pending and accept remote
    /// - update.epoch < local: stale update, ignored
    func applyStateUpdate(_ update: SEPGroupStateUpdate, to groupID: String) async {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        var group = groups[index]

        // Stale update — ignore
        guard update.epoch >= group.epoch else { return }

        // Capture epoch snapshot before any mutation
        let changeDesc: String
        if !update.removedMemberKeys.isEmpty {
            let names = update.removedMemberKeys.map { memberDisplayName(blsPubkey: $0) }.joined(separator: ", ")
            changeDesc = "\(names) was removed from the group"
        } else if !update.addedMembers.isEmpty {
            let names = update.addedMembers.map { memberDisplayName(blsPubkey: $0.publicKeyCompressed) }.joined(separator: ", ")
            changeDesc = "\(names) joined the group"
        } else {
            changeDesc = "Encryption key was rotated"
        }
        captureEpochSnapshot(group: group, changeDescription: changeDesc)

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

        // Extract and persist sender's transport bundle if present
        if let bundle = update.senderTransportBundle {
            processTransportBundle(bundle, groupID: groupID)
        }
        // Process all member bundles — enables secure rekey for members who
        // joined after others and never received their bundles directly.
        for bundle in update.memberBundles ?? [] {
            processTransportBundle(bundle, groupID: groupID)
        }

        if update.epoch == group.epoch {
            // Same epoch + same salt = our own update echoed back — ignore
            if update.salt == group.salt {
                #if DEBUG
                print("[AppState] Ignoring own echo at epoch=\(update.epoch) for group=\(groupID.prefix(8))")
                #endif
                return
            }

            // Epoch fork: concurrent changes at the same epoch.
            // For published groups: fetch chain state and adopt whichever was confirmed.
            // For unpublished groups: accept the remote state (no authority to resolve locally).
            if group.isPublishedOnChain, let service = onChainService {
                do {
                    let entry = try await service.fetchOnChainState(groupIDData: group.groupIDData)
                    #if DEBUG
                    print("[AppState] Fork at epoch=\(group.epoch): chain epoch=\(entry.epoch) for group=\(groupID.prefix(8))")
                    #endif
                    if entry.epoch > group.epoch {
                        // Chain has moved ahead — adopt chain epoch.
                        // Apply member delta from the update, set epoch/salt from chain,
                        // but we need the salt from the update (chain doesn't store salt).
                        applyMemberDelta(update, to: &group)
                        group.epoch = entry.epoch
                        group.salt = update.salt
                        if let commitment = update.commitment {
                            group.commitment = commitment
                        }
                        // Discard any pending local transition
                        pendingTransitions[groupID] = .idle
                    } else {
                        // Chain hasn't moved — our local transition was confirmed.
                        // Ignore the conflicting remote update.
                        #if DEBUG
                        print("[AppState] Fork: local state confirmed on-chain, ignoring remote fork")
                        #endif
                        return
                    }
                } catch {
                    // Chain unavailable — fall through to accept remote update
                    #if DEBUG
                    print("[AppState] Fork: chain fetch failed, accepting remote: \(error)")
                    #endif
                    applyMemberDelta(update, to: &group)
                    group.epoch = update.epoch + 1
                    group.salt = update.salt
                    try? group.recomputeCommitment()
                }
            } else {
                // Unpublished: accept remote state, bump epoch to resolve
                applyMemberDelta(update, to: &group)
                group.epoch += 1
                group.salt = update.salt
                try? group.recomputeCommitment()
            }

            groups[index] = group
            store.saveGroup(group)
            storeSalt(groupID: groupID, epoch: group.epoch, salt: group.salt)
            subscribeGroup(group)

            #if DEBUG
            print("[AppState] Epoch fork resolved to epoch=\(group.epoch) for group=\(groupID.prefix(8))")
            #endif
        } else {
            // Normal case: update.epoch > group.epoch
            // Apply optimistically — sender already verified with chain.
            // Check if WE were removed before applying
            let selfRemoved = isSelfRemoved(in: update)

            applyMemberDelta(update, to: &group)
            group.epoch = update.epoch
            group.salt = update.salt
            if let commitment = update.commitment {
                group.commitment = commitment
            }
            if selfRemoved {
                group.removedByPubkeyHex = update.senderAttestation.map {
                    $0.blsPubkey.map { String(format: "%02x", $0) }.joined()
                }
            }

            // Discard any pending local transition that's now superseded
            if case .awaitingChainConfirmation(let target) = pendingTransitions[groupID],
               update.epoch >= target {
                pendingTransitions[groupID] = .idle
            }

            groups[index] = group
            store.saveGroup(group)
            storeSalt(groupID: groupID, epoch: update.epoch, salt: update.salt)

            if selfRemoved {
                // Unsubscribe — do NOT resubscribe with the new key
                #if !DEBUG
                chatTransport.unsubscribe(topic: group.topicTag)
                #endif
                let removerName = update.senderAttestation.map { memberDisplayName(blsPubkey: $0.blsPubkey) } ?? "unknown"
                insertSystemMessage(groupID: groupID, text: "You were removed from this group by \(removerName)", event: "self-removed", epoch: update.epoch)
            } else {
                subscribeGroup(group)

                // System messages for peer state updates
                for added in update.addedMembers {
                    let name = memberDisplayName(blsPubkey: added.publicKeyCompressed)
                    insertSystemMessage(groupID: groupID, text: "\(name) joined the group", event: "member-add", epoch: update.epoch)
                }
                for removed in update.removedMemberKeys {
                    let name = memberDisplayName(blsPubkey: removed)
                    insertSystemMessage(groupID: groupID, text: "\(name) was removed from the group", event: "member-remove", epoch: update.epoch)
                }
                if update.addedMembers.isEmpty && update.removedMemberKeys.isEmpty {
                    insertSystemMessage(groupID: groupID, text: "Encryption key was rotated", event: "key-rotation", epoch: update.epoch)
                }
            }
        }
    }

    /// Check if the current user's BLS pubkey is in the removal list.
    private func isSelfRemoved(in update: SEPGroupStateUpdate) -> Bool {
        guard let myLeaf = try? keyManager.memberLeaf else { return false }
        return update.removedMemberKeys.contains { $0 == myLeaf.publicKeyCompressed }
    }

    /// Check if the current user is a member of the given group.
    func isMember(of group: ChatGroup) -> Bool {
        guard let myLeaf = try? keyManager.memberLeaf else { return false }
        return group.members.contains { $0.publicKeyCompressed == myLeaf.publicKeyCompressed }
    }

    /// Check if the current user can send in this group (considers pinned epoch).
    func canSend(in group: ChatGroup) -> Bool {
        guard let myLeaf = try? keyManager.memberLeaf else { return false }
        // If pinned, check the pinned epoch's member list
        if let pinned = group.pinnedEpoch,
           let snapshot = epochSnapshots[group.id]?[pinned] {
            return snapshot.members.contains { $0.publicKeyCompressed == myLeaf.publicKeyCompressed }
        }
        return group.members.contains { $0.publicKeyCompressed == myLeaf.publicKeyCompressed }
    }

    /// Apply the member delta (adds/removes) from a state update to a group.
    private func applyMemberDelta(_ update: SEPGroupStateUpdate, to group: inout ChatGroup) {
        for removed in update.removedMemberKeys {
            group.members.removeAll { $0.publicKeyCompressed == removed }
        }
        for added in update.addedMembers {
            if !group.members.contains(where: { $0.publicKeyCompressed == added.publicKeyCompressed }) {
                group.members.append(added)
            }
        }
        group.members.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }
    }

    /// Rotate the group key without membership changes. Provides forward secrecy.
    /// For published groups, blocks until chain confirms the new epoch.
    func rotateGroupKey(groupID: String) async {
        guard let group = groups.first(where: { $0.id == groupID }) else { return }

        let previousKey = group.encryptionKey

        let result = await performEpochTransition(
            groupID: groupID,
            kind: .keyRotation,
            previousKey: previousKey
        )

        #if DEBUG
        print("[AppState] rotateGroupKey result=\(result) group=\(groupID.prefix(8))")
        #endif
    }

    // MARK: - Fork Group

    /// Whether a removed member can fork this group (epoch snapshots exist where they were a member).
    func canForkGroup(_ group: ChatGroup) -> Bool {
        guard !isMember(of: group) else { return false }
        guard let myLeaf = try? keyManager.memberLeaf else { return false }
        guard let snapshots = epochSnapshots[group.id] else { return false }
        return snapshots.values.contains { snapshot in
            snapshot.members.contains { $0.publicKeyCompressed == myLeaf.publicKeyCompressed }
        }
    }

    /// Fork a group from the latest epoch where the current user was a member,
    /// excluding specified members. Creates a new, cryptographically independent group.
    func forkGroup(
        originalGroupID: String,
        excludingMembers: [Data],
        forkName: String? = nil
    ) async throws -> ChatGroup {
        guard let originalGroup = groups.first(where: { $0.id == originalGroupID }) else {
            throw ChatError.verificationFailed("Group not found")
        }
        guard let myLeaf = try? keyManager.memberLeaf else {
            throw ChatError.noKey
        }
        guard let snapshots = epochSnapshots[originalGroupID] else {
            throw ChatError.verificationFailed("No epoch snapshots available")
        }

        // Find the latest epoch where the user was a member
        let myKey = myLeaf.publicKeyCompressed
        guard let forkSnapshot = snapshots.values
            .filter({ snapshot in snapshot.members.contains { $0.publicKeyCompressed == myKey } })
            .max(by: { $0.epoch < $1.epoch })
        else {
            throw ChatError.verificationFailed("No epoch found where you were a member")
        }

        // Generate new group ID and secret
        var groupIDBytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, 32, &groupIDBytes)
        let newGroupIDHex = Data(groupIDBytes).map { String(format: "%02x", $0) }.joined()

        var secretBytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, 32, &secretBytes)
        let newGroupSecret = Data(secretBytes)

        // Build member list: snapshot members minus excluded, ensuring self is included
        var forkMembers = forkSnapshot.members.filter { member in
            !excludingMembers.contains(member.publicKeyCompressed)
        }
        if !forkMembers.contains(where: { $0.publicKeyCompressed == myKey }) {
            forkMembers.append(myLeaf)
        }
        forkMembers.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }

        let name = forkName ?? "Fork of \(originalGroup.name)"
        var forkedGroup = ChatGroup(
            id: newGroupIDHex,
            name: name,
            groupSecret: newGroupSecret,
            createdAt: Date(),
            relayHints: originalGroup.relayHints,
            members: forkMembers,
            epoch: 0,
            salt: SEPCommitmentBuilder.generateSalt(),
            tier: originalGroup.tier
        )
        forkedGroup.forkedFromGroupID = originalGroupID
        forkedGroup.forkedAtEpoch = forkSnapshot.epoch
        try forkedGroup.recomputeCommitment()

        addGroup(forkedGroup)
        captureEpochSnapshot(group: forkedGroup, changeDescription: "Forked from \(originalGroup.name) at epoch \(forkSnapshot.epoch)")
        ensureOwnTransportBundle(groupID: newGroupIDHex)

        // Copy messages from the original group up to the fork epoch
        let originalMessages = chatMessages[originalGroupID] ?? []
        let forkEpoch = forkSnapshot.epoch
        let copiedMessages: [ChatMessage] = originalMessages.compactMap { msg in
            // Include messages at or before the fork epoch
            if let msgEpoch = msg.epoch, msgEpoch > forkEpoch { return nil }
            return ChatMessage(
                id: "fork-\(msg.id)",
                groupID: newGroupIDHex,
                senderPubkey: msg.senderPubkey,
                text: msg.text,
                timestamp: msg.timestamp,
                isMine: msg.isMine,
                status: msg.status,
                mediaAttachment: msg.mediaAttachment,
                isSystemMessage: msg.isSystemMessage,
                epoch: msg.epoch
            )
        }
        chatMessages[newGroupIDHex] = copiedMessages
        for msg in copiedMessages {
            store.saveMessageAsync(msg)
        }

        let excludedNames = excludingMembers.map { memberDisplayName(blsPubkey: $0) }.joined(separator: ", ")
        let sysText = excludedNames.isEmpty
            ? "Group forked from \(originalGroup.name)."
            : "Group forked from \(originalGroup.name). Excluded: \(excludedNames)."
        insertSystemMessage(groupID: newGroupIDHex, text: sysText, event: "group-forked", epoch: 0)

        // Publish on-chain (fire-and-forget)
        if isContractConfigured {
            Task { try? await publishGroupOnChain(forkedGroup) }
        }

        #if DEBUG
        print("[AppState] forkGroup created=\(newGroupIDHex.prefix(8)) from=\(originalGroupID.prefix(8)) epoch=\(forkSnapshot.epoch) members=\(forkMembers.count)")
        #endif

        // Send invitations in background (fire-and-forget) — don't block fork creation
        let otherMembers = forkMembers.filter { $0.publicKeyCompressed != myKey }
        if !otherMembers.isEmpty {
            let originalBundles = transportBundles[originalGroupID] ?? store.loadTransportBundles(groupID: originalGroupID)
            let payload = BootstrapPayload.from(group: forkedGroup, senderPubkey: keyManager.publicKeyHex)
            let forkID = newGroupIDHex
            Task { [weak self] in
                guard let self else { return }
                var invitedCount = 0
                var failedCount = 0
                for (index, member) in otherMembers.enumerated() {
                    if index > 0 { try? await Task.sleep(nanoseconds: 500_000_000) } // Space out relay publishes
                    let hex = member.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
                    guard let bundle = originalBundles[hex] else { failedCount += 1; continue }
                    do {
                        try await self.invitationTransport.sendInvitation(
                            payload: payload,
                            recipientKeyAgreementPubkey: bundle.x25519InboxPubkey,
                            keyManager: self.keyManager
                        )
                        invitedCount += 1
                    } catch {
                        failedCount += 1
                        #if DEBUG
                        print("[AppState] forkGroup: failed to invite \(hex.prefix(8)): \(error)")
                        #endif
                    }
                }
                await MainActor.run {
                    if invitedCount > 0 && failedCount > 0 {
                        self.insertSystemMessage(groupID: forkID, text: "Sent \(invitedCount) invitation\(invitedCount == 1 ? "" : "s"), \(failedCount) failed.", event: "fork-invitations-sent", epoch: 0)
                    } else if invitedCount > 0 {
                        self.insertSystemMessage(groupID: forkID, text: "Sent invitations to \(invitedCount) member\(invitedCount == 1 ? "" : "s").", event: "fork-invitations-sent", epoch: 0)
                    } else if failedCount > 0 {
                        self.insertSystemMessage(groupID: forkID, text: "Failed to send \(failedCount) invitation\(failedCount == 1 ? "" : "s").", event: "fork-invitations-sent", epoch: 0)
                    }
                }
            }
        }

        return forkedGroup
    }

    /// Remove a member from a group via chain-confirmed transition.
    func removeMember(blsPubkey: Data, from groupID: String) async throws {
        guard let group = groups.first(where: { $0.id == groupID }) else {
            throw ChatError.verificationFailed("Group not found")
        }
        guard group.members.contains(where: { $0.publicKeyCompressed == blsPubkey }) else {
            throw ChatError.verificationFailed("Member not found")
        }

        let previousKey = group.encryptionKey

        let result = await performEpochTransition(
            groupID: groupID,
            kind: .memberRemove(blsPubkey: blsPubkey),
            previousKey: previousKey
        )

        switch result {
        case .success:
            break
        case .chainRejected(let reason):
            throw ChatError.onChainPublishFailed(reason)
        case .chainUnavailable:
            throw ChatError.contractNotConfigured
        case .chainMismatch:
            throw ChatError.verificationFailed("Chain epoch mismatch after submission")
        case .syncRequired:
            throw ChatError.verificationFailed("Group has a pending transition")
        case .bundlesMissing:
            throw ChatError.verificationFailed("Transport bundles missing. Rotate key first to distribute bundles, then retry removal.")
        }
    }

    /// Apply a group rename received from the protocol channel.
    /// Apply a re-key message: replace the poisoned salt with the real salt and resubscribe.
    func applyRekey(_ rekey: SEPRekey, to groupID: String) {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        var group = groups[index]
        // Don't apply rekey if we were removed from this group
        guard isMember(of: group) else {
            #if DEBUG
            print("[AppState] Ignoring rekey — no longer a member of group=\(groupID.prefix(8))")
            #endif
            return
        }
        guard rekey.epoch == group.epoch else { return }
        // Only apply if the salt differs (i.e., we have the poisoned salt)
        guard group.salt != rekey.salt else { return }
        group.salt = rekey.salt
        try? group.recomputeCommitment()
        groups[index] = group
        store.saveGroup(group)
        storeSalt(groupID: groupID, epoch: rekey.epoch, salt: rekey.salt)
        subscribeGroup(group)
        // Update push subscription with new topic tag and key material
        if group.pushNotificationsEnabled {
            Task {
                await pushManager.updateGroupSubscription(
                    groupID: groupID,
                    newTopicTag: group.topicTag,
                    epoch: UInt64(rekey.epoch),
                    groupSecret: group.groupSecret,
                    salt: rekey.salt
                )
            }
        }
        #if DEBUG
        print("[AppState] Applied re-key for epoch=\(rekey.epoch) group=\(groupID.prefix(8))")
        #endif
    }

    /// Apply a secure rekey envelope received via inbox. Installs fresh groupSecret/salt and migrates topic.
    func applyRekeyEnvelope(_ envelope: SEPRekeyEnvelope) {
        let groupIDHex = envelope.groupID.map { String(format: "%02x", $0) }.joined()
        guard let index = groups.firstIndex(where: { $0.id == groupIDHex }) else { return }
        var group = groups[index]

        // Only apply if epoch is ahead of current
        guard envelope.epoch > group.epoch else { return }

        // Verify sender bundle signature AND membership
        guard KeyManager.verifyTransportBundle(envelope.senderBundle) else {
            SecurityLog.invalidAttestation(reason: "rekey envelope sender bundle verification failed")
            return
        }
        // Sender must be a current member of the group (not a removed ex-member)
        let senderHex = envelope.senderBundle.blsPubkey.map { String(format: "%02x", $0) }.joined()
        guard group.members.contains(where: { $0.publicKeyCompressed.map { String(format: "%02x", $0) }.joined() == senderHex }) else {
            #if DEBUG
            print("[AppState] Rejected rekey envelope: sender \(senderHex.prefix(8)) not a member")
            #endif
            SecurityLog.nonMemberMessageRejected(groupID: groupIDHex)
            return
        }

        #if DEBUG
        print("[Rekey] RECEIVED groupID=\(groupIDHex.prefix(8)) epoch=\(envelope.epoch) sender=\(senderHex.prefix(8))")
        #endif

        let oldTopicTag = group.topicTag

        // Apply member changes from envelope
        var updatedMembers = group.members
        for removed in envelope.removedMemberKeys {
            updatedMembers.removeAll { $0.publicKeyCompressed == removed }
        }
        updatedMembers.sort { $0.publicKeyCompressed.lexicographicallyPrecedes($1.publicKeyCompressed) }

        // Install new secrets — ChatGroup.groupSecret is `let`, so create a new instance
        var updatedGroup = ChatGroup(
            id: group.id,
            name: group.name,
            groupSecret: envelope.groupSecret,
            createdAt: group.createdAt,
            relayHints: group.relayHints,
            members: updatedMembers,
            epoch: envelope.epoch,
            salt: envelope.salt,
            commitment: envelope.commitment ?? group.commitment,
            tier: group.tier,
            isPublishedOnChain: group.isPublishedOnChain,
            lastEventTimestamp: group.lastEventTimestamp
        )
        // Carry over mutable state that isn't part of the rekey
        updatedGroup.pinnedEpoch = group.pinnedEpoch
        updatedGroup.forkedFromGroupID = group.forkedFromGroupID
        updatedGroup.forkedAtEpoch = group.forkedAtEpoch
        updatedGroup.removedByPubkeyHex = group.removedByPubkeyHex
        updatedGroup.pushNotificationsEnabled = group.pushNotificationsEnabled
        group = updatedGroup

        // Update transport bundles from envelope
        for bundle in envelope.memberBundles {
            processTransportBundle(bundle, groupID: groupIDHex)
        }
        processTransportBundle(envelope.senderBundle, groupID: groupIDHex)

        groups[index] = group
        store.saveGroup(group)
        storeSalt(groupID: groupIDHex, epoch: envelope.epoch, salt: envelope.salt)

        // Migrate topic: unsubscribe old, subscribe new (groupSecret changed)
        #if !DEBUG
        chatTransport.unsubscribe(topic: oldTopicTag)
        #endif
        subscribeGroup(group)
        chatTransport.currentMembers = groups.flatMap(\.members)

        // Update push subscription with new key material
        if group.pushNotificationsEnabled {
            Task {
                await pushManager.updateGroupSubscription(
                    groupID: groupIDHex,
                    newTopicTag: group.topicTag,
                    epoch: UInt64(envelope.epoch),
                    groupSecret: envelope.groupSecret,
                    salt: envelope.salt
                )
            }
        }

        // System messages for removed members
        for removed in envelope.removedMemberKeys {
            let name = memberDisplayName(blsPubkey: removed)
            insertSystemMessage(groupID: groupIDHex, text: "\(name) was removed from the group", event: "member-remove", epoch: envelope.epoch)
        }

        #if DEBUG
        let newTopic = group.topicTag
        print("[Rekey] INSTALLED groupID=\(groupIDHex.prefix(8)) epoch=\(envelope.epoch) newTopic=\(newTopic.prefix(8))")
        #endif

        // Send ack on the NEW group topic
        if let myBls = try? keyManager.blsPublicKey {
            let ack = SEPRekeyAck(groupID: envelope.groupID, epoch: envelope.epoch, senderBlsPubkey: myBls)
            Task {
                try? await chatTransport.sendProtocolMessage(
                    ack, topic: group.topicTag, key: group.encryptionKey, keyManager: keyManager)
            }
        }
    }

    /// Handle a resend request: look up stored envelope, verify requester, and re-send.
    private func handleRekeyResendRequest(_ req: SEPRekeyResendRequest, groupID: String) {
        guard KeyManager.verifyTransportBundle(req.requesterBundle) else { return }
        let pending = store.loadPendingRekeys(groupID: groupID)
        guard let record = pending.first(where: { $0.epoch == Int(req.epoch) }) else { return }

        // Verify requester is an authorized surviving member for this epoch
        let requesterHex = req.requesterBundle.blsPubkey.map { String(format: "%02x", $0) }.joined()
        let unackedKeys = (try? JSONDecoder().decode([String].self, from: record.unackedMemberKeysJSON)) ?? []
        let isCurrentMember = groups.first(where: { $0.id == groupID })?.members
            .contains(where: { $0.publicKeyCompressed.map { String(format: "%02x", $0) }.joined() == requesterHex }) ?? false
        guard unackedKeys.contains(requesterHex) || isCurrentMember else {
            #if DEBUG
            print("[AppState] Resend denied: requester \(requesterHex.prefix(8)) not authorized for epoch=\(req.epoch)")
            #endif
            SecurityLog.nonMemberMessageRejected(groupID: groupID)
            return
        }

        guard let envelopeData = try? StorageEncryption.decrypt(record.encryptedEnvelope) else { return }

        // Re-send to the requester's inbox
        Task {
            do {
                let sealed = try GroupCrypto.encryptInvitation(
                    envelopeData,
                    recipientKeyAgreementPubkey: req.requesterBundle.x25519InboxPubkey,
                    senderSigningKey: keyManager.ed25519SigningKey
                )
                let sealedData = try JSONEncoder().encode(sealed)
                let content = sealedData.base64EncodedString()
                let recipientInboxTag = GroupCrypto.hiddenInboxTag(recipientPublicKey: req.requesterBundle.x25519InboxPubkey)
                let tags = InvitationTransport.eventTags(recipientInboxTag: recipientInboxTag)
                let event = try NostrEvent.build(kind: 34113, tags: tags, content: content, keyManager: keyManager,
                                                 ephemeralSigner: try RustBackedNostrSigner.ephemeral())
                try await publishEvent(event, relayURLs: relayURLs)
                #if DEBUG
                print("[AppState] Re-sent rekey envelope epoch=\(req.epoch) to requester")
                #endif
            } catch {
                #if DEBUG
                print("[AppState] Resend request failed: \(error)")
                #endif
            }
        }
    }

    /// Periodically resend rekey envelopes to unacked members. Max 3 retries, 24h expiry.
    func startRekeyResendTimer() {
        Task {
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(30))
                await resendPendingRekeys()
            }
        }
    }

    private func resendPendingRekeys() async {
        let allPending = store.loadAllPendingRekeys()
        let now = Date()
        let maxRetries = 3
        let expiryInterval: TimeInterval = 24 * 60 * 60 // 24 hours

        for record in allPending {
            // Expire old pending rekeys
            if now.timeIntervalSince(record.createdAt) > expiryInterval {
                store.deletePendingRekey(groupID: record.groupID, epoch: record.epoch)
                continue
            }
            // Max retries reached
            if record.retryCount >= maxRetries { continue }

            guard let envelopeData = try? StorageEncryption.decrypt(record.encryptedEnvelope) else { continue }
            let unackedKeys = (try? JSONDecoder().decode([String].self, from: record.unackedMemberKeysJSON)) ?? []
            if unackedKeys.isEmpty {
                store.deletePendingRekey(groupID: record.groupID, epoch: record.epoch)
                continue
            }

            let groupBundles = transportBundles[record.groupID] ?? [:]

            for hex in unackedKeys {
                guard let bundle = groupBundles[hex] else { continue }
                do {
                    let sealed = try GroupCrypto.encryptInvitation(
                        envelopeData,
                        recipientKeyAgreementPubkey: bundle.x25519InboxPubkey,
                        senderSigningKey: keyManager.ed25519SigningKey
                    )
                    let sealedData = try JSONEncoder().encode(sealed)
                    let content = sealedData.base64EncodedString()
                    let recipientInboxTag = GroupCrypto.hiddenInboxTag(recipientPublicKey: bundle.x25519InboxPubkey)
                    let tags = InvitationTransport.eventTags(recipientInboxTag: recipientInboxTag)
                    let event = try NostrEvent.build(kind: 34113, tags: tags, content: content, keyManager: keyManager,
                                                     ephemeralSigner: try RustBackedNostrSigner.ephemeral())
                    try await publishEvent(event, relayURLs: relayURLs)
                } catch {
                    #if DEBUG
                    print("[AppState] Resend failed for \(hex.prefix(8)): \(error)")
                    #endif
                }
            }
            store.incrementPendingRekeyRetry(groupID: record.groupID, epoch: record.epoch)
        }
    }

    func applyGroupRenamed(_ renamed: SEPGroupRenamed, to groupID: String) {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        // ChatGroup.name is let — we need to create a new instance
        let old = groups[index]
        var updated = ChatGroup(
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
        updated.pinnedEpoch = old.pinnedEpoch
        updated.forkedFromGroupID = old.forkedFromGroupID
        updated.forkedAtEpoch = old.forkedAtEpoch
        updated.removedByPubkeyHex = old.removedByPubkeyHex
        updated.pushNotificationsEnabled = old.pushNotificationsEnabled
        groups[index] = updated
        store.saveGroup(updated)
        insertSystemMessage(groupID: groupID, text: "Group renamed to \"\(renamed.name)\"", event: "renamed", epoch: updated.epoch)
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

    func setPushNotifications(enabled: Bool, forGroup groupID: String) {
        guard let index = groups.firstIndex(where: { $0.id == groupID }) else { return }
        groups[index].pushNotificationsEnabled = enabled
        store.saveGroupAsync(groups[index])

        let group = groups[index]
        Task {
            if enabled {
                await ensurePushPermissionAndRegister(group: group)
            } else {
                await pushManager.unregisterGroup(groupID)
            }
        }
    }

    // MARK: - Rekey Health & Diagnostics

    /// Phase 6.2: Check for groups with stale rekey state. Called on app foreground.
    func rekeyHealthCheck() {
        for group in groups {
            let bundleCount = transportBundles[group.id]?.count ?? 0
            let memberCount = group.members.count

            // Check: missing bundles
            if bundleCount < memberCount {
                #if DEBUG
                print("[RekeyHealth] WARN groupID=\(group.id.prefix(8)) bundles=\(bundleCount)/\(memberCount) — some members missing transport bundles")
                #endif
            }
        }

        // Check: pending rekeys older than 5 minutes
        let allPendingRekeys = store.loadAllPendingRekeys()
        for rekey in allPendingRekeys {
            let age = Date().timeIntervalSince(rekey.createdAt)
            if age > 300 {
                #if DEBUG
                print("[RekeyHealth] WARN groupID=\(rekey.groupID.prefix(8)) epoch=\(rekey.epoch) pending for \(Int(age))s — triggering resend")
                #endif
                Task {
                    await resendPendingRekeys()
                }
                break
            }
        }
    }

    /// Phase 6.3: Dump transport and group diagnostics for debugging.
    func dumpDiagnostics() async {
        #if DEBUG
        print("=== StellarChat Diagnostics ===")
        print("Groups: \(groups.count)")
        for group in groups {
            let bundleCount = transportBundles[group.id]?.count ?? 0
            print("  group=\(group.id.prefix(8)) epoch=\(group.epoch) members=\(group.members.count) bundles=\(bundleCount) topic=\(group.topicTag.prefix(8))")
        }
        let allPendingRekeys = store.loadAllPendingRekeys()
        print("Pending rekeys: \(allPendingRekeys.count)")
        for rekey in allPendingRekeys {
            print("  group=\(rekey.groupID.prefix(8)) epoch=\(rekey.epoch) retries=\(rekey.retryCount) age=\(Int(Date().timeIntervalSince(rekey.createdAt)))s")
        }
        let chatConnected = await chatTransport.isAnyRelayConnected
        print("Chat transport: connected=\(chatConnected) relays=\(chatTransport.connectedRelayCount)")
        print("Invitation transport: connections active")
        print("=== End Diagnostics ===")
        #endif
    }

    // MARK: - Persistent Chat & Protocol Transport

    /// Set up the chat message handler on the persistent transport (runs once at init).
    private func setupChatHandler() {
        chatTransport.onMessage = { [weak self] groupID, plaintext, senderBlsHex, event, replyToID in
            guard let self else { return }
            Task { @MainActor in
                guard self.groups.contains(where: { $0.id == groupID }) else { return }

                guard !(self.seenMessageIDs[groupID]?.contains(event.id) ?? false) else { return }
                self.seenMessageIDs[groupID, default: []].insert(event.id)

                let epochTag = event.tags.first(where: { $0.first == "epoch" }).flatMap { $0.dropFirst().first }
                let epoch = epochTag.flatMap { UInt64($0) }
                let myBlsHex = (try? self.keyManager.blsPublicKey).map { $0.map { String(format: "%02x", $0) }.joined() } ?? ""
                let msg = ChatMessage(
                    id: event.id,
                    groupID: groupID,
                    senderPubkey: senderBlsHex,
                    text: plaintext,
                    timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
                    isMine: senderBlsHex == myBlsHex,
                    epoch: epoch,
                    replyToID: replyToID
                )
                self.queueIncomingMessage(msg, groupID: groupID, event: event)
            }
        }

        chatTransport.onImageMessage = { [weak self] groupID, plaintext, media, senderBlsHex, event, replyToID in
            guard let self else { return }
            Task { @MainActor in
                guard self.groups.contains(where: { $0.id == groupID }) else { return }

                guard !(self.seenMessageIDs[groupID]?.contains(event.id) ?? false) else { return }
                self.seenMessageIDs[groupID, default: []].insert(event.id)

                let epochTag = event.tags.first(where: { $0.first == "epoch" }).flatMap { $0.dropFirst().first }
                let epoch = epochTag.flatMap { UInt64($0) }
                let myBlsHex = (try? self.keyManager.blsPublicKey).map { $0.map { String(format: "%02x", $0) }.joined() } ?? ""
                let msg = ChatMessage(
                    id: event.id,
                    groupID: groupID,
                    senderPubkey: senderBlsHex,
                    text: plaintext,
                    timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
                    isMine: senderBlsHex == myBlsHex,
                    mediaAttachment: media,
                    epoch: epoch,
                    replyToID: replyToID
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
    func sendMessage(text: String, groupID: String, replyToID: String? = nil) async throws {
        guard let group = groups.first(where: { $0.id == groupID }) else { return }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        // Block sending if we're no longer a member (considers pinned epoch)
        guard canSend(in: group) else {
            throw ChatError.verificationFailed("You are no longer a member of this group")
        }

        // Build the event synchronously so we have its deterministic ID for the local message.
        let blsPubkey = try keyManager.blsPublicKey
        let ts = Int64(Date().timeIntervalSince1970)
        var wrapper: [String: Any] = [
            "v": 2, "type": "chat", "text": trimmed,
            "senderBlsPubkey": blsPubkey.base64EncodedString(), "ts": ts
        ]
        if let replyToID {
            wrapper["replyTo"] = replyToID
        }
        let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
        let authenticatedText = String(data: wrapperData, encoding: .utf8)!
        let sendKey = effectiveEncryptionKey(for: group)
        let sendTopic = effectiveTopicTag(for: group)
        let sendEpoch = effectiveEpoch(for: group)
        let envelope = try GroupCrypto.encrypt(authenticatedText, key: sendKey)
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()
        let event = try NostrEvent.build(
            kind: 44114, tags: [["t", sendTopic], ["epoch", "\(sendEpoch)"]], content: content, keyManager: keyManager,
            ephemeralSigner: try RustBackedNostrSigner.ephemeral()
        )

        // Optimistic UI: show the message locally BEFORE waiting for relay publish.
        let msg = ChatMessage(
            id: event.id,
            groupID: groupID,
            senderPubkey: blsPubkey.map { String(format: "%02x", $0) }.joined(),
            text: trimmed,
            timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
            isMine: true,
            status: .sending,
            epoch: sendEpoch,
            replyToID: replyToID
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
        let replyToID = chatMessages[groupID]![idx].replyToID
        chatMessages[groupID]?[idx].status = .sending

        Task {
            do {
                let blsPubkey = try keyManager.blsPublicKey
                let ts = Int64(Date().timeIntervalSince1970)
                var wrapper: [String: Any] = [
                    "v": 2, "type": "chat", "text": text,
                    "senderBlsPubkey": blsPubkey.base64EncodedString(), "ts": ts
                ]
                if let replyToID {
                    wrapper["replyTo"] = replyToID
                }
                let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
                let authenticatedText = String(data: wrapperData, encoding: .utf8)!
                let sendKey = self.effectiveEncryptionKey(for: group)
                let sendTopic = self.effectiveTopicTag(for: group)
                let sendEpoch = self.effectiveEpoch(for: group)
                let envelope = try GroupCrypto.encrypt(authenticatedText, key: sendKey)
                let envelopeData = try JSONEncoder().encode(envelope)
                let content = envelopeData.base64EncodedString()
                let event = try NostrEvent.build(
                    kind: 44114, tags: [["t", sendTopic], ["epoch", "\(sendEpoch)"]], content: content, keyManager: keyManager,
                    ephemeralSigner: try RustBackedNostrSigner.ephemeral()
                )
                try await chatTransport.publishToRelays(event)
                await MainActor.run {
                    // Replace the message with updated ID and sent status
                    if let i = self.chatMessages[groupID]?.firstIndex(where: { $0.id == messageID }) {
                        let old = self.chatMessages[groupID]![i]
                        self.chatMessages[groupID]?[i] = ChatMessage(
                            id: event.id, groupID: old.groupID, senderPubkey: old.senderPubkey,
                            text: old.text, timestamp: old.timestamp, isMine: old.isMine, status: .sent,
                            epoch: sendEpoch, replyToID: old.replyToID
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
    func sendImage(imageData: Data, groupID: String, replyToID: String? = nil) async throws {
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
        var wrapper: [String: Any] = [
            "v": 2,
            "type": "image",
            "text": "Sent an image",
            "senderBlsPubkey": blsPubkey.base64EncodedString(),
            "ts": ts,
            "media": mediaDict,
        ]
        if let replyToID {
            wrapper["replyTo"] = replyToID
        }
        let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
        let wrapperText = String(data: wrapperData, encoding: .utf8)!

        // Build the Nostr event locally
        let sendKey = effectiveEncryptionKey(for: group)
        let sendTopic = effectiveTopicTag(for: group)
        let sendEpoch = effectiveEpoch(for: group)
        let envelope = try GroupCrypto.encrypt(wrapperText, key: sendKey)
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()
        let event = try NostrEvent.build(
            kind: 44114, tags: [["t", sendTopic], ["epoch", "\(sendEpoch)"]], content: content, keyManager: keyManager,
            ephemeralSigner: try RustBackedNostrSigner.ephemeral()
        )

        // Optimistic UI: show locally before relay publish
        let msg = ChatMessage(
            id: event.id,
            groupID: groupID,
            senderPubkey: blsPubkey.map { String(format: "%02x", $0) }.joined(),
            text: "Sent an image",
            timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
            isMine: true,
            status: .sending,
            mediaAttachment: media,
            epoch: sendEpoch,
            replyToID: replyToID
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

    func sendVideo(videoURL: URL, groupID: String, replyToID: String? = nil) async throws {
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
        var wrapper: [String: Any] = [
            "v": 2,
            "type": "video",
            "text": "Sent a video",
            "senderBlsPubkey": blsPubkey.base64EncodedString(),
            "ts": ts,
            "media": mediaDict,
        ]
        if let replyToID {
            wrapper["replyTo"] = replyToID
        }
        let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
        let wrapperText = String(data: wrapperData, encoding: .utf8)!

        // Build the Nostr event locally
        let sendKey = effectiveEncryptionKey(for: group)
        let sendTopic = effectiveTopicTag(for: group)
        let sendEpoch = effectiveEpoch(for: group)
        let envelope = try GroupCrypto.encrypt(wrapperText, key: sendKey)
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()
        let event = try NostrEvent.build(
            kind: 44114, tags: [["t", sendTopic], ["epoch", "\(sendEpoch)"]], content: content, keyManager: keyManager,
            ephemeralSigner: try RustBackedNostrSigner.ephemeral()
        )

        // Optimistic UI: show locally before relay publish
        let msg = ChatMessage(
            id: event.id,
            groupID: groupID,
            senderPubkey: blsPubkey.map { String(format: "%02x", $0) }.joined(),
            text: "Sent a video",
            timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
            isMine: true,
            status: .sending,
            mediaAttachment: media,
            epoch: sendEpoch,
            replyToID: replyToID
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
    func sendVoice(audioURL: URL, groupID: String, replyToID: String? = nil) async throws {
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
        var wrapper: [String: Any] = [
            "v": 2,
            "type": "audio",
            "text": "Sent a voice message",
            "senderBlsPubkey": blsPubkey.base64EncodedString(),
            "ts": ts,
            "media": mediaDict,
        ]
        if let replyToID {
            wrapper["replyTo"] = replyToID
        }
        let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
        let wrapperText = String(data: wrapperData, encoding: .utf8)!

        let envelope = try GroupCrypto.encrypt(wrapperText, key: group.encryptionKey)
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()
        let event = try NostrEvent.build(
            kind: 44114, tags: [["t", group.topicTag]], content: content, keyManager: keyManager,
            ephemeralSigner: try RustBackedNostrSigner.ephemeral()
        )

        // Optimistic UI
        let msg = ChatMessage(
            id: event.id,
            groupID: groupID,
            senderPubkey: blsPubkey.map { String(format: "%02x", $0) }.joined(),
            text: "Sent a voice message",
            timestamp: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0),
            isMine: true,
            status: .sending,
            mediaAttachment: media,
            replyToID: replyToID
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
        chatTransport.onProtocolMessage = { [weak self] groupID, json, eventID, senderBlsPubkeyHex in
            guard let self,
                  let data = json.data(using: .utf8) else { return }

            Task { @MainActor in
                // Replay protection (H-7)
                guard !self.processedProtocolEventIDs.contains(eventID) else { return }
                if self.processedProtocolEventIDs.count >= Self.maxDedupSetSize {
                    self.processedProtocolEventIDs.removeFirst()
                }
                self.processedProtocolEventIDs.insert(eventID)

                guard self.groups.contains(where: { $0.id == groupID }) else { return }

                let decoder = JSONDecoder()
                let msgType = SEPProtocolMessage.parse(json)
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
                        await self.applyStateUpdate(update, to: groupID)
                        // applyStateUpdate handles subscribe/unsubscribe internally.
                        // Only update currentMembers here — do NOT re-subscribe if self was removed.
                        if let updated = self.groups.first(where: { $0.id == groupID }) {
                            self.chatTransport.currentMembers = updated.members
                        }
                    } catch {
                        #if DEBUG
                        print("[AppState] FAILED to decode state update: \(error) json=\(json.prefix(200))")
                        #endif
                    }
                case SEPSaltRequest.messageType:
                    if let request = try? decoder.decode(SEPSaltRequest.self, from: data) {
                        // Phase 4: Refuse salt responses for removal epochs — the requester
                        // should use SEPRekeyResendRequest via inbox instead.
                        if self.store.isRemovalEpoch(groupID: groupID, epoch: Int(request.epoch)) {
                            #if DEBUG
                            print("[AppState] Salt request for removal epoch \(request.epoch) — refusing")
                            #endif
                            break
                        }
                        let rateKey = "\(senderBlsPubkeyHex ?? ""):\(request.epoch)"
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
                case SEPRekey.messageType:
                    if let rekey = try? decoder.decode(SEPRekey.self, from: data) {
                        self.applyRekey(rekey, to: groupID)
                    }
                case SEPRemovalNotice.messageType:
                    // Non-secret removal notice — check if we were removed
                    if let notice = try? decoder.decode(SEPRemovalNotice.self, from: data) {
                        let selfRemoved = notice.removedMemberKeys.contains { removed in
                            (try? self.keyManager.blsPublicKey) == removed
                        }
                        if selfRemoved {
                            // Use BLS pubkey from the message wrapper (extracted by transport)
                            let removerBlsHex = senderBlsPubkeyHex
                            if let idx = self.groups.firstIndex(where: { $0.id == groupID }) {
                                var g = self.groups[idx]
                                // Capture epoch snapshot BEFORE mutation so fork can find all members
                                self.captureEpochSnapshot(group: g, changeDescription: "Member removed (you)")
                                // Update epoch and remove self from member list
                                g.epoch = notice.epoch
                                g.removedByPubkeyHex = removerBlsHex
                                if let myBls = try? self.keyManager.blsPublicKey {
                                    g.members.removeAll { $0.publicKeyCompressed == myBls }
                                }
                                self.groups[idx] = g
                                self.store.saveGroup(g)
                                #if !DEBUG
                                self.chatTransport.unsubscribe(topic: g.topicTag)
                                #endif
                                self.chatTransport.currentMembers = self.groups.flatMap(\.members)
                            }
                            let removerHex = senderBlsPubkeyHex ?? ""
                            let removerName = self.contactAliasStore?.displayName(for: removerHex) ?? (String(removerHex.prefix(8)) + "...")
                            self.insertSystemMessage(groupID: groupID, text: "You were removed from this group by \(removerName)", event: "self-removed", epoch: notice.epoch)
                            #if DEBUG
                            print("[AppState] Self-removed from group=\(groupID.prefix(8)) epoch=\(notice.epoch)")
                            #endif
                        }
                        // Remaining members will receive the rekey via inbox — no action needed here
                    }
                case SEPRekeyAck.messageType:
                    // Handle rekey ack — remove sender from unacked set; delete when all acked
                    if let ack = try? decoder.decode(SEPRekeyAck.self, from: data) {
                        let senderHex = ack.senderBlsPubkey.map { String(format: "%02x", $0) }.joined()
                        self.store.updatePendingRekeyAck(groupID: groupID, epoch: Int(ack.epoch), ackedMemberHex: senderHex)
                    }
                case SEPRekeyResendRequest.messageType:
                    // Handle resend request — look up stored envelope and re-send to requester's inbox
                    if let req = try? decoder.decode(SEPRekeyResendRequest.self, from: data) {
                        self.handleRekeyResendRequest(req, groupID: groupID)
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
        updateCurrentMembers()
        #if DEBUG
        let keyData = group.encryptionKey.withUnsafeBytes { Data($0) }
        print("[AppState] subscribeGroup id=\(group.id.prefix(8)) epoch=\(group.epoch) key=\(keyData.prefix(6).base64EncodedString()) salt=\(group.salt.prefix(6).base64EncodedString())")
        #endif
        let groupID = group.id
        chatTransport.subscribe(
            topic: group.topicTag,
            groupID: groupID,
            keyResolver: { [weak self] epoch in
                self?.epochKey(groupID: groupID, epoch: epoch)
            },
            defaultKey: group.encryptionKey,
            sinceTimestamp: group.lastEventTimestamp > 0 ? group.lastEventTimestamp : nil
        )
    }

    /// Handle a member_joined announcement: add the joiner via chain-confirmed transition.
    private func handleMemberJoined(_ joined: SEPMemberJoined, groupID: String) async {
        guard let group = groups.first(where: { $0.id == groupID }) else { return }

        // Extract and persist joiner's transport bundle
        if let bundle = joined.transportBundle {
            processTransportBundle(bundle, groupID: groupID)
        }

        // Skip if already a member
        guard !group.members.contains(where: { $0.publicKeyCompressed == joined.member.publicKeyCompressed }) else { return }

        // Capture old encryption key BEFORE the transition
        let previousKey = group.encryptionKey

        let result = await performEpochTransition(
            groupID: groupID,
            kind: .memberAdd(member: joined.member),
            previousKey: previousKey
        )

        #if DEBUG
        print("[AppState] handleMemberJoined result=\(result) group=\(groupID.prefix(8))")
        #endif

        if result != .success {
            SecurityLog.onChainOperationFailed(
                operation: "member_add_transition",
                reason: "\(result)"
            )
        }
    }

    // MARK: - TURN Configuration

    func syncTurnConfig() {
        callManager.turnEnabled = turnEnabled
        callManager.turnURLs = turnURLs
        callManager.turnUsername = turnUsername
        callManager.turnPassword = turnPassword
    }

    // MARK: - Contract Configuration

    private static let contractEndpointKey = "chat.onym.ios.contractEndpoint"
    private static let contractIDKey = "chat.onym.ios.contractID"
    private static let relayerURLKey = "chat.onym.ios.relayerURL"
    private static let relayerAuthTokenKey = "chat.onym.ios.relayerAuthToken"

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
                    // Skip if already in this group or previously declined
                    let groupIDHex = invitation.payload.groupID
                        .map { String(format: "%02x", $0) }.joined()
                    if !self.groups.contains(where: { $0.id == groupIDHex }),
                       !self.declinedGroupIDs.contains(groupIDHex) {
                        self.pendingInvitations.append(invitation)
                    }
                }
            }
        }

        invitationTransport.onRekeyEnvelope = { [weak self] envelope in
            Task { @MainActor in
                guard let self else { return }
                self.applyRekeyEnvelope(envelope)
            }
        }

        invitationTransport.subscribeToInbox(
            inboxTag: keyManager.inboxTag,
            privateKey: keyManager.keyAgreementPrivateKey
        )

        // Start rekey resend timer for reliability
        startRekeyResendTimer()

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

    // MARK: - Push Notifications

    /// Import messages that were decrypted by the Notification Service Extension
    /// while the app was backgrounded. Reads from the shared App Group JSONL file.
    func importPendingPushMessages() {
        let appGroupID = "group.chat.onym.ios"
        guard let container = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: appGroupID
        ) else { return }

        let fileURL = container.appendingPathComponent("pending_pn_messages.jsonl")
        guard let data = try? String(contentsOf: fileURL, encoding: .utf8) else { return }

        var imported = 0
        for line in data.components(separatedBy: "\n") where !line.isEmpty {
            guard let jsonData = line.data(using: .utf8),
                  let entry = try? JSONSerialization.jsonObject(with: jsonData) as? [String: Any],
                  let id = entry["id"] as? String,
                  let groupID = entry["groupID"] as? String,
                  let senderPubkey = entry["senderPubkey"] as? String,
                  let text = entry["text"] as? String,
                  let ts = entry["timestamp"] as? TimeInterval else { continue }

            // Skip if already in memory (dedup)
            if seenMessageIDs[groupID]?.contains(id) == true { continue }

            let epoch = entry["epoch"] as? UInt64
            let msg = ChatMessage(
                id: id,
                groupID: groupID,
                senderPubkey: senderPubkey,
                text: text,
                timestamp: Date(timeIntervalSince1970: ts),
                isMine: false,
                epoch: epoch
            )

            // Add to in-memory state
            var msgs = chatMessages[groupID, default: []]
            msgs.append(msg)
            chatMessages[groupID] = msgs
            seenMessageIDs[groupID, default: []].insert(id)

            // Persist
            store.saveMessageAsync(msg)
            imported += 1
        }

        // Clear the file after import
        try? FileManager.default.removeItem(at: fileURL)

        #if DEBUG
        if imported > 0 {
            print("[Push] Imported \(imported) pending PN messages")
        }
        #endif
    }

    /// Lightweight setup — just configures the relay URL, no permission prompt.
    func configurePushRelay() {
        if let nostrRelay = relayURLs.first,
           let host = nostrRelay.host {
            let domain = host.replacingOccurrences(of: "nostr.", with: "")
            pushManager.relayURL = URL(string: "https://push.\(domain)")
        }
        // Store local BLS pubkey in shared App Group so NSE can filter own messages
        if let blsPub = try? keyManager.blsPublicKey {
            pushManager.subscriptionStore.setLocalBlsPubkey(blsPub.base64EncodedString())
        }
    }

    /// Request notification permission, wait for APNs token, and register a group.
    /// Called when the user first enables push for a group.
    func ensurePushPermissionAndRegister(group: ChatGroup) async {
        do {
            let center = UNUserNotificationCenter.current()
            let granted = try await center.requestAuthorization(options: [.alert, .sound, .badge])
            guard granted else { return }

            UIApplication.shared.registerForRemoteNotifications()

            // Wait for APNs token (or timeout after 10s)
            if pushManager.apnsToken == nil {
                await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
                    nonisolated(unsafe) var didResume = false
                    let observer = NotificationCenter.default.addObserver(
                        forName: .apnsTokenReceived,
                        object: nil,
                        queue: .main
                    ) { _ in
                        guard !didResume else { return }
                        didResume = true
                        continuation.resume()
                    }
                    Task { @MainActor in
                        try? await Task.sleep(for: .seconds(10))
                        NotificationCenter.default.removeObserver(observer)
                        guard !didResume else { return }
                        didResume = true
                        continuation.resume()
                    }
                }
            }

            await pushManager.registerGroup(group)
        } catch {
            #if DEBUG
            print("[Push] Authorization error: \(error)")
            #endif
        }
    }

    // MARK: - Relay Persistence

    private static let relayURLsKey = "chat.onym.ios.relayURLs"

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

    private static let blossomServerURLsKey = "chat.onym.ios.blossomServerURLs"
    private static let defaultBlossomServers: [URL] = {
        var servers: [URL] = []
        if let url = URL(string: RelayerDefaults.defaultBlossomServer), !RelayerDefaults.defaultBlossomServer.isEmpty {
            servers.append(url)
        }
        servers.append(URL(string: "https://nostr.download")!)
        return servers
    }()

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
