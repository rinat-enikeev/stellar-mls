package com.stellarmls.chat.viewmodel

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

    val messages: List<ChatMessage>
        get() = groupListViewModel.chatMessages[groupID] ?: emptyList()

    val groupName: String
        get() = groupListViewModel.groups.find { it.id == groupID }?.name ?: "Chat"

    fun sendMessage() {
        val text = inputText.trim()
        if (text.isEmpty()) return
        groupListViewModel.sendMessage(groupID, text)
        inputText = ""
    }
}
