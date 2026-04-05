package com.stellarmls.chat

import android.app.Application
import com.stellarmls.chat.push.NotificationHelper

class StellarChatApp : Application() {
    override fun onCreate() {
        super.onCreate()
        NotificationHelper.createChannels(this)
    }
}
