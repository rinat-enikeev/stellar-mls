package chat.onym.android.viewmodel

import android.content.Context
import android.net.Uri
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import chat.onym.android.model.ChatMessage

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
    var replyingToMessage by mutableStateOf<ChatMessage?>(null)

    val messages: List<ChatMessage>
        get() {
            val allMessages = groupListViewModel.chatMessages[groupID] ?: emptyList()
            val pinned = group?.pinnedEpoch ?: return allMessages
            // When pinned, only show messages from that epoch or system messages
            return allMessages.filter { msg ->
                msg.isSystemMessage || msg.epoch == pinned
            }
        }

    val group: chat.onym.android.model.ChatGroup?
        get() = groupListViewModel.groups.find { it.id == groupID }

    val groupName: String
        get() = group?.name ?: "Chat"

    val inviteLink: String?
        get() {
            val g = group ?: return null
            val code = chat.onym.android.model.InviteCode(
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
            return "https://onym.chat/join?code=${code.encode()}"
        }

    val hasBlossomServers: Boolean
        get() = groupListViewModel.blossomServerURLs.isNotEmpty()

    val isMember: Boolean
        get() {
            val g = group ?: return false
            return groupListViewModel.canSendInGroup(g)
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
        val replyToID = replyingToMessage?.id
        groupListViewModel.sendMessage(groupID, text, replyToID)
        inputText = ""
        replyingToMessage = null
    }

    fun retryMessage(messageID: String) {
        groupListViewModel.retryMessage(groupID, messageID)
    }

    fun sendImage(imageData: ByteArray) {
        isSendingImage = true
        val replyToID = replyingToMessage?.id
        groupListViewModel.sendImage(groupID, imageData, replyToID)
        selectedImageUri = null
        isSendingImage = false
        replyingToMessage = null
    }

    fun sendVideo(context: Context) {
        val uri = selectedVideoUri ?: return
        isSendingVideo = true
        val replyToID = replyingToMessage?.id
        groupListViewModel.sendVideo(groupID, context, uri, replyToID)
        selectedVideoUri = null
        isSendingVideo = false
        replyingToMessage = null
    }

    fun sendVoice(audioFile: java.io.File) {
        isSendingVoice = true
        val replyToID = replyingToMessage?.id
        groupListViewModel.sendVoice(groupID, audioFile, replyToID)
        isSendingVoice = false
        replyingToMessage = null
    }

    fun parentMessage(message: ChatMessage): ChatMessage? {
        val replyToID = message.replyToID ?: return null
        return messages.find { it.id == replyToID }
    }

    override fun onCleared() {
        super.onCleared()
        if (groupListViewModel.activeGroupID == groupID) {
            groupListViewModel.activeGroupID = null
        }
    }
}
