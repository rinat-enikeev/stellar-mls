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
        }
    }
}

@Observable
final class AppState {
    var keyManager: KeyManager
    var groups: [ChatGroup] = []
    let store: PersistenceStore
    let invitationTransport = InvitationTransport()
    var pendingInvitations: [PendingInvitation] = []
    var relayURLs: [URL] {
        didSet { Self.persistRelayURLs(relayURLs) }
    }

    private static let defaultRelays: [URL] = [
        URL(string: "wss://relay.damus.io")!,
        URL(string: "wss://nos.lol")!,
    ]

    init() {
        self.keyManager = KeyManager()
        self.store = try! PersistenceStore()
        self.groups = store.loadGroups()
        self.relayURLs = Self.loadRelayURLs()
    }

    func addGroup(_ group: ChatGroup) {
        groups.append(group)
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
            relayHints: relayURLs.map(\.absoluteString)
        )
        return (group, code.encode())
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
}
