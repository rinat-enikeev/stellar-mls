import SwiftUI
import SwiftMLS

@main
struct StellarChatApp: App {
    @State private var appState = AppState()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(appState)
        }
    }
}

@Observable
final class AppState {
    var keyManager: KeyManager
    var groups: [ChatGroup] = []
    let store: PersistenceStore
    var relayURLs: [URL] = [
        URL(string: "wss://relay.damus.io")!,
        URL(string: "wss://nos.lol")!,
    ]

    init() {
        self.keyManager = KeyManager()
        self.store = try! PersistenceStore()
        self.groups = store.loadGroups()
    }

    func addGroup(_ group: ChatGroup) {
        groups.append(group)
        store.saveGroup(group)
    }

    func removeGroup(id: String) {
        groups.removeAll { $0.id == id }
        store.deleteGroup(id: id)
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
}
