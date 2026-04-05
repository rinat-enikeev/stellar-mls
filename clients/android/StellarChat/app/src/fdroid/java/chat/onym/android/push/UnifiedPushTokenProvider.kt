package chat.onym.android.push

import android.content.Context

class UnifiedPushTokenProvider(context: Context) : PushTokenProvider {

    private val pushPrefs = context.getSharedPreferences("stellar_push_config", Context.MODE_PRIVATE)

    override suspend fun getToken(): String? {
        return pushPrefs.getString("up_endpoint", null)
    }

    override fun getPlatformName() = "android-unifiedpush"

    override fun setTokenRefreshCallback(callback: (String) -> Unit) {
        // UnifiedPush handles token refresh via the receiver's onNewEndpoint,
        // which writes to SharedPreferences. The ViewModel re-reads on next use.
    }
}
