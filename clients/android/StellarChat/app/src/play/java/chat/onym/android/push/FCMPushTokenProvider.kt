package chat.onym.android.push

import android.content.Context
import com.google.firebase.messaging.FirebaseMessaging
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.tasks.await
import kotlinx.coroutines.withContext

class FCMPushTokenProvider(context: Context) : PushTokenProvider {

    private val pushPrefs = context.getSharedPreferences("stellar_push_config", Context.MODE_PRIVATE)

    override suspend fun getToken(): String? {
        val cached = pushPrefs.getString("fcm_token", null)
        if (!cached.isNullOrBlank() && !pushPrefs.getBoolean("token_needs_refresh", false)) {
            return cached
        }
        return withContext(Dispatchers.IO) {
            try {
                val token = FirebaseMessaging.getInstance().token.await()
                pushPrefs.edit().putString("fcm_token", token).putBoolean("token_needs_refresh", false).apply()
                token
            } catch (_: Exception) {
                null
            }
        }
    }

    override fun getPlatformName() = "android-fcm"

    override fun setTokenRefreshCallback(callback: (String) -> Unit) {
        StellarChatMessagingService.onTokenRefresh = callback
    }
}
