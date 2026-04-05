import CryptoKit
import Foundation

/// Persists push subscription state in the shared App Group container.
/// Both the main app and the Notification Service Extension read this store.
final class PushSubscriptionStore {
    static let appGroupID = "group.chat.onym.ios"

    private let fileURL: URL

    struct SubscriptionInfo: Codable {
        let subscriptionID: String
        let subIDHint: String  // first 8 chars
        let encryptedNotificationKey: Data
        let encryptedGroupName: Data
        let encryptedGroupSecret: Data
        let encryptedSalt: Data
        let epoch: UInt64
        let groupID: String
    }

    struct StoreData: Codable {
        var subscriptions: [String: SubscriptionInfo]  // keyed by subscription_id
        var contacts: [String: Data]  // bls_pubkey_base64 -> encrypted alias
        var localBlsPubkeyBase64: String?
    }

    init() {
        if let container = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: Self.appGroupID
        ) {
            fileURL = container.appendingPathComponent("push_subscriptions.json")
        } else {
            // Fallback to temp directory if App Group is unavailable
            fileURL = FileManager.default.temporaryDirectory.appendingPathComponent("push_subscriptions.json")
        }
    }

    func load() -> StoreData {
        guard let data = try? Data(contentsOf: fileURL),
              let store = try? JSONDecoder().decode(StoreData.self, from: data) else {
            return StoreData(subscriptions: [:], contacts: [:])
        }
        return store
    }

    func save(_ store: StoreData) {
        guard let data = try? JSONEncoder().encode(store) else { return }
        try? data.write(to: fileURL, options: .atomic)
    }

    func addSubscription(_ info: SubscriptionInfo) {
        var store = load()
        store.subscriptions[info.subscriptionID] = info
        save(store)
    }

    func removeSubscription(_ subscriptionID: String) {
        var store = load()
        store.subscriptions.removeValue(forKey: subscriptionID)
        save(store)
    }

    func updateSubscription(_ subscriptionID: String, epoch: UInt64, groupSecret: Data, salt: Data) {
        var store = load()
        guard var info = store.subscriptions[subscriptionID] else { return }
        info = SubscriptionInfo(
            subscriptionID: info.subscriptionID,
            subIDHint: info.subIDHint,
            encryptedNotificationKey: info.encryptedNotificationKey,
            encryptedGroupName: info.encryptedGroupName,
            encryptedGroupSecret: (try? StorageEncryption.encrypt(groupSecret)) ?? info.encryptedGroupSecret,
            encryptedSalt: (try? StorageEncryption.encrypt(salt)) ?? info.encryptedSalt,
            epoch: epoch,
            groupID: info.groupID
        )
        store.subscriptions[subscriptionID] = info
        save(store)
    }

    func updateContacts(_ contacts: [String: String]) {
        var store = load()
        for (pubkey, alias) in contacts {
            if let encrypted = try? StorageEncryption.encrypt(alias) {
                store.contacts[pubkey] = encrypted
            }
        }
        save(store)
    }

    func findSubscription(byHint hint: String) -> SubscriptionInfo? {
        let store = load()
        return store.subscriptions.values.first { $0.subIDHint == hint }
    }

    func allSubscriptionIDs() -> [String] {
        Array(load().subscriptions.keys)
    }

    func subscriptionForGroup(_ groupID: String) -> SubscriptionInfo? {
        load().subscriptions.values.first { $0.groupID == groupID }
    }

    func setLocalBlsPubkey(_ base64: String) {
        var store = load()
        store.localBlsPubkeyBase64 = base64
        save(store)
    }

    func localBlsPubkeyBase64() -> String? {
        load().localBlsPubkeyBase64
    }
}
