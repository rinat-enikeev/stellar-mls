package chat.onym.android.push

import android.content.Context

object PushTokenProviderFactory {
    fun create(context: Context): PushTokenProvider = UnifiedPushTokenProvider(context)
}
