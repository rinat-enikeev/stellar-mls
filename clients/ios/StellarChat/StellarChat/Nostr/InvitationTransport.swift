import CryptoKit
import Foundation

/// Sends and receives kind 34113 invitation events over Nostr relays.
@Observable
final class InvitationTransport {
    private var connections: [URL: NostrRelayConnection] = [:]
    private var subscriptionTask: Task<Void, Never>?

    /// Called when an invitation is successfully decrypted.
    var onInvitation: ((PendingInvitation) -> Void)?
    /// Called on transport-level errors.
    var onError: ((String) -> Void)?

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
        subscriptionTask?.cancel()
        subscriptionTask = nil
        for conn in connections.values {
            await conn.disconnect()
        }
        connections.removeAll()
    }

    /// Subscribe to invitation events addressed to our inbox.
    func subscribeToInbox(
        inboxTag: String,
        privateKey: Curve25519.KeyAgreement.PrivateKey
    ) {
        subscriptionTask?.cancel()

        let subID = "inbox-\(inboxTag)"

        subscriptionTask = Task { [weak self] in
            guard let self else { return }
            for (_, conn) in self.connections {
                let filter: [String: Any] = [
                    "kinds": [34113],
                    "#d": ["sep-inbox:" + inboxTag],
                ]
                let stream = await conn.subscribe(subscriptionID: subID, filter: filter)
                for await event in stream {
                    guard !Task.isCancelled else { break }
                    self.handleInvitationEvent(event, privateKey: privateKey)
                }
            }
        }
    }

    /// Send an invitation to a specific recipient.
    func sendInvitation(
        payload: BootstrapPayload,
        recipientKeyAgreementPubkey: Data,
        keyManager: KeyManager
    ) async throws {
        let payloadData = try JSONEncoder().encode(payload)

        let envelope = try GroupCrypto.encryptInvitation(
            payloadData,
            recipientKeyAgreementPubkey: recipientKeyAgreementPubkey,
            senderSigningKey: keyManager.ed25519SigningKey
        )
        let envelopeData = try JSONEncoder().encode(envelope)
        let content = envelopeData.base64EncodedString()

        let recipientInboxTag = GroupCrypto.hiddenInboxTag(
            recipientPublicKey: recipientKeyAgreementPubkey
        )

        let tags: [[String]] = [
            ["d", "sep-inbox:" + recipientInboxTag],
            ["sep_version", "1"],
        ]

        let event = try NostrEvent.build(
            kind: 34113,
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
                onError?("Relay publish failed: \(error.localizedDescription)")
            }
        }

        if !published && !connections.isEmpty {
            throw ChatError.relayPublishFailed
        }
    }

    // MARK: - Private

    private func handleInvitationEvent(
        _ event: NostrEvent,
        privateKey: Curve25519.KeyAgreement.PrivateKey
    ) {
        guard let envelopeData = Data(base64Encoded: event.content) else {
            onError?("Failed to decode invitation base64")
            return
        }
        guard let envelope = try? JSONDecoder().decode(SealedEnvelope.self, from: envelopeData) else {
            onError?("Failed to decode invitation envelope")
            return
        }

        do {
            let payloadData = try GroupCrypto.decryptInvitation(envelope, privateKey: privateKey)
            let payload = try JSONDecoder().decode(BootstrapPayload.self, from: payloadData)
            let invitation = PendingInvitation(
                id: event.id,
                payload: payload,
                receivedAt: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0)
            )
            onInvitation?(invitation)
        } catch {
            // Not for us or corrupted — silently ignore
        }
    }
}
