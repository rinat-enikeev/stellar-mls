package com.stellarmls.chat.viewmodel

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.ChatMessage
import com.stellarmls.chat.nostr.NostrMessageTransport
import com.stellarmls.chat.persistence.PersistenceStore
import kotlinx.coroutines.launch
import java.util.Date

class ChatViewModel(
    private val group: ChatGroup,
    private val transport: NostrMessageTransport,
    private val myPubkey: String,
    private val store: PersistenceStore? = null
) : ViewModel() {
    val messages = mutableStateListOf<ChatMessage>()
    var inputText by mutableStateOf("")
    private val seenIDs = mutableSetOf<String>()

    init {
        // Load persisted messages
        if (store != null) {
            viewModelScope.launch {
                try {
                    val loaded = store.loadMessages(group.id)
                    for (msg in loaded) {
                        if (seenIDs.add(msg.id)) {
                            messages.add(msg)
                        }
                    }
                } catch (_: Exception) { }
            }
        }
    }

    fun startListening() {
        transport.onMessage = { groupID, senderPubkey, text, eventID, timestamp ->
            if (groupID == group.id && seenIDs.add(eventID)) {
                val msg = ChatMessage(
                    id = eventID,
                    groupID = groupID,
                    senderPubkey = senderPubkey,
                    text = text,
                    timestamp = Date(timestamp * 1000),
                    isMine = senderPubkey == myPubkey
                )
                messages.add(msg)
                if (store != null) {
                    viewModelScope.launch {
                        try { store.saveMessage(msg) } catch (_: Exception) { }
                    }
                }
            }
        }
    }

    fun sendMessage() {
        val text = inputText.trim()
        if (text.isEmpty()) return

        transport.send(group, text)

        val localID = "local-${System.currentTimeMillis()}"
        val msg = ChatMessage(
            id = localID,
            groupID = group.id,
            senderPubkey = myPubkey,
            text = text,
            timestamp = Date(),
            isMine = true
        )
        messages.add(msg)
        inputText = ""

        if (store != null) {
            viewModelScope.launch {
                try { store.saveMessage(msg) } catch (_: Exception) { }
            }
        }
    }

    fun stopListening() {
        transport.onMessage = null
    }
}
