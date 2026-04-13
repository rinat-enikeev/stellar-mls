package chat.onym.android.viewmodel

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import chat.onym.android.crypto.KeyManager
import chat.onym.android.model.BootstrapPayload
import chat.onym.android.model.ChatGroup
import chat.onym.android.model.InviteCode
import chat.onym.android.model.hexToBytes
import chat.onym.android.model.toHex
import chat.onym.android.nostr.InvitationTransport
import com.stellarmls.mls.SEPCommitmentBuilder
import com.stellarmls.mls.SEPTier
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import java.security.SecureRandom
import kotlin.coroutines.resume

enum class CreationPhase { INPUT, CREATING, PUBLISHING, INVITING, DONE }

sealed class OnChainPublishStatus {
    object Idle : OnChainPublishStatus()
    object Publishing : OnChainPublishStatus()
    object Published : OnChainPublishStatus()
    data class Failed(val reason: String) : OnChainPublishStatus()
}

sealed class InvitationStatus {
    object Sending : InvitationStatus()
    object Sent : InvitationStatus()
    data class Failed(val reason: String) : InvitationStatus()
}

class CreateGroupViewModel : ViewModel() {
    var groupName by mutableStateOf("")
    var inviteCode by mutableStateOf<String?>(null)
    var createdGroup by mutableStateOf<ChatGroup?>(null)
    var errorMessage by mutableStateOf<String?>(null)

    // Multi-phase creation state
    var phase by mutableStateOf(CreationPhase.INPUT)
    val participantKeys = mutableStateListOf<String>()
    var keyInput by mutableStateOf("")
    var keyValidationError by mutableStateOf<String?>(null)
    var onChainStatus by mutableStateOf<OnChainPublishStatus>(OnChainPublishStatus.Idle)
    val invitationStatuses = mutableStateMapOf<String, InvitationStatus>()

    fun createGroup(keyManager: KeyManager, tier: SEPTier = SEPTier.LARGE) {
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
                salt = SEPCommitmentBuilder.generateSalt(),
                tier = tier
            )
            group.recomputeCommitment()

            val invite = InviteCode(
                groupID = groupID,
                groupSecret = groupSecret,
                name = name,
                relayHints = group.relayHints,
                members = group.members.toList(),
                epoch = group.epoch,
                salt = group.salt,
                commitment = group.commitment,
                tierRawValue = group.tier.id
            )

            createdGroup = group
            inviteCode = invite.encode()
            errorMessage = null
        } catch (e: Exception) {
            errorMessage = e.message
        }
    }

    fun addParticipant(ownInboxKeyHex: String) {
        val trimmed = keyInput.trim().lowercase()
        keyValidationError = null

        if (trimmed.length != 64 || !trimmed.all { it in '0'..'9' || it in 'a'..'f' }) {
            keyValidationError = "Key must be exactly 64 hex characters."
            return
        }

        if (trimmed == ownInboxKeyHex.lowercase()) {
            keyValidationError = "Cannot add your own inbox key."
            return
        }

        if (participantKeys.contains(trimmed)) {
            keyValidationError = "This key has already been added."
            return
        }

        participantKeys.add(trimmed)
        keyInput = ""
    }

    fun removeParticipant(key: String) {
        participantKeys.remove(key)
    }

    fun startCreation(
        keyManager: KeyManager,
        tier: SEPTier,
        groupListViewModel: GroupListViewModel,
        invitationTransport: InvitationTransport
    ) {
        val sanitized = sanitizeGroupName(groupName) ?: run {
            errorMessage = "Group name must be 1-100 characters with no control characters"
            return
        }
        groupName = sanitized
        errorMessage = null
        phase = CreationPhase.CREATING

        viewModelScope.launch {
            try {
                createGroup(keyManager, tier)
                val group = createdGroup ?: return@launch

                // Add group to the list immediately
                groupListViewModel.addGroup(group)

                // On-chain publishing (blocking if configured)
                if (groupListViewModel.isContractConfigured) {
                    phase = CreationPhase.PUBLISHING
                    onChainStatus = OnChainPublishStatus.Publishing

                    val result = suspendCancellableCoroutine { cont ->
                        groupListViewModel.publishGroupOnChain(group) { result ->
                            cont.resume(result)
                        }
                    }

                    result.fold(
                        onSuccess = { onChainStatus = OnChainPublishStatus.Published },
                        onFailure = {
                            onChainStatus = OnChainPublishStatus.Failed(it.message ?: "Unknown error")
                            return@launch // UI shows retry/skip
                        }
                    )
                }

                // Send invitations
                sendInvitations(group, invitationTransport, keyManager, groupListViewModel.relayURLs.toList())
                phase = CreationPhase.DONE
            } catch (e: Exception) {
                errorMessage = e.message
                phase = CreationPhase.INPUT
            }
        }
    }

    fun retryOnChainPublish(
        groupListViewModel: GroupListViewModel,
        invitationTransport: InvitationTransport,
        keyManager: KeyManager
    ) {
        val group = createdGroup ?: return
        onChainStatus = OnChainPublishStatus.Publishing

        viewModelScope.launch {
            val result = suspendCancellableCoroutine { cont ->
                groupListViewModel.publishGroupOnChain(group) { result ->
                    cont.resume(result)
                }
            }
            result.fold(
                onSuccess = {
                    onChainStatus = OnChainPublishStatus.Published
                    sendInvitations(group, invitationTransport, keyManager, groupListViewModel.relayURLs.toList())
                    phase = CreationPhase.DONE
                },
                onFailure = {
                    onChainStatus = OnChainPublishStatus.Failed(it.message ?: "Unknown error")
                }
            )
        }
    }

    fun skipOnChainAndContinue(
        invitationTransport: InvitationTransport,
        keyManager: KeyManager,
        relayURLs: List<String>
    ) {
        val group = createdGroup ?: return
        viewModelScope.launch {
            sendInvitations(group, invitationTransport, keyManager, relayURLs)
            phase = CreationPhase.DONE
        }
    }

    private suspend fun sendInvitations(
        group: ChatGroup,
        invitationTransport: InvitationTransport,
        keyManager: KeyManager,
        relayURLs: List<String>
    ) {
        if (participantKeys.isEmpty()) return
        phase = CreationPhase.INVITING

        withContext(Dispatchers.IO) {
            invitationTransport.ensureConnected(relayURLs)
        }

        for ((index, key) in participantKeys.withIndex()) {
            if (index > 0) delay(500) // Space out relay publishes to avoid rate limiting
            invitationStatuses[key] = InvitationStatus.Sending
            try {
                val pubkeyBytes = key.hexToBytes()
                require(pubkeyBytes.size == 32) { "Key must be 32 bytes" }
                val payload = BootstrapPayload.from(group, keyManager.publicKeyHex)
                withContext(Dispatchers.IO) {
                    invitationTransport.sendInvitation(payload, pubkeyBytes, keyManager)
                }
                invitationStatuses[key] = InvitationStatus.Sent
            } catch (e: Exception) {
                invitationStatuses[key] = InvitationStatus.Failed(e.message ?: "Unknown error")
            }
        }
    }
}

/** M-15: Sanitize group name — strip control characters and enforce length limit. */
internal fun sanitizeGroupName(name: String): String? {
    val trimmed = name.trim()
    val sanitized = trimmed.filter { ch ->
        !ch.isISOControl() && ch.category != CharCategory.FORMAT
    }
    return if (sanitized.isNotEmpty() && sanitized.length <= 100) sanitized else null
}
