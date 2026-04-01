package com.stellarmls.chat.viewmodel

import android.app.Application
import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.stellarmls.chat.crypto.KeyAttestation
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
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPGroupStateUpdate
import com.stellarmls.mls.SEPKeyAttestationPayload
import com.stellarmls.mls.SEPSaltRequest
import com.stellarmls.mls.SEPSaltResponse
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
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

    // Relayer configuration (fee decoupling)
    var relayerURL by mutableStateOf("")
        private set
    var relayerAuthToken by mutableStateOf("")
        private set
    val isRelayerConfigured: Boolean
        get() = relayerURL.isNotBlank()

    // On-chain
    var onChainService: OnChainService? = null
        private set

    // Salt history for offline recovery: groupID → (epoch → salt)
    private val saltHistory = mutableMapOf<String, MutableMap<Long, ByteArray>>()
    // Replay protection: processed protocol event IDs (H-7)
    private val processedProtocolEventIDs = mutableSetOf<String>()
    // Salt request rate limiting: "senderPubkey:epoch" keys already responded to (H-5)
    private val saltRequestsResponded = mutableSetOf<String>()

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

        // Load contract + relayer config
        contractEndpoint = contractPrefs.getString("endpoint", "") ?: ""
        contractID = contractPrefs.getString("contract_id", "") ?: ""
        relayerURL = contractPrefs.getString("relayer_url", "") ?: ""
        relayerAuthToken = contractPrefs.getString("relayer_auth_token", "") ?: ""

        // Initialize transports
        transport = NostrMessageTransport(keyManager, relayURLs.toList())
        invitationTransport = InvitationTransport(keyManager)

        // Initialize on-chain service if configured
        configureContract()

        // Load persisted groups and initialize salt history
        viewModelScope.launch {
            try {
                val loaded = store.loadGroups()
                groups.addAll(loaded)
                for (group in loaded) {
                    storeSalt(group.id, group.epoch, group.salt)
                }
            } catch (_: Exception) { }
        }
    }

    fun connectIfNeeded() {
        if (!connected) {
            transport.connect()
            invitationTransport.connect(relayURLs.toList())
            startInboxListener()
            setupProtocolMessageHandler()
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

    /** Save relayer configuration. */
    fun saveRelayerConfig(url: String, authToken: String) {
        relayerURL = url.trim()
        relayerAuthToken = authToken.trim()
        contractPrefs.edit()
            .putString("relayer_url", relayerURL)
            .putString("relayer_auth_token", relayerAuthToken)
            .apply()
        configureContract()
    }

    companion object {
        /** Known-good Soroban RPC endpoints (M-13). */
        val KNOWN_RPC_ENDPOINTS = listOf(
            "https://soroban-testnet.stellar.org",
            "https://soroban.stellar.org",
            "https://rpc-futurenet.stellar.org",
        )

        /** Check if a Soroban RPC endpoint URL is well-formed and uses HTTPS. */
        fun isValidRPCEndpoint(url: String): Boolean {
            return try {
                val parsed = java.net.URL(url)
                parsed.protocol == "https" && parsed.host.isNotBlank()
            } catch (_: Exception) { false }
        }
    }

    /** Reconfigure the on-chain service when contract settings change. */
    fun configureContract() {
        if (isContractConfigured && isValidRPCEndpoint(contractEndpoint)) {
            onChainService = if (isRelayerConfigured) {
                OnChainService(
                    contractID,
                    relayerURL,
                    relayerAuthToken.ifBlank { null }
                )
            } else {
                OnChainService(contractID, contractEndpoint)
            }
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
                        tier = group.tier,
                        callerAddress = keyManager.stellarAccountID
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

    // -- Salt History --

    fun storeSalt(groupID: String, epoch: Long, salt: ByteArray) {
        val history = saltHistory.getOrPut(groupID) { mutableMapOf() }
        history[epoch] = salt
        // Cap to last 64 epochs to prevent memory exhaustion
        if (history.size > SALT_HISTORY_WINDOW) {
            val oldest = history.keys.sorted().take(history.size - SALT_HISTORY_WINDOW)
            for (key in oldest) { history.remove(key) }
        }
    }

    companion object {
        private const val SALT_HISTORY_WINDOW = 64
    }

    fun getSalt(groupID: String, epoch: Long): ByteArray? {
        return saltHistory[groupID]?.get(epoch)
    }

    // -- State Update Protocol --

    /** Build a state update message for broadcasting after a membership change. */
    fun buildStateUpdate(
        group: ChatGroup,
        addedMembers: List<SEPGroupMemberLeaf> = emptyList(),
        removedMemberKeys: List<ByteArray> = emptyList()
    ): SEPGroupStateUpdate {
        val attestation = try {
            val att = keyManager.createAttestation()
            SEPKeyAttestationPayload(
                blsPubkey = att.blsPubkey,
                ed25519Pubkey = att.ed25519Pubkey,
                signature = att.signature
            )
        } catch (_: Exception) { null }

        return SEPGroupStateUpdate(
            epoch = group.epoch,
            salt = group.salt,
            addedMembers = addedMembers,
            removedMemberKeys = removedMemberKeys,
            commitment = group.commitment,
            senderAttestation = attestation
        )
    }

    /** Broadcast a state update to all members via the encrypted group channel. */
    fun broadcastStateUpdate(group: ChatGroup, update: SEPGroupStateUpdate) {
        val json = stateUpdateToJson(update)
        transport.sendProtocolMessage(group, json)
    }

    /** Apply a received state update to a local group. */
    fun applyStateUpdate(update: SEPGroupStateUpdate, groupID: String) {
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) return
        val group = groups[index]

        // Only apply if newer
        if (update.epoch <= group.epoch) return

        // Verify sender attestation BEFORE mutating state
        val senderAtt = update.senderAttestation
        if (senderAtt != null) {
            val att = KeyAttestation(
                blsPubkey = senderAtt.blsPubkey,
                ed25519Pubkey = senderAtt.ed25519Pubkey,
                signature = senderAtt.signature
            )
            if (!KeyAttestation.verify(att)) {
                return // Invalid attestation — discard update without modifying state
            }
        }

        // Apply member changes
        for (removed in update.removedMemberKeys) {
            group.members.removeAll { it.publicKeyCompressed.contentEquals(removed) }
        }
        for (added in update.addedMembers) {
            if (group.members.none { it.publicKeyCompressed.contentEquals(added.publicKeyCompressed) }) {
                group.members.add(added)
            }
        }
        group.members.sortWith { a, b ->
            val aKey = a.publicKeyCompressed
            val bKey = b.publicKeyCompressed
            for (i in 0 until minOf(aKey.size, bKey.size)) {
                val cmp = (aKey[i].toInt() and 0xFF) - (bKey[i].toInt() and 0xFF)
                if (cmp != 0) return@sortWith cmp
            }
            aKey.size - bKey.size
        }

        group.epoch = update.epoch
        group.salt = update.salt
        if (update.commitment != null) {
            group.commitment = update.commitment
        }

        groups[index] = group
        storeSalt(groupID, update.epoch, update.salt)
        viewModelScope.launch {
            try { store.saveGroup(group) } catch (_: Exception) { }
        }
    }

    /** Set up handler for protocol messages received on the group channel. */
    private fun setupProtocolMessageHandler() {
        transport.onProtocolMessage = { groupID, json, eventID, senderPubkey ->
            // Replay protection: skip already-processed protocol events (H-7)
            if (processedProtocolEventIDs.add(eventID)) {
                try {
                    val obj = JSONObject(json)
                    when (obj.optString("type")) {
                        SEPGroupStateUpdate.MESSAGE_TYPE -> {
                            val update = parseStateUpdate(obj)
                            applyStateUpdate(update, groupID)
                        }
                        SEPSaltRequest.MESSAGE_TYPE -> {
                            val epoch = obj.getLong("epoch")
                            // Rate-limit: respond only once per (sender, epoch) pair (H-5)
                            val rateKey = "$senderPubkey:$epoch"
                            if (saltRequestsResponded.add(rateKey)) {
                                val salt = getSalt(groupID, epoch)
                                if (salt != null) {
                                    val group = groups.find { it.id == groupID }
                                    if (group != null) {
                                        val response = SEPSaltResponse(epoch = epoch, salt = salt)
                                        transport.sendProtocolMessage(group, saltResponseToJson(response))
                                    }
                                }
                            }
                        }
                        SEPSaltResponse.MESSAGE_TYPE -> {
                            val epoch = obj.getLong("epoch")
                            val saltB64 = obj.getString("salt")
                            val salt = android.util.Base64.decode(saltB64, android.util.Base64.NO_WRAP)
                            storeSalt(groupID, epoch, salt)
                        }
                    }
                } catch (_: Exception) { }
            }
        }
    }

    // -- Group Deactivation --

    /**
     * Deactivate a group on-chain. Any member with a valid proof can deactivate.
     * M-18: [confirmed] must be true — callers should show a confirmation dialog first,
     * since deactivation is irreversible on-chain.
     */
    fun deactivateGroupOnChain(group: ChatGroup, confirmed: Boolean = false, onResult: (Result<Unit>) -> Unit) {
        if (!confirmed) {
            onResult(Result.failure(IllegalStateException("Deactivation requires explicit confirmation")))
            return
        }
        val service = onChainService
        if (service == null) {
            onResult(Result.failure(ChatError.ContractNotConfigured))
            return
        }
        viewModelScope.launch {
            try {
                val response = withContext(Dispatchers.IO) {
                    service.deactivateGroup(
                        groupIDData = group.groupIDData,
                        members = group.members,
                        blsSecretKey = keyManager.blsSecretKey,
                        epoch = group.epoch,
                        salt = group.salt,
                        tier = group.tier
                    )
                }
                if (response.accepted) {
                    onResult(Result.success(Unit))
                } else {
                    onResult(Result.failure(
                        ChatError.OnChainPublishFailed(response.message ?: "Deactivation rejected")
                    ))
                }
            } catch (e: Exception) {
                onResult(Result.failure(
                    ChatError.OnChainPublishFailed(e.message ?: "Unknown error")
                ))
            }
        }
    }

    // -- JSON Helpers --

    private fun stateUpdateToJson(update: SEPGroupStateUpdate): String {
        val obj = JSONObject()
        obj.put("type", SEPGroupStateUpdate.MESSAGE_TYPE)
        obj.put("epoch", update.epoch)
        obj.put("salt", android.util.Base64.encodeToString(update.salt, android.util.Base64.NO_WRAP))
        val added = JSONArray()
        for (m in update.addedMembers) {
            added.put(JSONObject().apply {
                put("publicKeyCompressed", android.util.Base64.encodeToString(m.publicKeyCompressed, android.util.Base64.NO_WRAP))
                put("leafHash", android.util.Base64.encodeToString(m.leafHash, android.util.Base64.NO_WRAP))
            })
        }
        obj.put("addedMembers", added)
        val removed = JSONArray()
        for (k in update.removedMemberKeys) {
            removed.put(android.util.Base64.encodeToString(k, android.util.Base64.NO_WRAP))
        }
        obj.put("removedMemberKeys", removed)
        val commitmentBytes = update.commitment
        if (commitmentBytes != null) {
            obj.put("commitment", android.util.Base64.encodeToString(commitmentBytes, android.util.Base64.NO_WRAP))
        }
        val att = update.senderAttestation
        if (att != null) {
            obj.put("senderAttestation", JSONObject().apply {
                put("blsPubkey", android.util.Base64.encodeToString(att.blsPubkey, android.util.Base64.NO_WRAP))
                put("ed25519Pubkey", android.util.Base64.encodeToString(att.ed25519Pubkey, android.util.Base64.NO_WRAP))
                put("signature", android.util.Base64.encodeToString(att.signature, android.util.Base64.NO_WRAP))
            })
        }
        return obj.toString()
    }

    private fun saltResponseToJson(response: SEPSaltResponse): String {
        return JSONObject().apply {
            put("type", SEPSaltResponse.MESSAGE_TYPE)
            put("epoch", response.epoch)
            put("salt", android.util.Base64.encodeToString(response.salt, android.util.Base64.NO_WRAP))
        }.toString()
    }

    private fun parseStateUpdate(obj: JSONObject): SEPGroupStateUpdate {
        val salt = android.util.Base64.decode(obj.getString("salt"), android.util.Base64.NO_WRAP)
        val addedArr = obj.optJSONArray("addedMembers") ?: JSONArray()
        val addedMembers = (0 until addedArr.length()).map { i ->
            val m = addedArr.getJSONObject(i)
            SEPGroupMemberLeaf(
                publicKeyCompressed = android.util.Base64.decode(m.getString("publicKeyCompressed"), android.util.Base64.NO_WRAP),
                leafHash = android.util.Base64.decode(m.getString("leafHash"), android.util.Base64.NO_WRAP)
            )
        }
        val removedArr = obj.optJSONArray("removedMemberKeys") ?: JSONArray()
        val removedKeys = (0 until removedArr.length()).map { i ->
            android.util.Base64.decode(removedArr.getString(i), android.util.Base64.NO_WRAP)
        }
        val commitment = if (obj.has("commitment")) {
            android.util.Base64.decode(obj.getString("commitment"), android.util.Base64.NO_WRAP)
        } else null
        val attestation = if (obj.has("senderAttestation")) {
            val att = obj.getJSONObject("senderAttestation")
            SEPKeyAttestationPayload(
                blsPubkey = android.util.Base64.decode(att.getString("blsPubkey"), android.util.Base64.NO_WRAP),
                ed25519Pubkey = android.util.Base64.decode(att.getString("ed25519Pubkey"), android.util.Base64.NO_WRAP),
                signature = android.util.Base64.decode(att.getString("signature"), android.util.Base64.NO_WRAP)
            )
        } else null

        return SEPGroupStateUpdate(
            epoch = obj.getLong("epoch"),
            salt = salt,
            addedMembers = addedMembers,
            removedMemberKeys = removedKeys,
            commitment = commitment,
            senderAttestation = attestation
        )
    }

    override fun onCleared() {
        transport.disconnect()
        invitationTransport.disconnect()
        super.onCleared()
    }
}
