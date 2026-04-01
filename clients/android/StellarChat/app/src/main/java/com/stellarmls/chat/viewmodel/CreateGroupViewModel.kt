package com.stellarmls.chat.viewmodel

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import com.stellarmls.chat.crypto.KeyManager
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.InviteCode
import com.stellarmls.chat.model.toHex
import com.stellarmls.mls.SEPCommitmentBuilder
import java.security.SecureRandom

class CreateGroupViewModel : ViewModel() {
    var groupName by mutableStateOf("")
    var inviteCode by mutableStateOf<String?>(null)
    var createdGroup by mutableStateOf<ChatGroup?>(null)
    var errorMessage by mutableStateOf<String?>(null)

    fun createGroup(keyManager: KeyManager) {
        val name = groupName.trim()
        if (name.isEmpty()) return

        try {
            val random = SecureRandom()
            val groupID = ByteArray(32).also { random.nextBytes(it) }
            val groupSecret = ByteArray(32).also { random.nextBytes(it) }

            val myLeaf = keyManager.memberLeaf()

            val group = ChatGroup(
                id = groupID.toHex(),
                name = name,
                groupSecret = groupSecret,
                members = mutableListOf(myLeaf),
                epoch = 0,
                salt = SEPCommitmentBuilder.generateSalt()
            )
            group.recomputeCommitment()

            val invite = InviteCode(
                groupID = groupID,
                groupSecret = groupSecret,
                name = name,
                relayHints = group.relayHints
            )

            createdGroup = group
            inviteCode = invite.encode()
            errorMessage = null
        } catch (e: Exception) {
            errorMessage = e.message
        }
    }
}
