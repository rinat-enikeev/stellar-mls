package chat.onym.android.push

import android.content.Context
import android.util.Log
import org.unifiedpush.android.connector.UnifiedPush

class UnifiedPushTokenProvider(context: Context) : PushTokenProvider {

    private val appContext = context.applicationContext
    private val pushPrefs = appContext.getSharedPreferences("stellar_push_config", Context.MODE_PRIVATE)

    override suspend fun getToken(): String? {
        val cached = pushPrefs.getString("up_endpoint", null)
        if (!cached.isNullOrBlank()) {
            return cached
        }

        ensureEndpointRequested()
        return pushPrefs.getString("up_endpoint", null)
    }

    override fun getPlatformName() = "android-up"

    override fun setTokenRefreshCallback(callback: (String) -> Unit) {
        endpointRefreshCallback = callback

        val cached = pushPrefs.getString("up_endpoint", null)
        if (!cached.isNullOrBlank() && pushPrefs.getBoolean("endpoint_needs_refresh", false)) {
            callback(cached)
            pushPrefs.edit().putBoolean("endpoint_needs_refresh", false).apply()
        }
    }

    private fun ensureEndpointRequested() {
        val savedDistributor = UnifiedPush.getSavedDistributor(appContext)
        if (savedDistributor != null) {
            UnifiedPush.registerApp(appContext)
            return
        }

        val distributors = UnifiedPush.getDistributors(appContext)
        when (distributors.size) {
            0 -> Log.i(TAG, "No UnifiedPush distributor installed")
            1 -> {
                UnifiedPush.saveDistributor(appContext, distributors.first())
                UnifiedPush.registerApp(appContext)
            }
            else -> Log.w(TAG, "Multiple UnifiedPush distributors installed; select one before enabling push")
        }
    }

    companion object {
        private const val TAG = "UnifiedPushProvider"

        @Volatile
        private var endpointRefreshCallback: ((String) -> Unit)? = null

        fun notifyNewEndpoint(context: Context, endpoint: String) {
            endpointRefreshCallback?.invoke(endpoint)
            context.getSharedPreferences("stellar_push_config", Context.MODE_PRIVATE)
                .edit()
                .putBoolean("endpoint_needs_refresh", false)
                .apply()
        }
    }
}
