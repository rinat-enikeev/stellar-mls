package com.stellarmls.chat.viewmodel

import android.app.Application
import androidx.compose.runtime.mutableStateListOf
import androidx.lifecycle.AndroidViewModel
import com.stellarmls.chat.crypto.KeyManager
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.InviteCode
import com.stellarmls.chat.nostr.NostrMessageTransport

class GroupListViewModel(application: Application) : AndroidViewModel(application) {
    val keyManager = KeyManager(application)
    val groups = mutableStateListOf<ChatGroup>()
    val transport = NostrMessageTransport(keyManager)

    private var connected = false

    fun connectIfNeeded() {
        if (!connected) {
            transport.connect()
            connected = true
        }
    }

    fun addGroup(group: ChatGroup) {
        if (groups.none { it.id == group.id }) {
            groups.add(group)
            connectIfNeeded()
            transport.subscribe(group)
        }
    }

    fun removeGroup(index: Int) {
        if (index in groups.indices) {
            groups.removeAt(index)
        }
    }

    override fun onCleared() {
        transport.disconnect()
        super.onCleared()
    }
}
