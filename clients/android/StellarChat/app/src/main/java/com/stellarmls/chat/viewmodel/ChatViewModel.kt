package com.stellarmls.chat.viewmodel

import android.content.Context
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
    var selectedVideoUri by mutableStateOf<Uri?>(null)
    var isSendingVideo by mutableStateOf(false)
    var isSendingVoice by mutableStateOf(false)

    val messages: List<ChatMessage>
        get() {
            val allMessages = groupListViewModel.chatMessages[groupID] ?: emptyList()
            val pinned = group?.pinnedEpoch ?: return allMessages
            // When pinned, only show messages from that epoch or system messages
            return allMessages.filter { msg ->
                msg.isSystemMessage || msg.epoch == pinned
            }
        }

    val group: com.stellarmls.chat.model.ChatGroup?
        get() = groupListViewModel.groups.find { it.id == groupID }

    val groupName: String
        get() = group?.name ?: "Chat"

    val inviteLink: String?
        get() {
            val g = group ?: return null
            val code = com.stellarmls.chat.model.InviteCode(
                groupID = g.groupIDData,
                groupSecret = g.groupSecret,
                name = g.name,
                relayHints = g.relayHints,
                members = g.members.toList(),
                epoch = g.epoch,
                salt = g.salt,
                commitment = g.commitment,
                tierRawValue = g.tier.id
            )
            return "stellarchat://join?code=${code.encode()}"
        }

    val hasBlossomServers: Boolean
        get() = groupListViewModel.blossomServerURLs.isNotEmpty()

    val isMember: Boolean
        get() {
            val g = group ?: return false
            return groupListViewModel.isMember(g)
        }

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

    fun sendVideo(context: Context) {
        val uri = selectedVideoUri ?: return
        isSendingVideo = true
        groupListViewModel.sendVideo(groupID, context, uri)
        selectedVideoUri = null
        isSendingVideo = false
    }

    fun sendVoice(audioFile: java.io.File) {
        isSendingVoice = true
        groupListViewModel.sendVoice(groupID, audioFile)
        isSendingVoice = false
    }

    override fun onCleared() {
        super.onCleared()
        if (groupListViewModel.activeGroupID == groupID) {
            groupListViewModel.activeGroupID = null
        }
    }
}
