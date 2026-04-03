import CryptoKit
import Foundation

/// Sends and receives kind 24113 invitation events over Nostr relays.
@Observable
final class InvitationTransport {
    private static let primaryKind = 34113
    private static let legacyKind = 24113
    private static let primaryInboxFilterKey = "#d"
    private static let primaryInboxEventTagKey = "d"
    private static let secondaryInboxEventTagKey = "t"
    private static let legacyInboxEventTagKey = "sep_inbox"
    private static let primaryInboxTagPrefix = "sep-inbox:"

    private var connections: [URL: NostrRelayConnection] = [:]
    private var subscriptionTask: Task<Void, Never>?

    /// Called when an invitation is successfully decrypted.
    var onInvitation: ((PendingInvitation) -> Void)?
    /// Called on transport-level errors.
    var onError: ((String) -> Void)?

    static func subscriptionFilters(inboxTag: String) -> [[String: Any]] {
        [
            [
                "kinds": [primaryKind],
                primaryInboxFilterKey: [primaryInboxTagPrefix + inboxTag],
            ],
            [
                "kinds": [primaryKind],
                "#t": [inboxTag],
            ],
            [
                "kinds": [legacyKind],
                "#t": [inboxTag],
            ],
        ]
    }

    static func eventTags(recipientInboxTag: String) -> [[String]] {
        [
            // Use a parameterized-replaceable `d` tag on an addressable kind so
            // relays can retain and query invitations across reconnects.
            [primaryInboxEventTagKey, primaryInboxTagPrefix + recipientInboxTag],
            [secondaryInboxEventTagKey, recipientInboxTag],
            [legacyInboxEventTagKey, recipientInboxTag],
            ["sep_version", "1"],
        ]
    }

    func connect(to urls: [URL]) async {
        for url in urls {
            if connections[url] == nil {
                let conn = NostrRelayConnection(url: url)
                connections[url] = conn
                await conn.connect()
            }
        }
        #if DEBUG
        print("[Invite] connect relays=\(urls.map(\.absoluteString))")
        #endif
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
            let filters = Self.subscriptionFilters(inboxTag: inboxTag)
            await withTaskGroup(of: Void.self) { group in
                for (_, conn) in self.connections {
                    for (index, filter) in filters.enumerated() {
                        group.addTask {
                            let filterSubID = "\(subID)-\(index)"
                            let stream = await conn.subscribe(subscriptionID: filterSubID, filter: filter)
                            for await event in stream {
                                guard !Task.isCancelled else { break }
                                self.handleInvitationEvent(event, privateKey: privateKey)
                            }
                        }
                    }
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

        let tags = Self.eventTags(recipientInboxTag: recipientInboxTag)
        #if DEBUG
        print("[Invite] send recipientPubkey=\(recipientKeyAgreementPubkey.prefix(12).map { String(format: "%02x", $0) }.joined()) recipientInboxTag=\(recipientInboxTag) kind=\(Self.primaryKind) tags=\(tags)")
        #endif

        let event = try NostrEvent.build(
            kind: Self.primaryKind,
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
        #if DEBUG
        let tagSummary = event.tags.map { $0.joined(separator: ":") }.joined(separator: ",")
        print("[Invite] Received event id=\(event.id.prefix(12)) kind=\(event.kind) tags=\(tagSummary)")
        #endif
        guard let envelopeData = Data(base64Encoded: event.content) else {
            #if DEBUG
            print("[Invite] Base64 decode failed for event \(event.id.prefix(12))")
            #endif
            onError?("Failed to decode invitation base64")
            return
        }
        guard let envelope = try? JSONDecoder().decode(SealedEnvelope.self, from: envelopeData) else {
            #if DEBUG
            print("[Invite] Envelope decode failed for event \(event.id.prefix(12))")
            #endif
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
            #if DEBUG
            print("[Invite] Ignored event \(event.id.prefix(12)): \(error.localizedDescription)")
            #endif
        }
    }
}
