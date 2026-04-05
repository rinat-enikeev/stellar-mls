package chat.onym.android.push

import android.util.Base64
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import chat.onym.android.crypto.GroupCrypto
import chat.onym.android.crypto.StorageEncryption
import org.json.JSONObject
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

class StellarChatMessagingService : FirebaseMessagingService() {

    override fun onMessageReceived(message: RemoteMessage) {
        val data = message.data
        val subIDHint = data["sub_id"] ?: return
        val encB64 = data["enc"]
        val eventID = data["event_id"]

        // Initialize StorageEncryption if needed
        StorageEncryption.init(applicationContext)

        if (encB64 == null) {
            // Wake-only message
            NotificationHelper.showGenericNotification(applicationContext)
            return
        }

        val nonceB64 = data["nonce"] ?: return
        val tagB64 = data["tag"] ?: return

        try {
            val store = PushSubscriptionStore.getInstance(applicationContext)
            val subscription = store.findByHint(subIDHint)
                ?: run { NotificationHelper.showGenericNotification(applicationContext); return }

            // Step 1: Decrypt notification_key
            val notificationKey = StorageEncryption.decrypt(subscription.encryptedNotificationKey)

            // Step 2: Decrypt the notification payload using notification_key
            val encData = Base64.decode(encB64, Base64.DEFAULT)
            val nonceData = Base64.decode(nonceB64, Base64.DEFAULT)
            val tagData = Base64.decode(tagB64, Base64.DEFAULT)

            // AES-256-GCM decryption with event_id as AAD
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            val keySpec = SecretKeySpec(notificationKey, "AES")
            val gcmSpec = GCMParameterSpec(128, nonceData)
            cipher.init(Cipher.DECRYPT_MODE, keySpec, gcmSpec)
            if (eventID != null) {
                cipher.updateAAD(eventID.toByteArray())
            }
            // GCM expects ciphertext || tag concatenated
            val combined = encData + tagData
            val sealedEnvelopeB64Bytes = cipher.doFinal(combined)

            // Step 3: Decode SealedEnvelope from base64
            val sealedEnvelopeB64 = String(sealedEnvelopeB64Bytes)
            val sealedEnvelopeJSON = String(Base64.decode(sealedEnvelopeB64, Base64.DEFAULT))

            // Step 4: Decrypt the SealedEnvelope using the group key
            val groupSecret = StorageEncryption.decrypt(subscription.encryptedGroupSecret)
            val salt = StorageEncryption.decrypt(subscription.encryptedSalt)
            val groupKey = GroupCrypto.deriveMessageKey(groupSecret, subscription.epoch, salt)
            val plaintext = GroupCrypto.decrypt(sealedEnvelopeJSON, groupKey)

            // Step 5: Parse the v2 message JSON
            val messageJSON = JSONObject(plaintext)
            val text = messageJSON.optString("text", "")
            val senderPubkey = messageJSON.optString("senderBlsPubkey", "")
            val type = messageJSON.optString("type", "chat")

            // Resolve group name and sender alias
            val groupName = try {
                StorageEncryption.decryptString(subscription.encryptedGroupName)
            } catch (_: Exception) { "StellarChat" }

            val senderAlias = store.resolveAlias(senderPubkey) ?: "Unknown"

            when (type) {
                "chat", "image" -> NotificationHelper.showMessageNotification(
                    applicationContext, groupName, senderAlias, text, subscription.groupID
                )
                "call" -> NotificationHelper.showCallNotification(
                    applicationContext, groupName, senderAlias
                )
                else -> NotificationHelper.showMessageNotification(
                    applicationContext, groupName, senderAlias, text, subscription.groupID
                )
            }

        } catch (e: Exception) {
            // Decryption failed — show generic notification
            NotificationHelper.showGenericNotification(applicationContext)
        }
    }

    override fun onNewToken(token: String) {
        // Store the new token — GroupListViewModel will re-register on next launch
        val prefs = applicationContext.getSharedPreferences("stellar_push_config", MODE_PRIVATE)
        prefs.edit().putString("fcm_token", token).putBoolean("token_needs_refresh", true).apply()
    }
}
