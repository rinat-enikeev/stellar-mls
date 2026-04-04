import CryptoKit
import Foundation
import SwiftMLS

/// NostrMessageTransport: handles kind 44114 (topic-addressed, group broadcast).
/// Do NOT use this transport for kind 34113 (inbox) — use InvitationTransport.
@Observable
final class NostrMessageTransport {
    private var connections: [URL: NostrRelayConnection] = [:]
    private var activeSubscriptions: [String: Task<Void, Never>] = [:]
    /// Counter to generate unique relay subscription IDs, preventing race
    /// conditions where an old stream's onTermination CLOSE kills a new REQ.
    private var subscriptionGeneration: UInt64 = 0

    /// Whether at least one relay connection is active.
    var isAnyRelayConnected: Bool {
        get async {
            for conn in connections.values {
                if await conn.isConnected { return true }
            }
            return false
        }
    }

    /// Callback for received and decrypted plain-text chat messages.
    /// Parameters: (plaintext, event, senderVerified: true if BLS pubkey is in member list)
    var onMessage: ((String, NostrEvent) -> Void)?
    /// Callback for received image messages with media attachment metadata.
    /// Parameters: (text fallback, media attachment, event)
    var onImageMessage: ((String, MediaAttachment, NostrEvent) -> Void)?
    /// Callback for received protocol messages (state updates, salt requests/responses).
    var onProtocolMessage: ((String, NostrEvent) -> Void)?
    /// Callback for received call signaling messages (offer, answer, ice, hangup, busy, reject).
    /// Parameters: (groupID, call JSON dict, senderBlsPubkey, event)
    var onCallSignal: ((String, [String: Any], Data, NostrEvent) -> Void)?
    /// Callback for transport-level errors (decryption, relay, encoding).
    var onError: ((String) -> Void)?
    /// Callback for relay OK responses: (eventID, accepted). Used for delivery status.
    var onOK: ((String, Bool) -> Void)?

    /// Current group members used for sender authentication (H-4).
    var currentMembers: [SEPGroupMemberLeaf] = []

    /// Number of connected relays — observable for connection status UI.
    var connectedRelayCount: Int { connections.count }

