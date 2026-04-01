package com.stellarmls.chat.viewmodel

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.ChatMessage
import com.stellarmls.chat.nostr.NostrMessageTransport
import java.util.Date

class ChatViewModel(
    private val group: ChatGroup,
    private val transport: NostrMessageTransport,
    private val myPubkey: String
) : ViewModel() {
    val messages = mutableStateListOf<ChatMessage>()
    var inputText by mutableStateOf("")
    private val seenIDs = mutableSetOf<String>()

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
            }
        }
    }

    fun sendMessage() {
        val text = inputText.trim()
        if (text.isEmpty()) return

        transport.send(group, text)

        // Add local copy immediately
        val localID = "local-${System.currentTimeMillis()}"
        messages.add(
            ChatMessage(
                id = localID,
                groupID = group.id,
                senderPubkey = myPubkey,
                text = text,
                timestamp = Date(),
                isMine = true
            )
        )
        inputText = ""
    }

    fun stopListening() {
        transport.onMessage = null
    }
}
