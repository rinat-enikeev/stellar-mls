package chat.onym.android

import android.app.Application
import chat.onym.android.push.NotificationHelper

class StellarChatApp : Application() {
    override fun onCreate() {
        super.onCreate()
        NotificationHelper.createChannels(this)
    }
}
