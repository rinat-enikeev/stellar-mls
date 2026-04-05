package chat.onym.android.push

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import chat.onym.android.crypto.StorageEncryption
import org.json.JSONObject
import java.security.SecureRandom

class PushSubscriptionStore private constructor(
    private val prefs: SharedPreferences
) {
    data class SubscriptionInfo(
        val subscriptionID: String,
        val subIDHint: String,
        val encryptedNotificationKey: ByteArray,
        val encryptedGroupName: ByteArray,
        val encryptedGroupSecret: ByteArray,
        val encryptedSalt: ByteArray,
        val epoch: Long,
        val groupID: String
    )

    fun save(info: SubscriptionInfo) {
        val json = JSONObject().apply {
            put("subscriptionID", info.subscriptionID)
            put("subIDHint", info.subIDHint)
            put("encryptedNotificationKey", android.util.Base64.encodeToString(info.encryptedNotificationKey, android.util.Base64.NO_WRAP))
            put("encryptedGroupName", android.util.Base64.encodeToString(info.encryptedGroupName, android.util.Base64.NO_WRAP))
            put("encryptedGroupSecret", android.util.Base64.encodeToString(info.encryptedGroupSecret, android.util.Base64.NO_WRAP))
            put("encryptedSalt", android.util.Base64.encodeToString(info.encryptedSalt, android.util.Base64.NO_WRAP))
            put("epoch", info.epoch)
            put("groupID", info.groupID)
        }
        prefs.edit().putString("sub_${info.subscriptionID}", json.toString()).apply()

        // Maintain index
        val index = getIndex().toMutableSet()
        index.add(info.subscriptionID)
        prefs.edit().putStringSet("subscription_index", index).apply()
    }

    fun delete(subscriptionID: String) {
        prefs.edit().remove("sub_$subscriptionID").apply()
        val index = getIndex().toMutableSet()
        index.remove(subscriptionID)
        prefs.edit().putStringSet("subscription_index", index).apply()
    }

    fun findByHint(subIDHint: String): SubscriptionInfo? {
        return all().firstOrNull { it.subIDHint == subIDHint }
    }

    fun findByGroupID(groupID: String): SubscriptionInfo? {
        return all().firstOrNull { it.groupID == groupID }
    }

    fun all(): List<SubscriptionInfo> {
        return getIndex().mapNotNull { id -> load(id) }
    }

    fun updateGroupKey(subscriptionID: String, epoch: Long, groupSecret: ByteArray, salt: ByteArray) {
        val info = load(subscriptionID) ?: return
        val updated = info.copy(
            epoch = epoch,
            encryptedGroupSecret = StorageEncryption.encrypt(groupSecret),
            encryptedSalt = StorageEncryption.encrypt(salt)
        )
        save(updated)
    }

    fun saveContact(blsPubkeyB64: String, alias: String) {
        val encrypted = StorageEncryption.encrypt(alias)
        prefs.edit().putString("contact_$blsPubkeyB64",
            android.util.Base64.encodeToString(encrypted, android.util.Base64.NO_WRAP)
        ).apply()
    }

    fun resolveAlias(blsPubkeyB64: String): String? {
        val encB64 = prefs.getString("contact_$blsPubkeyB64", null) ?: return null
        return try {
            val enc = android.util.Base64.decode(encB64, android.util.Base64.DEFAULT)
            StorageEncryption.decryptString(enc)
        } catch (_: Exception) { null }
    }

    private fun load(subscriptionID: String): SubscriptionInfo? {
        val jsonStr = prefs.getString("sub_$subscriptionID", null) ?: return null
        return try {
            val json = JSONObject(jsonStr)
            SubscriptionInfo(
                subscriptionID = json.getString("subscriptionID"),
                subIDHint = json.getString("subIDHint"),
                encryptedNotificationKey = android.util.Base64.decode(json.getString("encryptedNotificationKey"), android.util.Base64.DEFAULT),
                encryptedGroupName = android.util.Base64.decode(json.getString("encryptedGroupName"), android.util.Base64.DEFAULT),
                encryptedGroupSecret = android.util.Base64.decode(json.getString("encryptedGroupSecret"), android.util.Base64.DEFAULT),
                encryptedSalt = android.util.Base64.decode(json.getString("encryptedSalt"), android.util.Base64.DEFAULT),
                epoch = json.getLong("epoch"),
                groupID = json.getString("groupID")
            )
        } catch (_: Exception) { null }
    }

    private fun getIndex(): Set<String> {
        return prefs.getStringSet("subscription_index", emptySet()) ?: emptySet()
    }

    companion object {
        @Volatile
        private var instance: PushSubscriptionStore? = null

        fun getInstance(context: Context): PushSubscriptionStore {
            return instance ?: synchronized(this) {
                instance ?: run {
                    val masterKey = MasterKey.Builder(context)
                        .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
                        .build()
                    val prefs = EncryptedSharedPreferences.create(
                        context,
                        "stellar_push_subscriptions",
                        masterKey,
                        EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
                        EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
                    )
                    PushSubscriptionStore(prefs).also { instance = it }
                }
            }
        }

        fun generateSubscriptionID(): String {
            val bytes = ByteArray(32)
            SecureRandom().nextBytes(bytes)
            return bytes.joinToString("") { "%02x".format(it) }
        }

        fun generateNotificationKey(): ByteArray {
            val bytes = ByteArray(32)
            SecureRandom().nextBytes(bytes)
            return bytes
        }
    }
}
