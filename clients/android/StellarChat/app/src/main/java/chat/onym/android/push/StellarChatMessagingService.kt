package chat.onym.android.push

import android.util.Base64
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import chat.onym.android.crypto.GroupCrypto
import chat.onym.android.crypto.KeyManager
import chat.onym.android.crypto.StorageEncryption
import chat.onym.android.model.ChatMessage
import chat.onym.android.model.MediaAttachment
import chat.onym.android.model.MessageStatus
import chat.onym.android.persistence.PersistenceStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.util.Date
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

class StellarChatMessagingService : FirebaseMessagingService() {

    companion object {
        /** Callback set by GroupListViewModel to handle token refreshes while app is alive. */
        var onTokenRefresh: ((String) -> Unit)? = null
    }

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

            // Skip messages sent by ourselves
            try {
                val myPubkey = Base64.encodeToString(
                    KeyManager.create(applicationContext).blsPublicKey(), Base64.NO_WRAP
                )
                if (senderPubkey == myPubkey) return
            } catch (_: Exception) { }

            // Resolve group name and sender alias
            val groupName = try {
                StorageEncryption.decryptString(subscription.encryptedGroupName)
            } catch (_: Exception) { "StellarChat" }

            val senderAlias = store.resolveAlias(senderPubkey) ?: run {
                // Show truncated pubkey instead of "Unknown"
                val decoded = try { Base64.decode(senderPubkey, Base64.DEFAULT) } catch (_: Exception) { null }
                if (decoded != null && decoded.size >= 4) {
                    decoded.take(4).joinToString("") { "%02x".format(it) } + "..."
                } else if (senderPubkey.length >= 8) {
                    senderPubkey.take(8) + "..."
                } else {
                    "Unknown"
                }
            }

            when (type) {
                "chat", "image" -> {
                    NotificationHelper.showMessageNotification(
                        applicationContext, groupName, senderAlias, text, subscription.groupID
                    )
                    // Persist message so it appears in chat history when the user opens the app
                    val mediaAttachment = if (type == "image") {
                        try {
                            val mediaJSON = messageJSON.optJSONObject("media")
                            if (mediaJSON != null) MediaAttachment(
                                blobHash = mediaJSON.optString("blobHash", ""),
                                fileKey = Base64.decode(mediaJSON.optString("fileKey", ""), Base64.DEFAULT),
                                mimeType = mediaJSON.optString("mimeType", "image/jpeg"),
                                width = mediaJSON.optInt("width", 0),
                                height = mediaJSON.optInt("height", 0),
                                size = mediaJSON.optInt("size", 0),
                                blossomServers = mutableListOf<String>().apply {
                                    val arr = mediaJSON.optJSONArray("blossomServers")
                                    if (arr != null) for (i in 0 until arr.length()) add(arr.getString(i))
                                },
                                encryptedThumbnail = mediaJSON.optString("encryptedThumbnail", "").let {
                                    if (it.isNotEmpty()) Base64.decode(it, Base64.DEFAULT) else null
                                }
                            ) else null
                        } catch (_: Exception) { null }
                    } else null

                    val msg = ChatMessage(
                        id = eventID ?: java.util.UUID.randomUUID().toString(),
                        groupID = subscription.groupID,
                        senderPubkey = senderPubkey,
                        text = text,
                        timestamp = Date(),
                        isMine = false,
                        status = MessageStatus.SENT,
                        mediaAttachment = mediaAttachment,
                        epoch = subscription.epoch
                    )
                    CoroutineScope(Dispatchers.IO).launch {
                        try {
                            PersistenceStore(applicationContext).saveMessage(msg)
                        } catch (_: Exception) { }
                    }
                }
                "call" -> NotificationHelper.showCallNotification(
                    applicationContext, groupName, senderAlias
                )
                // Protocol messages (sep_message_ack, sep_rekey, etc.) — silently ignore
                else -> return
            }

        } catch (e: Exception) {
            // Decryption failed — show generic notification
            NotificationHelper.showGenericNotification(applicationContext)
        }
    }

    override fun onNewToken(token: String) {
        // Store the new token
        val prefs = applicationContext.getSharedPreferences("stellar_push_config", MODE_PRIVATE)
        prefs.edit().putString("fcm_token", token).putBoolean("token_needs_refresh", false).apply()
        // Notify ViewModel to re-register if app is alive
        onTokenRefresh?.invoke(token)
    }
}
