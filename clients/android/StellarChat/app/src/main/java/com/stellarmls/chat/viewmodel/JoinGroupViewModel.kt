package com.stellarmls.chat.viewmodel

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.InviteCode
import com.stellarmls.chat.model.toHex

class JoinGroupViewModel : ViewModel() {
    var inviteText by mutableStateOf("")
    var error by mutableStateOf<String?>(null)
    var joinedGroup by mutableStateOf<ChatGroup?>(null)

    fun joinGroup() {
        val code = inviteText.trim()
        if (code.isEmpty()) {
            error = "Please enter an invite code"
            return
        }

        try {
            val invite = InviteCode.decode(code)
            joinedGroup = ChatGroup(
                id = invite.groupID.toHex(),
                name = invite.name,
                groupSecret = invite.groupSecret,
                relayHints = invite.relayHints
            )
            error = null
        } catch (e: Exception) {
            error = "Invalid invite code: ${e.message}"
            joinedGroup = null
        }
    }
}
