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
            // Merge invite code relays with default relays for maximum overlap
            val defaultRelays = ChatGroup(id = "", name = "", groupSecret = ByteArray(0)).relayHints
            val mergedRelays = (defaultRelays + invite.relayHints).distinct()

            joinedGroup = ChatGroup(
                id = invite.groupID.toHex(),
                name = invite.name,
                groupSecret = invite.groupSecret,
                relayHints = mergedRelays,
                members = invite.members.toMutableList(),
                epoch = invite.epoch,
                salt = invite.salt,
                commitment = invite.commitment
            )
            error = null
        } catch (e: Exception) {
            error = "Invalid invite code: ${e.message}"
            joinedGroup = null
        }
    }
}
