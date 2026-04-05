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

                // Resolve group name
                let groupName = (try? StorageEncryption.decryptString(subscription.encryptedGroupName)) ?? "StellarChat"

                // Resolve sender alias
                let storeData = store.load()
                var senderAlias = "Unknown"
                if let encAlias = storeData.contacts[senderPubkey],
                   let alias = try? StorageEncryption.decryptString(encAlias) {
                    senderAlias = alias
                }

                if type == "chat" || type == "image" {
                    content.title = groupName
                    content.body = "\(senderAlias): \(text)"
                } else if type == "call" {
                    content.title = groupName
                    content.body = "\(senderAlias) is calling..."
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
}
