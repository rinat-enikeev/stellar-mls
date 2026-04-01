import CryptoKit
import Foundation
import SwiftMLS

/// Sends and receives encrypted group messages over Nostr relays.
@Observable
final class NostrMessageTransport {
    private var connections: [URL: NostrRelayConnection] = [:]
    private var activeSubscriptions: [String: Task<Void, Never>] = [:]

    /// Callback for received and decrypted plain-text chat messages.
    /// Parameters: (plaintext, event, senderVerified: true if BLS pubkey is in member list)
    var onMessage: ((String, NostrEvent) -> Void)?
    /// Callback for received protocol messages (state updates, salt requests/responses).
    var onProtocolMessage: ((String, NostrEvent) -> Void)?
    /// Callback for transport-level errors (decryption, relay, encoding).
    var onError: ((String) -> Void)?

    /// Current group members used for sender authentication (H-4).
    var currentMembers: [SEPGroupMemberLeaf] = []

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
    /// Subscribe to messages for a group topic on all connected relays.
    /// - Parameter sinceTimestamp: Unix timestamp for catch-up. Defaults to 5 minutes ago.
    func subscribe(topic: String, groupID: String, key: SymmetricKey, sinceTimestamp: Int64? = nil) {
        let subID = "chat-\(topic)"

        // Cancel existing subscription for this topic
        activeSubscriptions[subID]?.cancel()

        let since = sinceTimestamp ?? (Int64(Date().timeIntervalSince1970) - 300)

        let task = Task { [weak self] in
            guard let self else { return }
            for (_, conn) in self.connections {
                let filter: [String: Any] = [
                    "kinds": [24114],
                    "#t": [topic],
                    "since": since,
                ]
                let stream = await conn.subscribe(subscriptionID: subID, filter: filter)
                for await event in stream {
                    guard !Task.isCancelled else { break }

                    guard let envelopeData = Data(base64Encoded: event.content) else {
                        self.onError?("Failed to decode base64 event content")
                        continue
                    }
                    guard let envelope = try? JSONDecoder().decode(SealedEnvelope.self, from: envelopeData) else {
                        self.onError?("Failed to decode sealed envelope")
                        continue
                    }
                    do {
                        let plaintext = try GroupCrypto.decrypt(envelope, key: key)
                        // Distinguish protocol messages from plain-text chat
                        if SEPProtocolMessage.parse(plaintext) != nil {
                            self.onProtocolMessage?(plaintext, event)
                        } else {
                            // Sender authentication (H-4): verify BLS pubkey is in member list
                            if let data = plaintext.data(using: .utf8),
                               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                               let blsPubkeyB64 = json["senderBlsPubkey"] as? String,
                               let blsPubkey = Data(base64Encoded: blsPubkeyB64),
                               let text = json["text"] as? String {
                                // Check membership
                                let isMember = self.currentMembers.contains { $0.publicKeyCompressed == blsPubkey }
                                if isMember {
                                    self.onMessage?(text, event)
                                } else {
                                    SecurityLog.nonMemberMessageRejected(groupID: groupID)
                                self.onError?("Message from non-member BLS key — rejected")
                                }
                            } else {
                                // N-6: Reject messages without BLS sender authentication.
                                // Legacy unverified messages are no longer accepted to prevent
                                // bypass of H-4 sender authentication.
                                SecurityLog.nonMemberMessageRejected(groupID: groupID)
                                self.onError?("Message rejected: missing sender authentication")
                            }
                        }
                    } catch {
                        SecurityLog.decryptionFailed(context: "group message")
                        self.onError?("Decryption failed: \(error.localizedDescription)")
                    }
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
        try await sendRaw(text, topic: topic, key: key, keyManager: keyManager)
    }

    /// Send a protocol message (state update, salt request/response) to a group.
    func sendProtocolMessage<T: Encodable>(
        _ message: T,
        topic: String,
        key: SymmetricKey,
        keyManager: KeyManager
    ) async throws {
        let json = try JSONEncoder().encode(message)
        let text = String(data: json, encoding: .utf8)!
        try await sendRaw(text, topic: topic, key: key, keyManager: keyManager)
    }

    private func sendRaw(
        _ text: String,
        topic: String,
        key: SymmetricKey,
        keyManager: KeyManager
    ) async throws {
        // Wrap text with sender BLS pubkey for receiver-side membership verification (H-4)
        let authenticatedText: String
        if let blsPubkey = try? keyManager.blsPublicKey {
            let wrapper: [String: Any] = [
                "text": text,
                "senderBlsPubkey": blsPubkey.base64EncodedString()
            ]
            if let data = try? JSONSerialization.data(withJSONObject: wrapper),
               let json = String(data: data, encoding: .utf8) {
                authenticatedText = json
            } else {
                authenticatedText = text
            }
        } else {
            authenticatedText = text
        }
        let envelope = try GroupCrypto.encrypt(authenticatedText, key: key)
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()

        let tags: [[String]] = [
            ["t", topic],
        ]

        let event = try NostrEvent.build(
            kind: 24114,
            tags: tags,
            content: content,
            keyManager: keyManager
        )

        var published = false
        for conn in connections.values {
            do {
                try await conn.publish(event: event)
                published = true
            } catch {
                onError?("Relay \(await conn.url.host ?? "unknown") publish failed: \(error.localizedDescription)")
            }
        }

        if !published && !connections.isEmpty {
            throw ChatError.relayPublishFailed
        }
    }
}
