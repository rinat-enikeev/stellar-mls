import Foundation
import SwiftUI

@Observable @MainActor
final class ChatViewModel {
    let groupID: String
    var inputText = ""
    var errorMessage: String?
    var selectedImageData: Data?
    var isSendingImage = false
    var selectedVideoURL: URL?
    var isSendingVideo = false
    var isSendingVoice = false
    /// The message currently being replied to, or nil.
    var replyingToMessage: ChatMessage?
    private weak var appState: AppState?

    var messages: [ChatMessage] {
        let all = appState?.chatMessages[groupID] ?? []
        guard let pinned = appState?.groups.first(where: { $0.id == groupID })?.pinnedEpoch else {
            return all
        }
        return all.filter { $0.epoch == pinned || $0.isSystemMessage }
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
        // Flush ACKs for messages that arrived while the chat was closed so the sender sees ✓✓.
        appState.sendBacklogAcks(groupID: groupID, count: unreadCount)
        // Mark this group as active and clear unread count
        appState.activeGroupID = groupID
        appState.unreadCounts[groupID] = 0
    }

    func sendMessage() async {
        let text = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }

        let replyToID = replyingToMessage?.id
        do {
            try await appState?.sendMessage(text: text, groupID: groupID, replyToID: replyToID)
            inputText = ""
            replyingToMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func sendImage() async {
        guard let imageData = selectedImageData else { return }
        isSendingImage = true
        defer { isSendingImage = false }

        let replyToID = replyingToMessage?.id
        do {
            try await appState?.sendImage(imageData: imageData, groupID: groupID, replyToID: replyToID)
            selectedImageData = nil
            replyingToMessage = nil
        } catch {
            print("[Blossom] sendImage failed: \(error)")
            errorMessage = error.localizedDescription
        }
    }

    func sendVideo() async {
        guard let videoURL = selectedVideoURL else { return }
        isSendingVideo = true
        defer { isSendingVideo = false }

        let replyToID = replyingToMessage?.id
        do {
            try await appState?.sendVideo(videoURL: videoURL, groupID: groupID, replyToID: replyToID)
            selectedVideoURL = nil
            replyingToMessage = nil
        } catch {
            print("[Blossom] sendVideo failed: \(error)")
            errorMessage = error.localizedDescription
        }
    }

    func sendVoice(audioURL: URL) async {
        isSendingVoice = true
        defer { isSendingVoice = false }

        let replyToID = replyingToMessage?.id
        do {
            try await appState?.sendVoice(audioURL: audioURL, groupID: groupID, replyToID: replyToID)
            replyingToMessage = nil
        } catch {
            print("[Blossom] sendVoice failed: \(error)")
            errorMessage = error.localizedDescription
        }
    }

    func parentMessage(for message: ChatMessage) -> ChatMessage? {
        guard let replyToID = message.replyToID else { return nil }
        return messages.first(where: { $0.id == replyToID })
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
