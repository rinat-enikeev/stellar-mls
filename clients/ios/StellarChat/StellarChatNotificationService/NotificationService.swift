import CryptoKit
import UserNotifications

class NotificationService: UNNotificationServiceExtension {
    private var contentHandler: ((UNNotificationContent) -> Void)?
    private var bestAttemptContent: UNMutableNotificationContent?

    override func didReceive(
        _ request: UNNotificationRequest,
        withContentHandler contentHandler: @escaping (UNNotificationContent) -> Void
    ) {
        self.contentHandler = contentHandler
        bestAttemptContent = request.content.mutableCopy() as? UNMutableNotificationContent

        guard let content = bestAttemptContent else {
            contentHandler(request.content)
            return
        }

        let userInfo = request.content.userInfo

        // Check for fetch-only notification (large payload)
        if userInfo["fetch"] != nil {
            contentHandler(content)
            return
        }

        // Extract encrypted fields
        guard let encB64 = userInfo["enc"] as? String,
              let nonceB64 = userInfo["nonce"] as? String,
              let tagB64 = userInfo["tag"] as? String,
              let subIDHint = userInfo["sub_id"] as? String,
              let eventID = userInfo["event_id"] as? String else {
            contentHandler(content)
            return
        }

        guard let encData = Data(base64Encoded: encB64),
              let nonceData = Data(base64Encoded: nonceB64),
              let tagData = Data(base64Encoded: tagB64) else {
            contentHandler(content)
            return
        }

        // Load subscription info from shared App Group store
        let store = PushSubscriptionStore()
        guard let subscription = store.findSubscription(byHint: subIDHint) else {
            contentHandler(content)
            return
        }

        do {
            // Step 1: Decrypt notification_key
            let notificationKey = try StorageEncryption.decrypt(subscription.encryptedNotificationKey)

            // Step 2: Decrypt the notification payload using notification_key
            let symmetricKey = SymmetricKey(data: notificationKey)
            let nonce = try AES.GCM.Nonce(data: nonceData)
            let sealedBox = try AES.GCM.SealedBox(
                nonce: nonce,
                ciphertext: encData,
                tag: tagData
            )
            let sealedEnvelopeB64Data = try AES.GCM.open(
                sealedBox,
                using: symmetricKey,
                authenticating: Data(eventID.utf8)
            )

            // Step 3: Decode the SealedEnvelope from base64
            guard let sealedEnvelopeB64 = String(data: sealedEnvelopeB64Data, encoding: .utf8),
                  let sealedEnvelopeData = Data(base64Encoded: sealedEnvelopeB64) else {
                contentHandler(content)
                return
            }

            let envelope = try JSONDecoder().decode(SealedEnvelope.self, from: sealedEnvelopeData)

            // Step 4: Decrypt the SealedEnvelope using the group key
            let groupSecret = try StorageEncryption.decrypt(subscription.encryptedGroupSecret)
            let salt = try StorageEncryption.decrypt(subscription.encryptedSalt)
            let groupKey = GroupCrypto.deriveMessageKey(
                groupSecret: groupSecret,
                epoch: subscription.epoch,
                salt: salt
            )

            let plaintext = try GroupCrypto.decrypt(envelope, key: groupKey)

            // Step 5: Parse the v2 message JSON
            if let messageData = plaintext.data(using: .utf8),
               let messageJSON = try? JSONSerialization.jsonObject(with: messageData) as? [String: Any] {
                let text = messageJSON["text"] as? String ?? ""
                let senderPubkey = messageJSON["senderBlsPubkey"] as? String ?? ""
                let type = messageJSON["type"] as? String ?? "chat"

                // Skip messages sent by ourselves
                if let myPubkey = store.localBlsPubkeyBase64(), senderPubkey == myPubkey {
                    content.title = ""
                    content.body = ""
                    content.sound = nil
                    contentHandler(content)
                    return
                }

                // Resolve group name
                let groupName = (try? StorageEncryption.decryptString(subscription.encryptedGroupName)) ?? "StellarChat"

                // Show truncated pubkey as sender identifier
                let senderAlias: String
                if let pubkeyData = Data(base64Encoded: senderPubkey) {
                    let hex = pubkeyData.prefix(4).map { String(format: "%02x", $0) }.joined()
                    senderAlias = hex + "..."
                } else {
                    senderAlias = String(senderPubkey.prefix(8)) + "..."
                }

                if type == "chat" || type == "image" {
                    content.title = groupName
                    content.body = "\(senderAlias): \(text)"
                    content.badge = Self.incrementBadgeCount() as NSNumber
                    // Store groupID for navigation on tap
                    content.userInfo["groupID"] = subscription.groupID

                    // Persist to shared App Group so main app can import on launch
                    let persistedEpoch = Int64(exactly: subscription.epoch) ?? Int64.max
                    Self.persistPendingMessage(
                        id: eventID,
                        groupID: subscription.groupID,
                        senderPubkey: senderPubkey,
                        text: text,
                        type: type,
                        epoch: persistedEpoch,
                        mediaJSON: messageJSON["media"] as? [String: Any]
                    )
                } else if type == "call" {
                    content.title = groupName
                    content.body = "\(senderAlias) is calling..."
                } else {
                    // Protocol message (sep_message_ack, sep_rekey, etc.) — suppress notification
                    content.title = ""
                    content.body = ""
                    content.sound = nil
                }
            }
        } catch {
            // Decryption failed — show generic notification (already set as default)
        }

        contentHandler(content)
    }

