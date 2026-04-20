package chat.onym.android.push

import android.content.Context
import chat.onym.android.crypto.StorageEncryption
import chat.onym.android.crypto.GroupCrypto
import org.json.JSONObject
import org.unifiedpush.android.connector.MessagingReceiver
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

class StellarChatUnifiedPushReceiver : MessagingReceiver() {

    override fun onNewEndpoint(context: Context, endpoint: String, instance: String) {
        // Store new endpoint — GroupListViewModel will re-register on next launch
        val prefs = context.getSharedPreferences("stellar_push_config", Context.MODE_PRIVATE)
        prefs.edit()
            .putString("up_endpoint", endpoint)
            .putBoolean("endpoint_needs_refresh", true)
            .apply()
        UnifiedPushTokenProvider.notifyNewEndpoint(context, endpoint)
    }

    override fun onMessage(context: Context, message: ByteArray, instance: String) {
        // Initialize StorageEncryption
        StorageEncryption.init(context)

        // Message format: nonce(12) || ciphertext || tag(16)
        if (message.size < 28) {
            NotificationHelper.showGenericNotification(context)
            return
        }

        // For UnifiedPush, the sub_id hint is not in a JSON wrapper — we need to try all subscriptions
        // The PN relay sends: nonce(12) || AES-GCM(notification_key, event_content) || tag(16)
        // But we don't know which subscription it's for, so we try each one
        val store = PushSubscriptionStore.getInstance(context)
        val subscriptions = store.all()

        for (subscription in subscriptions) {
            try {
                val notificationKey = StorageEncryption.decrypt(subscription.encryptedNotificationKey)

                val nonce = message.sliceArray(0 until 12)
                val ciphertextAndTag = message.sliceArray(12 until message.size)

                val cipher = Cipher.getInstance("AES/GCM/NoPadding")
                val keySpec = SecretKeySpec(notificationKey, "AES")
                val gcmSpec = GCMParameterSpec(128, nonce)
                cipher.init(Cipher.DECRYPT_MODE, keySpec, gcmSpec)
                val sealedEnvelopeB64Bytes = cipher.doFinal(ciphertextAndTag)

                // If decryption succeeded, this is the right subscription
                val sealedEnvelopeJSON = String(android.util.Base64.decode(String(sealedEnvelopeB64Bytes), android.util.Base64.DEFAULT))

                val groupSecret = StorageEncryption.decrypt(subscription.encryptedGroupSecret)
                val salt = StorageEncryption.decrypt(subscription.encryptedSalt)
                val groupKey = GroupCrypto.deriveMessageKey(groupSecret, subscription.epoch, salt)
                val plaintext = GroupCrypto.decrypt(sealedEnvelopeJSON, groupKey)

                val messageJSON = JSONObject(plaintext)
                val text = messageJSON.optString("text", "")
                val senderPubkey = messageJSON.optString("senderBlsPubkey", "")
                val type = messageJSON.optString("type", "chat")

                val groupName = try {
                    StorageEncryption.decryptString(subscription.encryptedGroupName)
                } catch (_: Exception) { "StellarChat" }

                val senderAlias = store.resolveAlias(senderPubkey) ?: "Unknown"

                when (type) {
                    "chat", "image" -> NotificationHelper.showMessageNotification(
                        context, groupName, senderAlias, text, subscription.groupID
                    )
                    else -> NotificationHelper.showMessageNotification(
                        context, groupName, senderAlias, text, subscription.groupID
                    )
                }
                return  // Successfully decrypted — done
            } catch (_: Exception) {
                // Wrong key — try next subscription
                continue
            }
        }

        // No subscription could decrypt — show generic
        NotificationHelper.showGenericNotification(context)
    }

    override fun onUnregistered(context: Context, instance: String) {
        val prefs = context.getSharedPreferences("stellar_push_config", Context.MODE_PRIVATE)
        prefs.edit().remove("up_endpoint").apply()
    }

    override fun onRegistrationFailed(context: Context, instance: String) {
        // Nothing to do — user can retry from settings
    }
}
