package com.stellarmls.chat.viewmodel

import android.app.Application
import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.stellarmls.chat.crypto.KeyManager
import com.stellarmls.chat.model.BootstrapPayload
import com.stellarmls.chat.model.ChatError
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.InviteCode
import com.stellarmls.chat.model.PendingInvitation
import com.stellarmls.chat.model.toHex
import com.stellarmls.chat.nostr.InvitationTransport
import com.stellarmls.chat.nostr.NostrMessageTransport
import com.stellarmls.chat.onchain.OnChainService
import com.stellarmls.chat.onchain.OnChainVerificationResult
import com.stellarmls.chat.persistence.PersistenceStore
import com.stellarmls.mls.SEPCommitmentBuilder
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.security.SecureRandom

class GroupListViewModel(application: Application) : AndroidViewModel(application) {
    val keyManager = KeyManager(application)
    val groups = mutableStateListOf<ChatGroup>()
    val pendingInvitations = mutableStateListOf<PendingInvitation>()

    val store = PersistenceStore(application)

    // Relay management
    val relayURLs = mutableStateListOf<String>()
    private val relayPrefs = application.getSharedPreferences("stellar_relays", Context.MODE_PRIVATE)

    // Contract configuration
    var contractEndpoint by mutableStateOf("")
        private set
    var contractID by mutableStateOf("")
        private set
    private val contractPrefs = application.getSharedPreferences("stellar_contract", Context.MODE_PRIVATE)

    // On-chain
    var onChainService: OnChainService? = null
        private set

    // Transports
    lateinit var transport: NostrMessageTransport
        private set
    lateinit var invitationTransport: InvitationTransport
        private set

    private var connected = false

    init {
        // Load relay URLs
        val savedRelays = relayPrefs.getString("relay_urls", null)
        if (savedRelays != null) {
            relayURLs.addAll(savedRelays.split(",").filter { it.isNotBlank() })
        } else {
            relayURLs.addAll(listOf("wss://relay.damus.io", "wss://nos.lol"))
        }

        // Load contract config
        contractEndpoint = contractPrefs.getString("endpoint", "") ?: ""
        contractID = contractPrefs.getString("contract_id", "") ?: ""

        // Initialize transports
        transport = NostrMessageTransport(keyManager, relayURLs.toList())
        invitationTransport = InvitationTransport(keyManager)

        // Initialize on-chain service if configured
        configureContract()

        // Load persisted groups
        viewModelScope.launch {
            try {
                val loaded = store.loadGroups()
                groups.addAll(loaded)
            } catch (_: Exception) { }
        }
    }

    fun connectIfNeeded() {
        if (!connected) {
            transport.connect()
            invitationTransport.connect(relayURLs.toList())
            startInboxListener()
            connected = true

            // Subscribe to existing groups
            for (group in groups) {
                transport.subscribe(group)
            }
        }
    }

    // -- Group lifecycle --

    fun addGroup(group: ChatGroup) {
        if (groups.none { it.id == group.id }) {
            groups.add(group)
            connectIfNeeded()
            transport.subscribe(group)
            viewModelScope.launch {
                try { store.saveGroup(group) } catch (_: Exception) { }
            }
        }
    }

    fun updateGroup(group: ChatGroup) {
        val index = groups.indexOfFirst { it.id == group.id }
        if (index >= 0) {
            groups[index] = group
        }
        viewModelScope.launch {
            try { store.saveGroup(group) } catch (_: Exception) { }
        }
    }

    fun removeGroup(id: String) {
        groups.removeAll { it.id == id }
        viewModelScope.launch {
            try { store.deleteGroup(id) } catch (_: Exception) { }
        }
    }

