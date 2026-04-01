import CryptoKit
import Foundation

@Observable
final class ChatViewModel {
    let group: ChatGroup
    var messages: [ChatMessage] = []
    var inputText = ""
    var errorMessage: String?
    private let transport: NostrMessageTransport
    private let keyManager: KeyManager
    private var seenIDs: Set<String> = []

    init(group: ChatGroup, transport: NostrMessageTransport, keyManager: KeyManager) {
        self.group = group
        self.transport = transport
        self.keyManager = keyManager

        // Load persisted messages
        let persisted = PersistenceStore.loadMessages(groupID: group.id)
        self.messages = persisted
        self.seenIDs = Set(persisted.map(\.id))

        transport.onMessage = { [weak self] plaintext, event in
            guard let self else { return }
            let msg = ChatMessage(
                id: event.id,
                groupID: group.id,
                senderPubkey: event.pubkey,
                text: plaintext,
                timestamp: Date(timeIntervalSince1970: TimeInterval(event.createdAt)),
                isMine: event.pubkey == keyManager.publicKeyHex
            )
            Task { @MainActor in
                if !self.seenIDs.contains(msg.id) {
                    self.seenIDs.insert(msg.id)
                    self.messages.append(msg)
                    self.messages.sort { $0.timestamp < $1.timestamp }
                    self.persistMessages()
                }
            }
        }

        transport.onError = { [weak self] error in
            Task { @MainActor in
                self?.errorMessage = error
            }
        }
    }

    func startListening(relayURLs: [URL]) async {
        await transport.connect(to: relayURLs)
        transport.subscribe(
            topic: group.topicTag,
            groupID: group.id,
            key: group.encryptionKey
        )
    }

    func sendMessage() async {
        let text = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }

        do {
            try await transport.send(
                text: text,
                topic: group.topicTag,
                key: group.encryptionKey,
                keyManager: keyManager
            )

            let msg = ChatMessage(
                id: UUID().uuidString,
                groupID: group.id,
                senderPubkey: keyManager.publicKeyHex,
                text: text,
                timestamp: Date(),
                isMine: true
            )
            messages.append(msg)
            seenIDs.insert(msg.id)
            inputText = ""
            persistMessages()
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func stopListening() async {
        transport.unsubscribe(topic: group.topicTag)
    }

    func dismissError() {
        errorMessage = nil
    }

    private func persistMessages() {
        PersistenceStore.saveMessages(messages, groupID: group.id)
    }
}
