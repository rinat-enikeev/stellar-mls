package chat.onym.android.viewmodel

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import chat.onym.android.model.ChatGroup
import chat.onym.android.model.InviteCode
import chat.onym.android.model.toHex
import chat.onym.android.onchain.OnChainVerificationResult
import com.stellarmls.mls.SEPGroupType
import com.stellarmls.mls.SEPTier

class JoinGroupViewModel : ViewModel() {
    private val _inviteText = mutableStateOf("")
    var inviteText: String
        get() = _inviteText.value
        set(value) {
            _inviteText.value = value
            // Reset preview when text changes
            decodedGroup = null
            verificationResult = null
            error = null
        }
    var error by mutableStateOf<String?>(null)
    var joinedGroup by mutableStateOf<ChatGroup?>(null)
    var decodedGroup by mutableStateOf<ChatGroup?>(null)
    var verificationResult by mutableStateOf<OnChainVerificationResult?>(null)
    var isVerifying by mutableStateOf(false)

    fun decodeInvite() {
        val code = inviteText.trim()
        if (code.isEmpty()) {
            error = "Please enter an invite code"
            return
        }

        try {
            val invite = InviteCode.decode(code)
            val defaultRelays = ChatGroup(id = "", name = "", groupSecret = ByteArray(0)).relayHints
            val mergedRelays = (defaultRelays + invite.relayHints).distinct()

            decodedGroup = ChatGroup(
                id = invite.groupID.toHex(),
                name = invite.name,
                groupSecret = invite.groupSecret,
                relayHints = mergedRelays,
                members = invite.members.toMutableList(),
                epoch = invite.epoch,
                salt = invite.salt,
                commitment = invite.commitment,
                tier = SEPTier.entries.find { it.id == invite.tierRawValue } ?: SEPTier.LARGE,
                groupType = try { SEPGroupType.fromId(invite.groupTypeRawValue) } catch (_: IllegalArgumentException) { SEPGroupType.ANARCHY },
                adminPubkeys = invite.adminPubkeys.toMutableSet()
            )
            verificationResult = null
            error = null
        } catch (e: Exception) {
            error = "Invalid invite code: ${e.message}"
            decodedGroup = null
        }
    }

    fun confirmJoin() {
        joinedGroup = decodedGroup
    }
}
