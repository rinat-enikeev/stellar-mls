import Foundation

@Observable @MainActor
final class ChatViewModel {
    let groupID: String
    var inputText = ""
    var errorMessage: String?
    private weak var appState: AppState?

    var messages: [ChatMessage] {
        appState?.chatMessages[groupID] ?? []
    }

    var group: ChatGroup? {
        appState?.groups.first(where: { $0.id == groupID })
    }

    init(groupID: String, appState: AppState) {
        self.groupID = groupID
        self.appState = appState
    }

    func sendMessage() async {
        let text = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }

        do {
            try await appState?.sendMessage(text: text, groupID: groupID)
            inputText = ""
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func dismissError() {
        errorMessage = nil
    }
}
