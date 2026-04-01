import CryptoKit
import Foundation
import SwiftMLS

@Observable
final class ChatViewModel {
    let group: ChatGroup
    var messages: [ChatMessage] = []
    var inputText = ""
    var errorMessage: String?
    private let transport: NostrMessageTransport
    private let keyManager: KeyManager
    private let store: PersistenceStore
    private weak var appState: AppState?
    private var seenIDs: Set<String> = []
    /// Tracks processed protocol event IDs to prevent replay (H-7).
    /// N-8: Bounded to prevent unbounded memory growth.
    private var processedProtocolEventIDs: Set<String> = []
    /// Tracks (senderPubkey, epoch) pairs for salt request rate limiting (H-5).
    private var saltRequestsResponded: Set<String> = []
    /// N-8: Max entries for dedup sets.
    private static let maxDedupSetSize = 10_000

    init(group: ChatGroup, transport: NostrMessageTransport, keyManager: KeyManager, store: PersistenceStore, appState: AppState? = nil) {
        self.group = group
        self.transport = transport
        self.keyManager = keyManager
        self.store = store
        self.appState = appState

        // Load persisted messages
        let persisted = store.loadMessages(groupID: group.id)
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
                    self.store.saveMessage(msg)
                    // Track latest event for offline catch-up
                    if event.createdAt > self.group.lastEventTimestamp {
                        self.appState?.updateLastEventTimestamp(groupID: group.id, timestamp: event.createdAt)
                    }
                }
            }
        }

        // Handle protocol messages (state updates, salt requests/responses)
        transport.onProtocolMessage = { [weak self] json, event in
            guard let self, let appState = self.appState,
                  let data = json.data(using: .utf8) else { return }

            Task { @MainActor in
                // Replay protection: skip already-processed protocol events (H-7)
                guard !self.processedProtocolEventIDs.contains(event.id) else { return }
                // N-8: Evict oldest entries when set exceeds max size
                if self.processedProtocolEventIDs.count >= Self.maxDedupSetSize {
                    self.processedProtocolEventIDs.removeFirst()
                }
                self.processedProtocolEventIDs.insert(event.id)

                let decoder = JSONDecoder()
                let msgType = SEPProtocolMessage.parse(json)

                switch msgType {
                case SEPMemberJoined.messageType:
                    if let joined = try? decoder.decode(SEPMemberJoined.self, from: data) {
                        await appState.handleMemberJoined(joined, groupID: group.id, transport: self.transport, keyManager: self.keyManager)
                    }
                case SEPGroupStateUpdate.messageType:
                    if let update = try? decoder.decode(SEPGroupStateUpdate.self, from: data) {
                        appState.applyStateUpdate(update, to: group.id)
                    }
                case SEPSaltRequest.messageType:
                    // Rate-limit: respond only once per (sender, epoch) pair (H-5)
                    if let request = try? decoder.decode(SEPSaltRequest.self, from: data) {
                        let rateKey = "\(event.pubkey):\(request.epoch)"
                        guard !self.saltRequestsResponded.contains(rateKey) else { break }
                        self.saltRequestsResponded.insert(rateKey)

                        if let salt = appState.getSalt(groupID: group.id, epoch: request.epoch) {
                            let response = SEPSaltResponse(epoch: request.epoch, salt: salt)
                            try? await self.transport.sendProtocolMessage(
                                response,
                                topic: group.topicTag,
                                key: group.encryptionKey,
                                keyManager: self.keyManager
                            )
                        }
                    }
                case SEPSaltResponse.messageType:
                    if let response = try? decoder.decode(SEPSaltResponse.self, from: data) {
                        appState.storeSalt(groupID: group.id, epoch: response.epoch, salt: response.salt)
                    }
                case SEPGroupRenamed.messageType:
                    if let renamed = try? decoder.decode(SEPGroupRenamed.self, from: data) {
                        appState.applyGroupRenamed(renamed, to: group.id)
                    }
                default:
                    break
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
        transport.currentMembers = group.members
        transport.subscribe(
            topic: group.topicTag,
            groupID: group.id,
            key: group.encryptionKey,
            sinceTimestamp: group.lastEventTimestamp > 0 ? group.lastEventTimestamp : nil
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
            store.saveMessage(msg)
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
}
