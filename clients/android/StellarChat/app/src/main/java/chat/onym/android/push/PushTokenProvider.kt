package chat.onym.android.push

interface PushTokenProvider {
    suspend fun getToken(): String?
    fun getPlatformName(): String
    fun setTokenRefreshCallback(callback: (String) -> Unit)
}
