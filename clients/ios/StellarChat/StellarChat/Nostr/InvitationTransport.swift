import CryptoKit
import Foundation
import SwiftMLS

/// InvitationTransport: handles kind 34113 (inbox-addressed, per-recipient).
/// Do NOT use this transport for kind 44114 (group broadcast) — use NostrMessageTransport.
/// Still listens for legacy kind 24113 events during migration.
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
    /// Called when a rekey envelope is successfully decrypted.
    var onRekeyEnvelope: ((SEPRekeyEnvelope) -> Void)?
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

        let conns = Array(connections.values)
        let accepted = await withTaskGroup(of: Bool.self) { group in
            for conn in conns {
                group.addTask {
                    do {
                        let ok = try await conn.publishAndAwaitOK(event: event)
                        #if DEBUG
                        print("[Invite] publishAndAwaitOK: relay ok=\(ok) eventID=\(event.id.prefix(12))")
                        #endif
                        return ok
                    } catch {
                        #if DEBUG
                        print("[Invite] publishAndAwaitOK failed: \(error)")
                        #endif
                        return false
                    }
                }
            }
            var any = false
            for await result in group {
                if result { any = true }
            }
            return any
        }

        if !accepted {
            throw ChatError.relayPublishFailed
        }
    }

    /// Publish a pre-built event to all connected relays (concurrently).
    /// - Precondition: event.kind must be 34113 (inbox). Use NostrMessageTransport for 44114.
    func publishToRelays(_ event: NostrEvent, relayURLs: [URL] = []) async throws {
        assert(event.kind == Self.primaryKind || event.kind == Self.legacyKind,
               "InvitationTransport received non-inbox event kind \(event.kind) — use NostrMessageTransport for group messages")
        if connections.isEmpty && !relayURLs.isEmpty {
            #if DEBUG
            print("[Invite] publishToRelays: no connections — connecting to \(relayURLs.count) relays")
            #endif
            await connect(to: relayURLs)
        }
        let conns = Array(connections.values)
        #if DEBUG
        print("[Invite] publishToRelays: publishing to \(conns.count) connections")
        #endif
        guard !conns.isEmpty else {
            throw ChatError.relayPublishFailed
        }
        let accepted = await withTaskGroup(of: Bool.self) { group in
            for conn in conns {
                group.addTask {
                    do {
                        return try await conn.publishAndAwaitOK(event: event)
                    } catch {
                        #if DEBUG
                        print("[Invite] publishToRelays: relay publish failed: \(error)")
                        #endif
                        return false
                    }
                }
            }
            var any = false
            for await result in group {
                if result { any = true }
            }
            return any
        }
        #if DEBUG
        print("[Invite] publishToRelays: eventID=\(event.id.prefix(12)) kind=\(event.kind) relays=\(conns.count) accepted=\(accepted)")
        #endif
        if !accepted {
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
            // Try to parse as rekey envelope first, then as invitation
            if let rekeyEnvelope = try? JSONDecoder().decode(SEPRekeyEnvelope.self, from: payloadData),
               rekeyEnvelope.type == SEPRekeyEnvelope.messageType {
                onRekeyEnvelope?(rekeyEnvelope)
            } else {
                let payload = try JSONDecoder().decode(BootstrapPayload.self, from: payloadData)
                let invitation = PendingInvitation(
                    id: event.id,
                    payload: payload,
                    receivedAt: Date(timeIntervalSince1970: TimeInterval(event.displayMilliseconds) / 1000.0)
                )
                onInvitation?(invitation)
            }
        } catch {
            #if DEBUG
            print("[Invite] Ignored event \(event.id.prefix(12)): \(error.localizedDescription)")
            #endif
        }
    }
}
