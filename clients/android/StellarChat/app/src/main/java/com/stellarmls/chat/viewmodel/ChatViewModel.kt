package com.stellarmls.chat.viewmodel

import android.net.Uri
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import com.stellarmls.chat.model.ChatMessage

class ChatViewModel(
    val groupID: String,
    private val groupListViewModel: GroupListViewModel
) : ViewModel() {
    var inputText by mutableStateOf("")
    var selectedImageUri by mutableStateOf<Uri?>(null)
    var isSendingImage by mutableStateOf(false)

    val messages: List<ChatMessage>
        get() = groupListViewModel.chatMessages[groupID] ?: emptyList()

    val groupName: String
        get() = groupListViewModel.groups.find { it.id == groupID }?.name ?: "Chat"

    val hasBlossomServers: Boolean
        get() = groupListViewModel.blossomServerURLs.isNotEmpty()

    /** ID of the first unread message when the chat was opened. */
    val firstUnreadMessageID: String?

    init {
        // Capture the first unread message before clearing the count
        val unreadCount = groupListViewModel.unreadCounts[groupID] ?: 0
        val msgs = groupListViewModel.chatMessages[groupID] ?: emptyList()
        firstUnreadMessageID = if (unreadCount > 0 && msgs.size >= unreadCount) {
            msgs[msgs.size - unreadCount].id
        } else null
        // Mark this group as active and clear unread count
        groupListViewModel.activeGroupID = groupID
        groupListViewModel.unreadCounts[groupID] = 0
    }

    fun sendMessage() {
        val text = inputText.trim()
        if (text.isEmpty()) return
        groupListViewModel.sendMessage(groupID, text)
        inputText = ""
    }

    fun retryMessage(messageID: String) {
        groupListViewModel.retryMessage(groupID, messageID)
    }

    fun sendImage(imageData: ByteArray) {
        isSendingImage = true
        groupListViewModel.sendImage(groupID, imageData)
        selectedImageUri = null
        isSendingImage = false
    }

    override fun onCleared() {
        super.onCleared()
        if (groupListViewModel.activeGroupID == groupID) {
            groupListViewModel.activeGroupID = null
        }
    }
}
