import SwiftUI

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
    var relayURLs: [URL] = [
        URL(string: "wss://relay.damus.io")!,
        URL(string: "wss://nos.lol")!,
    ]

    init() {
        self.keyManager = KeyManager()
    }

    func addGroup(_ group: ChatGroup) {
        groups.append(group)
    }

    func removeGroup(id: String) {
        groups.removeAll { $0.id == id }
    }
}
