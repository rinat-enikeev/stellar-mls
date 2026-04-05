import CryptoKit
import Foundation
import UIKit

/// Manages push notification registration with the PN relay.
/// Handles subscribe/update/unsubscribe for all groups.
@MainActor
final class PushNotificationManager {
    private var pnRelayClient: PNRelayClient?
    let subscriptionStore = PushSubscriptionStore()
    private(set) var apnsToken: String?

    /// The PN relay URL, read from RelayerDefaults or settings.
    var relayURL: URL? {
        didSet {
            if let url = relayURL {
                pnRelayClient = PNRelayClient(relayURL: url)
            }
        }
    }

    /// Called when APNs returns a device token.
    func setAPNsToken(_ tokenData: Data) {
        apnsToken = tokenData.map { String(format: "%02x", $0) }.joined()
    }

    /// Register push notifications for all groups.
    func registerAll(groups: [ChatGroup]) async {
        guard let token = apnsToken, let client = pnRelayClient else { return }

        for group in groups {
            if !group.pushNotificationsEnabled {
                // Unregister if previously registered but now disabled
                if subscriptionStore.subscriptionForGroup(group.id) != nil {
                    await unregisterGroup(group.id)
                }
                continue
            }
            // Skip if already registered
            if subscriptionStore.subscriptionForGroup(group.id) != nil { continue }

            await registerGroup(group, token: token, client: client)
        }
    }

    /// Register push for a single group.
    func registerGroup(_ group: ChatGroup, token: String? = nil, client: PNRelayClient? = nil) async {
        guard let token = token ?? apnsToken,
              let client = client ?? pnRelayClient else { return }

        let subscriptionID = generateSubscriptionID()
        let notificationKey = generateNotificationKey()

        let filter: [String: Any] = [
            "kinds": [44114],
            "#t": [group.topicTag]
        ]

        do {
            try await client.subscribe(
                subscriptionID: subscriptionID,
                filter: filter,
                pushToken: token,
                notificationKey: notificationKey
            )

            // Store subscription info for NSE
            let info = PushSubscriptionStore.SubscriptionInfo(
                subscriptionID: subscriptionID,
                subIDHint: String(subscriptionID.prefix(8)),
                encryptedNotificationKey: try StorageEncryption.encrypt(notificationKey),
                encryptedGroupName: try StorageEncryption.encrypt(group.name),
                encryptedGroupSecret: try StorageEncryption.encrypt(group.groupSecret),
                encryptedSalt: try StorageEncryption.encrypt(group.salt),
                epoch: group.epoch,
                groupID: group.id
            )
            subscriptionStore.addSubscription(info)
            #if DEBUG
            print("[Push] Registered subscription for group \(group.name)")
            #endif
        } catch {
            #if DEBUG
            print("[Push] Failed to register group \(group.name): \(error)")
            #endif
        }
    }

    /// Update subscription when topic rotates (rekey).
    func updateGroupSubscription(groupID: String, newTopicTag: String, epoch: UInt64, groupSecret: Data, salt: Data) async {
        guard let client = pnRelayClient,
              let sub = subscriptionStore.subscriptionForGroup(groupID) else { return }

        let filter: [String: Any] = [
            "kinds": [44114],
            "#t": [newTopicTag]
        ]

        do {
            try await client.update(subscriptionID: sub.subscriptionID, filter: filter)
            subscriptionStore.updateSubscription(sub.subscriptionID, epoch: epoch, groupSecret: groupSecret, salt: salt)
            #if DEBUG
            print("[Push] Updated subscription for group \(groupID)")
            #endif
        } catch {
            #if DEBUG
            print("[Push] Failed to update group \(groupID): \(error)")
            #endif
        }
    }

    /// Unregister push for a group.
    func unregisterGroup(_ groupID: String) async {
        guard let client = pnRelayClient,
              let sub = subscriptionStore.subscriptionForGroup(groupID) else { return }

        do {
            try await client.unsubscribe(subscriptionID: sub.subscriptionID)
        } catch {
            #if DEBUG
            print("[Push] Failed to unsubscribe group \(groupID): \(error)")
            #endif
        }
        subscriptionStore.removeSubscription(sub.subscriptionID)
    }

    /// Re-register all subscriptions with a new APNs token.
    func reregisterAll(groups: [ChatGroup]) async {
        guard let token = apnsToken, let client = pnRelayClient else { return }

        // Unregister all existing
        for subID in subscriptionStore.allSubscriptionIDs() {
            try? await client.unsubscribe(subscriptionID: subID)
            subscriptionStore.removeSubscription(subID)
        }

        // Re-register only groups with push enabled
        for group in groups where group.pushNotificationsEnabled {
            await registerGroup(group, token: token, client: client)
        }
    }

    /// Register for invitation inbox notifications.
    func registerInbox(inboxTag: String) async {
        guard let token = apnsToken, let client = pnRelayClient else { return }

        let subscriptionID = generateSubscriptionID()
        let notificationKey = generateNotificationKey()

        let filter: [String: Any] = [
            "kinds": [24113, 34113],
            "#t": [inboxTag]
        ]

        do {
            try await client.subscribe(
                subscriptionID: subscriptionID,
                filter: filter,
                pushToken: token,
                notificationKey: notificationKey
            )
            #if DEBUG
            print("[Push] Registered inbox subscription")
            #endif
        } catch {
            #if DEBUG
            print("[Push] Failed to register inbox: \(error)")
            #endif
        }
    }

    // MARK: - Private

    private func generateSubscriptionID() -> String {
        var bytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, 32, &bytes)
        return bytes.map { String(format: "%02x", $0) }.joined()
    }

    private func generateNotificationKey() -> Data {
        var bytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, 32, &bytes)
        return Data(bytes)
    }
}
