package com.stellarmls.chat.push

import android.content.Context
import com.stellarmls.chat.crypto.StorageEncryption
import com.stellarmls.chat.model.ChatGroup
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

class PushNotificationManager(
    private val context: Context,
    relayURL: String
) {
    private val client = PNRelayClient(relayURL)
    private val store = PushSubscriptionStore.getInstance(context)

    /// Register push notifications for all groups.
    suspend fun registerAll(groups: List<ChatGroup>, pushToken: String, platform: String) {
        for (group in groups) {
            if (!group.pushNotificationsEnabled) {
                // Unregister if previously registered but now disabled
                if (store.findByGroupID(group.id) != null) {
                    unregisterGroup(group.id)
                }
                continue
            }
            if (store.findByGroupID(group.id) != null) continue
            registerGroup(group, pushToken, platform)
        }
    }

    /// Register push for a single group.
    suspend fun registerGroup(group: ChatGroup, pushToken: String, platform: String) {
        val subscriptionID = PushSubscriptionStore.generateSubscriptionID()
        val notificationKey = PushSubscriptionStore.generateNotificationKey()

        val filter = JSONObject().apply {
            put("kinds", JSONArray().put(44114))
            put("#t", JSONArray().put(group.topicTag))
        }

        try {
            client.subscribe(
                subscriptionID = subscriptionID,
                filter = filter,
                pushToken = pushToken,
                notificationKey = notificationKey,
                platform = platform
            )

            val info = PushSubscriptionStore.SubscriptionInfo(
                subscriptionID = subscriptionID,
                subIDHint = subscriptionID.take(8),
                encryptedNotificationKey = StorageEncryption.encrypt(notificationKey),
                encryptedGroupName = StorageEncryption.encrypt(group.name),
                encryptedGroupSecret = StorageEncryption.encrypt(group.groupSecret),
                encryptedSalt = StorageEncryption.encrypt(group.salt),
                epoch = group.epoch,
                groupID = group.id
            )
            store.save(info)
        } catch (e: Exception) {
            e.printStackTrace()
        }
    }

    /// Update subscription when topic rotates.
    suspend fun updateGroupSubscription(
        groupID: String,
        newTopicTag: String,
        epoch: Long,
        groupSecret: ByteArray,
        salt: ByteArray
    ) {
        val sub = store.findByGroupID(groupID) ?: return

        val filter = JSONObject().apply {
            put("kinds", JSONArray().put(44114))
            put("#t", JSONArray().put(newTopicTag))
        }

        try {
            client.update(sub.subscriptionID, filter)
            store.updateGroupKey(sub.subscriptionID, epoch, groupSecret, salt)
        } catch (e: Exception) {
            e.printStackTrace()
        }
    }

    /// Unregister push for a group.
    suspend fun unregisterGroup(groupID: String) {
        val sub = store.findByGroupID(groupID) ?: return
        try {
            client.unsubscribe(sub.subscriptionID)
        } catch (_: Exception) { }
        store.delete(sub.subscriptionID)
    }

    /// Re-register all with new token.
    suspend fun reregisterAll(groups: List<ChatGroup>, newToken: String, platform: String) {
        // Unregister all existing
        for (sub in store.all()) {
            try { client.unsubscribe(sub.subscriptionID) } catch (_: Exception) { }
            store.delete(sub.subscriptionID)
        }
        // Re-register
        registerAll(groups, newToken, platform)
    }

    /// Sync contacts for notification display.
    fun syncContacts(aliases: Map<String, String>) {
        for ((pubkey, alias) in aliases) {
            store.saveContact(pubkey, alias)
        }
    }
}
