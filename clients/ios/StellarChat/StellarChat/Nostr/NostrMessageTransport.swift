import CryptoKit
import Foundation

/// Sends and receives encrypted group messages over Nostr relays.
@Observable
final class NostrMessageTransport {
    private var connections: [URL: NostrRelayConnection] = [:]
    private var activeSubscriptions: [String: Task<Void, Never>] = [:]

    /// Callback for received messages.
    var onMessage: ((String, NostrEvent) -> Void)?

    func connect(to urls: [URL]) async {
        for url in urls {
            if connections[url] == nil {
                let conn = NostrRelayConnection(url: url)
                connections[url] = conn
                await conn.connect()
            }
        }
    }

    func disconnect() async {
        for task in activeSubscriptions.values {
            task.cancel()
        }
        activeSubscriptions.removeAll()
        for conn in connections.values {
            await conn.disconnect()
        }
        connections.removeAll()
    }

    /// Subscribe to messages for a group topic on all connected relays.
    func subscribe(topic: String, groupID: String, key: SymmetricKey) {
        let subID = "chat-\(topic)"

        // Cancel existing subscription for this topic
        activeSubscriptions[subID]?.cancel()

        let task = Task { [weak self] in
            guard let self else { return }
            for (_, conn) in self.connections {
                let filter: [String: Any] = [
                    "kinds": [24114],
                    "#sep_topic": [topic],
                ]
                let stream = await conn.subscribe(subscriptionID: subID, filter: filter)
                for await event in stream {
                    guard !Task.isCancelled else { break }
                    // Decode and decrypt the message
                    guard let envelopeData = Data(base64Encoded: event.content),
                          let envelope = try? JSONDecoder().decode(SealedEnvelope.self, from: envelopeData),
                          let plaintext = try? GroupCrypto.decrypt(envelope, key: key)
                    else { continue }
                    self.onMessage?(plaintext, event)
                }
            }
        }
        activeSubscriptions[subID] = task
    }

    func unsubscribe(topic: String) {
        let subID = "chat-\(topic)"
        activeSubscriptions[subID]?.cancel()
        activeSubscriptions.removeValue(forKey: subID)
    }

    /// Send an encrypted message to a group.
    func send(
        text: String,
        topic: String,
        key: SymmetricKey,
        keyManager: KeyManager
    ) async throws {
        let envelope = try GroupCrypto.encrypt(text, key: key)
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()

        let tags: [[String]] = [
            ["sep_topic", topic],
            ["sep_version", "1"],
        ]

        let event = NostrEvent.build(
            kind: 24114,
            tags: tags,
            content: content,
            keyManager: keyManager
        )

        for conn in connections.values {
            try? await conn.publish(event: event)
        }
    }
}