    func connect(to urls: [URL]) async {
        for url in urls {
            if connections[url] == nil {
                let conn = NostrRelayConnection(url: url)
                await conn.setOnOK { [weak self] eventID, accepted in
                    guard let self else { return }
                    self.onOK?(eventID, accepted)
                }
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

        // Use an overlap window of 60 seconds to handle clock skew across relays
        let since = sinceTimestamp.map { max(0, $0 - 60) } ?? (Int64(Date().timeIntervalSince1970) - 300)

        let conns = Array(connections.values)
        let task = Task { [weak self] in
            guard let self else { return }
            await withTaskGroup(of: Void.self) { taskGroup in
                for conn in conns {
                    taskGroup.addTask { [weak self] in
                        let filter: [String: Any] = [
                            "kinds": [44114, 24114],
                            "#t": [topic],
                            "since": since,
                        ]
                        let stream = await conn.subscribe(subscriptionID: subID, filter: filter)
                        for await event in stream {
                            guard !Task.isCancelled else { break }
                            guard let self else { break }
                            // Spawn a detached task per event so crypto/JSON parsing
                            // runs concurrently and doesn't block the relay stream.
                            Task.detached { [weak self] in
                                self?.handleIncomingEvent(event, groupID: groupID, key: key)
                            }
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
            return
        }
        guard let envelope = try? JSONDecoder().decode(SealedEnvelope.self, from: envelopeData) else {
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
                return
            }

            let wrapperType = wrapperJSON["type"] as? String

            if wrapperType == "call" {
                // Call signaling — verify sender is a group member
                let isMember = currentMembers.contains { $0.publicKeyCompressed == blsPubkey }
                if isMember, let callDict = wrapperJSON["call"] as? [String: Any] {
                    // Reject stale signaling (>60 seconds old)
                    let ts = wrapperJSON["ts"] as? Int64 ?? 0
                    let age = Int64(Date().timeIntervalSince1970) - ts
                    if age <= 60 {
                        onCallSignal?(groupID, callDict, blsPubkey, event)
                    }
                } else if !isMember {
                    SecurityLog.nonMemberMessageRejected(groupID: groupID)
                }
            } else if wrapperType == "image" || wrapperType == "video" || wrapperType == "audio" {
                // Parse media attachment from wrapper JSON
                let isMember = currentMembers.contains { $0.publicKeyCompressed == blsPubkey }
                if isMember, let mediaDict = wrapperJSON["media"] as? [String: Any],
                   let blobHash = mediaDict["blobHash"] as? String,
                   let fileKeyB64 = mediaDict["fileKey"] as? String,
                   let fileKey = Data(base64Encoded: fileKeyB64),
                   let mimeType = mediaDict["mimeType"] as? String,
                   let width = mediaDict["width"] as? Int,
                   let height = mediaDict["height"] as? Int,
                   let size = mediaDict["size"] as? Int,
                   let blossomServers = mediaDict["blossomServers"] as? [String] {
                    let encryptedThumbnail: Data?
                    if let thumbB64 = (mediaDict["thumbnail"] as? String) ?? (mediaDict["encryptedThumbnail"] as? String) {
                        encryptedThumbnail = Data(base64Encoded: thumbB64)
                    } else {
                        encryptedThumbnail = nil
                    }
                    let duration = mediaDict["duration"] as? Int
                    let media = MediaAttachment(
                        blobHash: blobHash,
                        fileKey: fileKey,
                        mimeType: mimeType,
                        width: width,
                        height: height,
                        size: size,
                        blossomServers: blossomServers,
                        encryptedThumbnail: encryptedThumbnail,
                        duration: duration
                    )
                    onImageMessage?(innerText, media, event)
                } else if !isMember {
                    SecurityLog.nonMemberMessageRejected(groupID: groupID)
                }
            } else if SEPProtocolMessage.parse(innerText) != nil {
                applyMemberChanges(from: innerText)
                onProtocolMessage?(innerText, event)
            } else {
                let isMember = currentMembers.contains { $0.publicKeyCompressed == blsPubkey }
                if isMember {
                    onMessage?(innerText, event)
                } else {
                    SecurityLog.nonMemberMessageRejected(groupID: groupID)
                }
            }
        } catch {
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

        // SEPRemovalNotice — strip removed members immediately so BLS rejects their messages
        if let notice = try? decoder.decode(SEPRemovalNotice.self, from: data) {
            for removed in notice.removedMemberKeys {
                currentMembers.removeAll { $0.publicKeyCompressed == removed }
            }
        }
    }

    func unsubscribe(topic: String) {
        let topicKey = "chat-\(topic)"
        activeSubscriptions[topicKey]?.cancel()
        activeSubscriptions.removeValue(forKey: topicKey)
    }

    /// Send an encrypted message to a group. Returns the published NostrEvent (with deterministic NIP-01 ID).
    @discardableResult
    func send(
        text: String,
        topic: String,
        key: SymmetricKey,
        keyManager: KeyManager
    ) async throws -> NostrEvent {
        return try await sendRaw(text, topic: topic, key: key, keyManager: keyManager)
    }

    /// Send a pre-built wrapper JSON (e.g. image messages with custom fields) encrypted to the group.
    /// Returns the published NostrEvent.
    @discardableResult
    func sendWrapper(
        _ wrapperJSON: String,
        topic: String,
        key: SymmetricKey,
        keyManager: KeyManager
    ) async throws -> NostrEvent {
        let envelope = try GroupCrypto.encrypt(wrapperJSON, key: key)
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()

        let tags: [[String]] = [["t", topic]]
        let event = try NostrEvent.build(
            kind: 44114,
            tags: tags,
            content: content,
            keyManager: keyManager
        )

        try await publishToRelays(event)
        return event
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

    @discardableResult
    private func sendRaw(
        _ text: String,
        topic: String,
        key: SymmetricKey,
        keyManager: KeyManager
    ) async throws -> NostrEvent {
        // v2 wrapper: includes version, type discriminator, message ID, and sender timestamp
        let blsPubkey = try keyManager.blsPublicKey
        let ts = Int64(Date().timeIntervalSince1970)
        let wrapper: [String: Any] = [
            "v": 2,
            "type": "chat",
            "text": text,
            "senderBlsPubkey": blsPubkey.base64EncodedString(),
            "ts": ts
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
            kind: 44114,
            tags: tags,
            content: content,
            keyManager: keyManager
        )

        try await publishToRelays(event)
        return event
    }

    /// Publish a pre-built event to all connected relays concurrently.
    /// Succeeds if at least one relay accepts. Throws if all fail or no connections exist.
    /// - Precondition: event.kind must be 44114 (group). Use InvitationTransport for 34113.
    func publishToRelays(_ event: NostrEvent) async throws {
        assert(event.kind == 44114 || event.kind == 24114,
               "MessageTransport received non-chat event kind \(event.kind) — use InvitationTransport for inbox events")
        let conns = Array(connections.values)
        guard !conns.isEmpty else {
            throw ChatError.relayPublishFailed
        }

        let published = await withTaskGroup(of: Bool.self) { group in
            for conn in conns {
                group.addTask {
                    do {
                        try await conn.publish(event: event)
                        return true
                    } catch {
                        return false
                    }
                }
            }
            var anyPublished = false
            for await result in group {
                if result { anyPublished = true }
            }
            return anyPublished
        }

        if !published {
            throw ChatError.relayPublishFailed
        }
    }
}