    fun createGroup(name: String): Pair<ChatGroup, String>? {
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

            addGroup(group)
            return Pair(group, invite.encode())
        } catch (_: Exception) {
            return null
        }
    }

    // -- Invitations --

    fun removePendingInvitation(id: String) {
        pendingInvitations.removeAll { it.id == id }
    }

    private fun startInboxListener() {
        invitationTransport.onInvitation = { invitation ->
            // Deduplicate
            if (pendingInvitations.none { it.id == invitation.id } &&
                groups.none { it.id == invitation.payload.groupID.toHex() }) {
                pendingInvitations.add(invitation)
            }
        }
        invitationTransport.subscribeToInbox(
            inboxTag = keyManager.inboxTag,
            privateKey = keyManager.keyAgreementPrivateKey
        )
    }

    // -- Relay management --

    fun addRelay(urlString: String): Boolean {
        val trimmed = urlString.trim()
        if (!trimmed.startsWith("ws://") && !trimmed.startsWith("wss://")) return false
        if (relayURLs.contains(trimmed)) return false
        relayURLs.add(trimmed)
        persistRelays()
        return true
    }

    fun removeRelay(index: Int) {
        if (index in relayURLs.indices) {
            relayURLs.removeAt(index)
            persistRelays()
        }
    }

    fun moveRelay(from: Int, to: Int) {
        if (from in relayURLs.indices && to in relayURLs.indices) {
            val item = relayURLs.removeAt(from)
            relayURLs.add(to, item)
            persistRelays()
        }
    }

    private fun persistRelays() {
        relayPrefs.edit().putString("relay_urls", relayURLs.joinToString(",")).apply()
    }

    // -- Contract configuration --

    fun saveContractConfig(endpoint: String, contractId: String) {
        contractEndpoint = endpoint.trim()
        contractID = contractId.trim()
        contractPrefs.edit()
            .putString("endpoint", contractEndpoint)
            .putString("contract_id", contractID)
            .apply()
        configureContract()
    }

    val isContractConfigured: Boolean
        get() = contractEndpoint.isNotBlank() && contractID.isNotBlank()

    /** Reconfigure the on-chain service when contract settings change. */
    fun configureContract() {
        if (isContractConfigured) {
            onChainService = OnChainService(contractID, contractEndpoint)
        } else {
            onChainService = null
        }
    }

    /** Publish a group on-chain (runs on IO dispatcher). */
    fun publishGroupOnChain(group: ChatGroup, onResult: (Result<Unit>) -> Unit) {
        val service = onChainService
        if (service == null) {
            onResult(Result.failure(ChatError.ContractNotConfigured))
            return
        }
        viewModelScope.launch {
            try {
                val response = withContext(Dispatchers.IO) {
                    service.publishGroupCreation(
                        groupIDData = group.groupIDData,
                        members = group.members,
                        blsSecretKey = keyManager.blsSecretKey,
                        epoch = group.epoch,
                        salt = group.salt,
                        tier = group.tier
                    )
                }
                if (response.accepted) {
                    group.isPublishedOnChain = true
                    updateGroup(group)
                    onResult(Result.success(Unit))
                } else {
                    onResult(Result.failure(
                        ChatError.OnChainPublishFailed(response.message ?: "Rejected")
                    ))
                }
            } catch (e: Exception) {
                onResult(Result.failure(
                    ChatError.OnChainPublishFailed(e.message ?: "Unknown error")
                ))
            }
        }
    }

    /** Verify a group's on-chain state (runs on IO dispatcher). */
    fun verifyGroupOnChain(group: ChatGroup, onResult: (OnChainVerificationResult) -> Unit) {
        val service = onChainService
        if (service == null) {
            onResult(OnChainVerificationResult.Error("Contract not configured"))
            return
        }
        viewModelScope.launch {
            val result = withContext(Dispatchers.IO) {
                service.verifyCommitment(
                    groupIDData = group.groupIDData,
                    members = group.members,
                    epoch = group.epoch,
                    salt = group.salt,
                    tier = group.tier
                )
            }
            onResult(result)
        }
    }

    /** Verify membership via on-chain contract (runs on IO dispatcher). */
    fun verifyMembershipOnChain(group: ChatGroup, onResult: (Result<Boolean>) -> Unit) {
        val service = onChainService
        if (service == null) {
            onResult(Result.failure(ChatError.ContractNotConfigured))
            return
        }
        viewModelScope.launch {
            try {
                val valid = withContext(Dispatchers.IO) {
                    service.verifyMembership(
                        groupIDData = group.groupIDData,
                        members = group.members,
                        blsSecretKey = keyManager.blsSecretKey,
                        epoch = group.epoch,
                        salt = group.salt,
                        tier = group.tier
                    )
                }
                onResult(Result.success(valid))
            } catch (e: Exception) {
                onResult(Result.failure(e))
            }
        }
    }

    override fun onCleared() {
        transport.disconnect()
        invitationTransport.disconnect()
        super.onCleared()
    }
}
