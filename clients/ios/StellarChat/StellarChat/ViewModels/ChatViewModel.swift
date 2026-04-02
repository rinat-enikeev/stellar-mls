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

    /// ID of the first unread message when the chat was opened, for showing a separator.
    var firstUnreadMessageID: String?

    init(groupID: String, appState: AppState) {
        self.groupID = groupID
        self.appState = appState
        // Capture the first unread message before clearing the count
        let unreadCount = appState.unreadCounts[groupID] ?? 0
        let msgs = appState.chatMessages[groupID] ?? []
        if unreadCount > 0 && msgs.count >= unreadCount {
            firstUnreadMessageID = msgs[msgs.count - unreadCount].id
        }
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

    func retryMessage(id: String) {
        appState?.retryMessage(groupID: groupID, messageID: id)
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