    override func serviceExtensionTimeWillExpire() {
        if let handler = contentHandler, let content = bestAttemptContent {
            handler(content)
        }
    }

    // MARK: - Persist pending messages for main app import

    private static let appGroupID = "group.chat.onym.ios"
    private static let pendingFileName = "pending_pn_messages.jsonl"

    /// Write a decrypted message to the shared App Group container as a JSON line.
    /// The main app reads and imports these on launch, then clears the file.
    static func persistPendingMessage(
        id: String,
        groupID: String,
        senderPubkey: String,
        text: String,
        type: String,
        epoch: Int64,
        mediaJSON: [String: Any]?
    ) {
        guard let container = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: appGroupID
        ) else { return }

        let fileURL = container.appendingPathComponent(pendingFileName)
        var entry: [String: Any] = [
            "id": id,
            "groupID": groupID,
            "senderPubkey": senderPubkey,
            "text": text,
            "type": type,
            "epoch": epoch,
            "timestamp": Date().timeIntervalSince1970
        ]
        if let media = mediaJSON {
            entry["media"] = media
        }

        guard let data = try? JSONSerialization.data(withJSONObject: entry),
              var line = String(data: data, encoding: .utf8) else { return }
        line += "\n"

        if FileManager.default.fileExists(atPath: fileURL.path) {
            if let handle = try? FileHandle(forWritingTo: fileURL) {
                handle.seekToEndOfFile()
                handle.write(Data(line.utf8))
                handle.closeFile()
            }
        } else {
            try? Data(line.utf8).write(to: fileURL, options: .atomic)
        }
    }

    // MARK: - Badge count

    private static let badgeCountKey = "pn_badge_count"

    /// Atomically increment the badge count stored in the shared App Group and return the new value.
    static func incrementBadgeCount() -> Int {
        let defaults = UserDefaults(suiteName: appGroupID) ?? .standard
        let current = defaults.integer(forKey: badgeCountKey)
        let next = current + 1
        defaults.set(next, forKey: badgeCountKey)
        return next
    }

    /// Reset the badge count (called by the main app when it becomes active).
    static func resetBadgeCount() {
        let defaults = UserDefaults(suiteName: appGroupID) ?? .standard
        defaults.set(0, forKey: badgeCountKey)
    }
}
