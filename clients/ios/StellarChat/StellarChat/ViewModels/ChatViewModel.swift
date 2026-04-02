import Foundation
import SwiftUI

@Observable @MainActor
final class ChatViewModel {
    let groupID: String
    var inputText = ""
    var errorMessage: String?
    var selectedImageData: Data?
    var isSendingImage = false
    private weak var appState: AppState?

    var messages: [ChatMessage] {
        appState?.chatMessages[groupID] ?? []
    }

    var group: ChatGroup? {
        appState?.groups.first(where: { $0.id == groupID })
    }

    var hasBlossomServers: Bool {
        !(appState?.blossomServerURLs.isEmpty ?? true)
    }

    init(groupID: String, appState: AppState) {
        self.groupID = groupID
        self.appState = appState
        // Mark this group as active and clear unread count
        appState.activeGroupID = groupID
        appState.unreadCounts[groupID] = 0
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

    func sendImage() async {
        guard let imageData = selectedImageData else { return }
        isSendingImage = true
        defer { isSendingImage = false }

        do {
            try await appState?.sendImage(imageData: imageData, groupID: groupID)
            selectedImageData = nil
        } catch {
            print("[Blossom] sendImage failed: \(error)")
            errorMessage = error.localizedDescription
        }
    }

    func dismissError() {
        errorMessage = nil
    }

    func onDisappear() {
        if appState?.activeGroupID == groupID {
            appState?.activeGroupID = nil
        }
    }
}
