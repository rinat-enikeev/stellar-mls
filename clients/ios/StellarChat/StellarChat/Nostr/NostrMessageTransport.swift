import CryptoKit
import Foundation
import SwiftMLS

/// Sends and receives encrypted group messages over Nostr relays.
@Observable
final class NostrMessageTransport {
    private var connections: [URL: NostrRelayConnection] = [:]
    private var activeSubscriptions: [String: Task<Void, Never>] = [:]
    /// Counter to generate unique relay subscription IDs, preventing race
    /// conditions where an old stream's onTermination CLOSE kills a new REQ.
    private var subscriptionGeneration: UInt64 = 0

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
        let topicKey = "chat-\(topic)"

        // Cancel existing subscription task for this topic
        activeSubscriptions[topicKey]?.cancel()

        // Unique relay subscription ID prevents old stream's onTermination
        // CLOSE from killing the new subscription on the relay.
        subscriptionGeneration += 1
        let subID = "chat-\(topic)-\(subscriptionGeneration)"

        let since = sinceTimestamp ?? (Int64(Date().timeIntervalSince1970) - 300)

        let conns = Array(connections.values)
        let task = Task { [weak self] in
            guard let self else { return }
            await withTaskGroup(of: Void.self) { taskGroup in
                for conn in conns {
                    taskGroup.addTask { [weak self] in
                        let filter: [String: Any] = [
                            "kinds": [24114],
                            "#t": [topic],
                            "since": since,
                        ]
                        let stream = await conn.subscribe(subscriptionID: subID, filter: filter)
                        for await event in stream {
                            guard !Task.isCancelled else { break }
                            guard let self else { break }
                            self.handleIncomingEvent(event, groupID: groupID, key: key)
                        }
                    }
                }
            }
        }
        activeSubscriptions[topicKey] = task
    }

    /// Process a single incoming Nostr event: decrypt, unwrap BLS, route to chat or protocol handler.
    private func handleIncomingEvent(_ event: NostrEvent, groupID: String, key: SymmetricKey) {
        guard let envelopeData = Data(base64Encoded: event.content) else {
            print("[Transport] base64 decode failed group=\(groupID.prefix(8)) content_start=\(event.content.prefix(40))")
            return
        }
        guard let envelope = try? JSONDecoder().decode(SealedEnvelope.self, from: envelopeData) else {
            print("[Transport] envelope decode failed group=\(groupID.prefix(8))")
            return
        }
        do {
            let plaintext = try GroupCrypto.decrypt(envelope, key: key)
            guard let wrapperData = plaintext.data(using: .utf8),
                  let wrapperJSON = try? JSONSerialization.jsonObject(with: wrapperData) as? [String: Any],
                  let innerText = wrapperJSON["text"] as? String,
                  let blsPubkeyB64 = wrapperJSON["senderBlsPubkey"] as? String,
                  let blsPubkey = Data(base64Encoded: blsPubkeyB64)
            else {
                SecurityLog.nonMemberMessageRejected(groupID: groupID)
                print("[Transport] rejected: missing BLS auth group=\(groupID.prefix(8))")
                return
            }

            let isProtocol = SEPProtocolMessage.parse(innerText) != nil
            print("[Transport] Decrypted OK group=\(groupID.prefix(8)) isProtocol=\(isProtocol) members=\(currentMembers.count)")

            if isProtocol {
                applyMemberChanges(from: innerText)
                onProtocolMessage?(innerText, event)
            } else {
                let isMember = currentMembers.contains { $0.publicKeyCompressed == blsPubkey }
                if isMember {
                    onMessage?(innerText, event)
                } else {
                    print("[Transport] BLS rejected group=\(groupID.prefix(8)) members=\(currentMembers.count)")
                    SecurityLog.nonMemberMessageRejected(groupID: groupID)
                }
            }
        } catch {
            print("[Transport] Decrypt failed group=\(groupID.prefix(8)) err=\(error)")
            SecurityLog.decryptionFailed(context: "group message")
        }
    }

    /// Synchronously update currentMembers from protocol messages so the BLS check
    /// for subsequent chat messages uses the latest member list without waiting for
    /// the async AppState callback to complete.
    private func applyMemberChanges(from json: String) {
        guard let data = json.data(using: .utf8) else { return }
        let decoder = JSONDecoder()

        // SEPMemberJoined — add the new member
        if let joined = try? decoder.decode(SEPMemberJoined.self, from: data) {
            if !currentMembers.contains(where: { $0.publicKeyCompressed == joined.member.publicKeyCompressed }) {
                currentMembers.append(joined.member)
            }
        }

        // SEPGroupStateUpdate — apply added/removed members
        if let update = try? decoder.decode(SEPGroupStateUpdate.self, from: data) {
            for removed in update.removedMemberKeys {
                currentMembers.removeAll { $0.publicKeyCompressed == removed }
            }
            for added in update.addedMembers {
                if !currentMembers.contains(where: { $0.publicKeyCompressed == added.publicKeyCompressed }) {
                    currentMembers.append(added)
                }
            }
        }
    }

    func unsubscribe(topic: String) {
        let topicKey = "chat-\(topic)"
        activeSubscriptions[topicKey]?.cancel()
        activeSubscriptions.removeValue(forKey: topicKey)
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
        let blsPubkey = try keyManager.blsPublicKey
        let wrapper: [String: Any] = [
            "text": text,
            "senderBlsPubkey": blsPubkey.base64EncodedString()
        ]
        let wrapperData = try JSONSerialization.data(withJSONObject: wrapper)
        let authenticatedText = String(data: wrapperData, encoding: .utf8)!
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

        print("[Transport] sendRaw to \(connections.count) relays eventId=\(event.id.prefix(12))")
        var published = false
        for conn in connections.values {
            do {
                try await conn.publish(event: event)
                published = true
            } catch {
                print("[Transport] publish failed relay=\(await conn.url.host ?? "?") err=\(error)")
            }
        }
        print("[Transport] sendRaw done published=\(published)")

        if !published && !connections.isEmpty {
            throw ChatError.relayPublishFailed
        }
    }
}
