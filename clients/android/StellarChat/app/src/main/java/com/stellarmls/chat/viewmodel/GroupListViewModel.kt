package com.stellarmls.chat.viewmodel

import android.app.Application
import android.content.Context
import android.util.Log
import com.stellarmls.chat.BuildConfig
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.stellarmls.chat.blossom.BlossomClient
import com.stellarmls.chat.crypto.GroupCrypto
import com.stellarmls.chat.crypto.KeyAttestation
import com.stellarmls.chat.crypto.KeyManager
import com.stellarmls.chat.crypto.MediaCrypto
import com.stellarmls.chat.crypto.NostrEventBuilder
import com.stellarmls.chat.model.BootstrapPayload
import com.stellarmls.chat.model.ChatError
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.ChatMessage
import com.stellarmls.chat.model.MediaAttachment
import com.stellarmls.chat.model.MessageStatus
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
import com.stellarmls.mls.SEPGroupRenamed
import com.stellarmls.mls.SEPGroupStateUpdate
import com.stellarmls.mls.SEPKeyAttestationPayload
import com.stellarmls.mls.SEPMemberJoined
import com.stellarmls.mls.SEPMessageAck
import com.stellarmls.mls.SEPSaltRequest
import com.stellarmls.mls.SEPSaltResponse
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.security.SecureRandom
import java.util.Date

class GroupListViewModel(application: Application) : AndroidViewModel(application) {
    val keyManager = KeyManager.create(application)
    val groups = mutableStateListOf<ChatGroup>()
    val pendingInvitations = mutableStateListOf<PendingInvitation>()

    // Persistent chat message storage — keyed by group ID, alive for entire app session.
    // Uses SnapshotStateMap so Compose recomposes when messages change.
    val chatMessages = androidx.compose.runtime.mutableStateMapOf<String, List<ChatMessage>>()
    private val seenMessageIDs = mutableMapOf<String, MutableSet<String>>()
    /** Unread message count per group. Reset when the user opens the chat. */
    val unreadCounts = androidx.compose.runtime.mutableStateMapOf<String, Int>()
    /** The group ID currently being viewed, used to suppress unread increments. */
    var activeGroupID: String? = null

    val store = PersistenceStore(application)

    // Relay management
    val relayURLs = mutableStateListOf<String>()
    val blossomServerURLs = mutableStateListOf<String>()
    private val relayPrefs = application.getSharedPreferences("stellar_relays", Context.MODE_PRIVATE)
    private val blossomPrefs = application.getSharedPreferences("stellar_blossom", Context.MODE_PRIVATE)

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
    // N-2: Thread-safe collections for concurrent access from relay callbacks
    private val saltHistory = java.util.concurrent.ConcurrentHashMap<String, java.util.concurrent.ConcurrentHashMap<Long, ByteArray>>()
    // Replay protection: processed protocol event IDs (H-7)
    // N-8: Bounded LRU set to prevent unbounded memory growth
    private val processedProtocolEventIDs: MutableSet<String> = java.util.Collections.synchronizedSet(
        java.util.LinkedHashSet<String>()
    )
    // Salt request rate limiting: "senderPubkey:epoch" keys already responded to (H-5)
    private val saltRequestsResponded = java.util.Collections.newSetFromMap(
        java.util.concurrent.ConcurrentHashMap<String, Boolean>()
    )

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
            relayURLs.addAll(listOf(
                "wss://relay.damus.io",
                "wss://nos.lol",
                "wss://relay.nostr.band",
                "wss://relay.snort.social",
                "wss://nostr.wine"
            ))
        }

        // Load Blossom server URLs
        val savedBlossom = blossomPrefs.getString("blossom_urls", null)
        if (savedBlossom != null) {
            blossomServerURLs.addAll(savedBlossom.split(",").filter { it.isNotBlank() })
        } else {
            blossomServerURLs.add("https://nostr.download")
        }

        // Load contract + relayer config
        contractEndpoint = contractPrefs.getString("endpoint", "") ?: ""
        contractID = contractPrefs.getString("contract_id", "") ?: ""
        relayerURL = contractPrefs.getString("relayer_url", "") ?: ""
        // N-5: Load auth token from encrypted prefs (migrate from plaintext if needed)
        val legacyToken = contractPrefs.getString("relayer_auth_token", null)
        if (legacyToken != null && legacyToken.isNotBlank()) {
            keyManager.saveRelayerAuthToken(legacyToken)
            contractPrefs.edit().remove("relayer_auth_token").apply()
        }
        relayerAuthToken = keyManager.loadRelayerAuthToken()

        // Initialize transports
        transport = NostrMessageTransport(keyManager, relayURLs.toList())
        invitationTransport = InvitationTransport(keyManager)

        // Initialize on-chain service if configured
        configureContract()

        // Load persisted groups, messages, and initialize salt history
        viewModelScope.launch {
            try {
                val loaded = store.loadGroups()
                groups.addAll(loaded)
                // Populate currentMembers from all persisted groups
                transport.currentMembers.addAll(loaded.flatMap { it.members })
                for (group in loaded) {
                    storeSalt(group.id, group.epoch, group.salt)
                    // Load persisted chat messages
                    val msgs = store.loadMessages(group.id)
                    chatMessages[group.id] = msgs
                    seenMessageIDs[group.id] = msgs.map { it.id }.toMutableSet()
                }
                // Connect to relays and subscribe all loaded groups
                if (loaded.isNotEmpty()) {
                    connectIfNeeded()
                }
            } catch (_: Exception) { }
        }
    }

    fun connectIfNeeded() {
        if (!connected) {
            transport.connect()
            invitationTransport.connect(relayURLs.toList())
            startInboxListener()
            setupChatMessageHandler()
            setupImageMessageHandler()
            setupProtocolMessageHandler()
            setupOKHandler()
            connected = true

            // Subscribe all persisted groups for chat + protocol messages
            for (group in groups) {
                transport.subscribe(group)
            }
        }
    }

    fun reconnectRelays() {
        transport.disconnect()
        invitationTransport.disconnect()
        connected = false
        connectIfNeeded()
    }

    // -- Group lifecycle --

    fun addGroup(group: ChatGroup) {
        if (groups.none { it.id == group.id }) {
            groups.add(group)
            chatMessages[group.id] = emptyList()
            seenMessageIDs[group.id] = mutableSetOf()
            // Update transport members
            transport.currentMembers.clear()
            transport.currentMembers.addAll(groups.flatMap { it.members })
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
        chatMessages.remove(id)
        seenMessageIDs.remove(id)
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
                relayHints = group.relayHints,
                members = group.members.toList(),
                epoch = group.epoch,
                salt = group.salt,
                commitment = group.commitment
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

    // -- Blossom server management --

    fun addBlossomServer(urlString: String): Boolean {
        val trimmed = urlString.trim()
        if (!trimmed.startsWith("http://") && !trimmed.startsWith("https://")) return false
        if (blossomServerURLs.contains(trimmed)) return false
        blossomServerURLs.add(trimmed)
        persistBlossomServers()
        return true
    }

    fun removeBlossomServer(index: Int) {
        if (index in blossomServerURLs.indices) {
            blossomServerURLs.removeAt(index)
            persistBlossomServers()
        }
    }

    private fun persistBlossomServers() {
        blossomPrefs.edit().putString("blossom_urls", blossomServerURLs.joinToString(",")).apply()
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

    /** Save relayer configuration.
     *  N-5: Auth token stored in EncryptedSharedPreferences (via KeyManager's prefs)
     *  to prevent plaintext credential exposure. URL is non-sensitive and stays in contractPrefs. */
    fun saveRelayerConfig(url: String, authToken: String) {
        relayerURL = url.trim()
        relayerAuthToken = authToken.trim()
        contractPrefs.edit()
            .putString("relayer_url", relayerURL)
            .apply()
        // N-5: Store auth token in encrypted prefs instead of plaintext SharedPreferences
        keyManager.saveRelayerAuthToken(relayerAuthToken)
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

        private const val SALT_HISTORY_WINDOW = 64
        /** N-8: Max entries for dedup sets to prevent unbounded memory growth. */
        private const val MAX_DEDUP_SET_SIZE = 10_000
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
        val history = saltHistory.computeIfAbsent(groupID) { java.util.concurrent.ConcurrentHashMap() }
        history[epoch] = salt
        // Cap to last 64 epochs to prevent memory exhaustion
        if (history.size > SALT_HISTORY_WINDOW) {
            val oldest = history.keys.sorted().take(history.size - SALT_HISTORY_WINDOW)
            for (key in oldest) { history.remove(key) }
        }
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
    fun broadcastStateUpdate(group: ChatGroup, update: SEPGroupStateUpdate, overrideKey: ByteArray? = null) {
        val json = stateUpdateToJson(update)
        transport.sendProtocolMessage(group, json, overrideKey)
    }

    /**
     * Apply a received state update to a local group.
     * Handles three cases:
     * - update.epoch > local: straightforward apply (normal case)
     * - update.epoch == local: epoch fork — deterministic merge to resolve conflict
     * - update.epoch < local: stale update, ignored
     */
    fun applyStateUpdate(update: SEPGroupStateUpdate, groupID: String) {
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) return
        val group = groups[index]

        // Stale update — ignore
        if (update.epoch < group.epoch) return

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

        if (update.epoch == group.epoch) {
            // Same epoch + same salt = our own update echoed back from the relay — ignore.
            if (update.salt.contentEquals(group.salt)) {
                if (BuildConfig.DEBUG) {
                    Log.d("GroupListVM", "Ignoring own echo at epoch=${update.epoch} for group=${groupID.take(8)}")
                }
                return
            }

            // Epoch fork: two members made concurrent changes at the same epoch.
            // Deterministic merge: union members, lexicographic-smaller salt wins.
            val remoteSalt = update.salt
            val localSalt = group.salt

            // Merge: add remote's added members, remove remote's removed members
            for (removed in update.removedMemberKeys) {
                group.members.removeAll { it.publicKeyCompressed.contentEquals(removed) }
            }
            for (added in update.addedMembers) {
                if (group.members.none { it.publicKeyCompressed.contentEquals(added.publicKeyCompressed) }) {
                    group.members.add(added)
                }
            }
            sortMembers(group)

            // Deterministic salt selection: lexicographically smaller wins
            val useRemoteSalt = lexCompare(remoteSalt, localSalt) < 0
            group.epoch += 1
            group.salt = if (useRemoteSalt) remoteSalt else localSalt
            group.recomputeCommitment()

            groups[index] = group
            storeSalt(groupID, group.epoch, group.salt)

            transport.currentMembers.clear()
            transport.currentMembers.addAll(groups.flatMap { it.members })
            transport.subscribe(group)

            viewModelScope.launch {
                try { store.saveGroup(group) } catch (_: Exception) { }
            }

            // Broadcast the merged state so all members converge
            val mergedUpdate = buildStateUpdate(group)
            broadcastStateUpdate(group, mergedUpdate)
        } else {
            // Normal case: update.epoch > group.epoch
            for (removed in update.removedMemberKeys) {
                group.members.removeAll { it.publicKeyCompressed.contentEquals(removed) }
            }
            for (added in update.addedMembers) {
                if (group.members.none { it.publicKeyCompressed.contentEquals(added.publicKeyCompressed) }) {
                    group.members.add(added)
                }
            }
            sortMembers(group)

            group.epoch = update.epoch
            group.salt = update.salt
            if (update.commitment != null) {
                group.commitment = update.commitment
            }

            groups[index] = group
            storeSalt(groupID, update.epoch, update.salt)

            transport.currentMembers.clear()
            transport.currentMembers.addAll(groups.flatMap { it.members })
            transport.subscribe(group)

            viewModelScope.launch {
                try { store.saveGroup(group) } catch (_: Exception) { }
            }
        }
    }

    /** Rotate the group key without membership changes. Provides forward secrecy. */
    fun rotateGroupKey(groupID: String) {
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) return
        val group = groups[index]

        val previousKey = group.encryptionKey

        group.epoch++
        group.salt = SEPCommitmentBuilder.generateSalt()
        group.recomputeCommitment()

        groups[index] = group
        storeSalt(groupID, group.epoch, group.salt)

        val update = buildStateUpdate(group)
        broadcastStateUpdate(group, update, overrideKey = previousKey)

        transport.currentMembers.clear()
        transport.currentMembers.addAll(groups.flatMap { it.members })
        transport.subscribe(group)

        viewModelScope.launch {
            try { store.saveGroup(group) } catch (_: Exception) { }
        }
    }

    /** Remove a member from a group, broadcast state update, and rotate keys. */
    fun removeMember(blsPubkey: ByteArray, groupID: String) {
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) return
        val group = groups[index]

        if (group.members.none { it.publicKeyCompressed.contentEquals(blsPubkey) }) return

        // Capture old key for broadcasting
        val previousKey = group.encryptionKey

        group.members.removeAll { it.publicKeyCompressed.contentEquals(blsPubkey) }
        group.epoch++
        group.salt = SEPCommitmentBuilder.generateSalt()
        group.recomputeCommitment()

        groups[index] = group
        storeSalt(groupID, group.epoch, group.salt)

        transport.currentMembers.clear()
        transport.currentMembers.addAll(groups.flatMap { it.members })

        // Broadcast removal encrypted with the PREVIOUS key
        val update = buildStateUpdate(group, removedMemberKeys = listOf(blsPubkey))
        broadcastStateUpdate(group, update, overrideKey = previousKey)

        transport.subscribe(group)

        viewModelScope.launch {
            try { store.saveGroup(group) } catch (_: Exception) { }
        }
    }

    private fun sortMembers(group: ChatGroup) {
        group.members.sortWith { a, b ->
            val aKey = a.publicKeyCompressed
            val bKey = b.publicKeyCompressed
            for (i in 0 until minOf(aKey.size, bKey.size)) {
                val cmp = (aKey[i].toInt() and 0xFF) - (bKey[i].toInt() and 0xFF)
                if (cmp != 0) return@sortWith cmp
            }
            aKey.size - bKey.size
        }
    }

    private fun lexCompare(a: ByteArray, b: ByteArray): Int {
        for (i in 0 until minOf(a.size, b.size)) {
            val cmp = (a[i].toInt() and 0xFF) - (b[i].toInt() and 0xFF)
            if (cmp != 0) return cmp
        }
        return a.size - b.size
    }

    /** Handle a member_joined announcement: add the joiner and broadcast updated state. */
    private fun handleMemberJoined(member: SEPGroupMemberLeaf, groupID: String) {
        if (BuildConfig.DEBUG) android.util.Log.d("GroupListVM", "handleMemberJoined group=${groupID.take(8)}")
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) {
            if (BuildConfig.DEBUG) android.util.Log.w("GroupListVM", "handleMemberJoined: group not found")
            return
        }
        val group = groups[index]

        // Skip if already a member
        if (group.members.any { it.publicKeyCompressed.contentEquals(member.publicKeyCompressed) }) {
            if (BuildConfig.DEBUG) android.util.Log.d("GroupListVM", "handleMemberJoined: already a member, skipping")
            return
        }

        // Capture old encryption key BEFORE bumping epoch/salt.
        // The state update must be encrypted with the current key so all
        // existing members (including the joiner) can decrypt it.
        val previousKey = group.encryptionKey

        // Add the joiner (bumps epoch and salt internally)
        group.addMember(member)

        groups[index] = group
        storeSalt(groupID, group.epoch, group.salt)
        viewModelScope.launch {
            try { store.saveGroup(group) } catch (_: Exception) { }
        }

        // Refresh transport member list
        transport.currentMembers.clear()
        transport.currentMembers.addAll(groups.flatMap { it.members })

        // Broadcast state update encrypted with the PREVIOUS key so everyone can read it
        val update = SEPGroupStateUpdate(
            epoch = group.epoch,
            salt = group.salt,
            addedMembers = listOf(member),
            commitment = group.commitment
        )
        broadcastStateUpdate(group, update, overrideKey = previousKey)
        if (BuildConfig.DEBUG) android.util.Log.d("GroupListVM", "broadcastStateUpdate SENT group=${groupID.take(8)} epoch=${group.epoch} members=${group.members.size}")

        // Resubscribe with new key after epoch change
        transport.subscribe(group)
    }

    /** Apply a group rename received via protocol message. */
    private fun applyGroupRenamed(newName: String, groupID: String) {
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) return
        val updated = groups[index].copy(name = newName)
        groups[index] = updated
        viewModelScope.launch {
            try { store.saveGroup(updated) } catch (_: Exception) { }
        }
    }

    /** Announce ourselves as a new member to the group over the Nostr transport. */
    fun announceMemberJoined(group: ChatGroup) {
        val member = keyManager.memberLeaf()
        val json = JSONObject().apply {
            put("type", SEPMemberJoined.MESSAGE_TYPE)
            put("member", JSONObject().apply {
                put("publicKeyCompressed", android.util.Base64.encodeToString(
                    member.publicKeyCompressed, android.util.Base64.NO_WRAP))
                put("leafHash", android.util.Base64.encodeToString(
                    member.leafHash, android.util.Base64.NO_WRAP))
            })
        }.toString()
        transport.sendProtocolMessage(group, json)
    }

    /** Set up handler for incoming chat messages — stores in chatMessages, persists to store. */
    private fun setupChatMessageHandler() {
        transport.onMessage = { groupID, senderPubkey, text, eventID, timestamp ->
            val seen = seenMessageIDs.getOrPut(groupID) { mutableSetOf() }
            if (seen.add(eventID)) {
                val msg = ChatMessage(
                    id = eventID,
                    groupID = groupID,
                    senderPubkey = senderPubkey,
                    text = text,
                    timestamp = Date(timestamp * 1000),
                    isMine = senderPubkey == keyManager.publicKeyHex
                )
                // Replace the list to trigger Compose recomposition
                val current = (chatMessages[groupID] ?: emptyList()) + msg
                chatMessages[groupID] = current.sortedWith(compareBy<ChatMessage> { it.timestamp }.thenBy { it.id })
                viewModelScope.launch {
                    try { store.saveMessage(msg) } catch (_: Exception) { }
                }
                // Increment unread count if this group isn't currently active
                if (!msg.isMine && activeGroupID != groupID) {
                    unreadCounts[groupID] = (unreadCounts[groupID] ?: 0) + 1
                }
                // Send delivery ACK for non-mine messages (fire-and-forget)
                if (!msg.isMine) {
                    val group = groups.find { it.id == groupID }
                    if (group != null) {
                        val ackJson = JSONObject().apply {
                            put("type", SEPMessageAck.MESSAGE_TYPE)
                            put("eventID", eventID)
                        }.toString()
                        transport.sendProtocolMessage(group, ackJson)
                    }
                }
                // Update lastEventTimestamp
                val group = groups.find { it.id == groupID }
                if (group != null && timestamp > group.lastEventTimestamp) {
                    group.lastEventTimestamp = timestamp
                    viewModelScope.launch {
                        try { store.saveGroup(group) } catch (_: Exception) { }
                    }
                }
            }
        }
    }

    /** Set up handler for incoming image messages — stores in chatMessages with media attachment. */
    private fun setupImageMessageHandler() {
        transport.onImageMessage = { groupID, text, media, eventID, senderPubkey, timestamp ->
            val seen = seenMessageIDs.getOrPut(groupID) { mutableSetOf() }
            if (seen.add(eventID)) {
                val msg = ChatMessage(
                    id = eventID,
                    groupID = groupID,
                    senderPubkey = senderPubkey,
                    text = text,
                    timestamp = Date(timestamp * 1000),
                    isMine = senderPubkey == keyManager.publicKeyHex,
                    mediaAttachment = media
                )
                val current = (chatMessages[groupID] ?: emptyList()) + msg
                chatMessages[groupID] = current.sortedWith(compareBy<ChatMessage> { it.timestamp }.thenBy { it.id })
                viewModelScope.launch {
                    try { store.saveMessage(msg) } catch (_: Exception) { }
                }
                if (!msg.isMine && activeGroupID != groupID) {
                    unreadCounts[groupID] = (unreadCounts[groupID] ?: 0) + 1
                }
                // Send delivery ACK for non-mine image messages (fire-and-forget)
                if (!msg.isMine) {
                    val group = groups.find { it.id == groupID }
                    if (group != null) {
                        val ackJson = JSONObject().apply {
                            put("type", SEPMessageAck.MESSAGE_TYPE)
                            put("eventID", eventID)
                        }.toString()
                        transport.sendProtocolMessage(group, ackJson)
                    }
                }
                val group = groups.find { it.id == groupID }
                if (group != null && timestamp > group.lastEventTimestamp) {
                    group.lastEventTimestamp = timestamp
                    viewModelScope.launch {
                        try { store.saveGroup(group) } catch (_: Exception) { }
                    }
                }
            }
        }
    }

    /** Wire relay OK responses to update message delivery status. */
    private fun setupOKHandler() {
        transport.onOK = { eventID, accepted ->
            val newStatus = if (accepted) MessageStatus.SENT else MessageStatus.FAILED
            for ((groupID, messages) in chatMessages) {
                val index = messages.indexOfFirst { it.id == eventID }
                if (index >= 0) {
                    val updated = messages.toMutableList()
                    updated[index] = updated[index].copy(status = newStatus)
                    chatMessages[groupID] = updated
                    break
                }
            }
        }
    }

    /** Send a chat message in a group. */
    fun sendMessage(groupID: String, text: String) {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) return
        val group = groups.find { it.id == groupID } ?: return

        // Use the deterministic NIP-01 event ID so the relay echo is deduplicated,
        // and the event's createdAt so timestamp matches what other clients see.
        val event = transport.send(group, trimmed)

        val msg = ChatMessage(
            id = event.id,
            groupID = groupID,
            senderPubkey = keyManager.publicKeyHex,
            text = trimmed,
            timestamp = Date(event.createdAt * 1000),
            isMine = true,
            status = MessageStatus.SENDING
        )
        // Replace the list to trigger Compose recomposition
        val current = (chatMessages[groupID] ?: emptyList()) + msg
        chatMessages[groupID] = current.sortedWith(compareBy<ChatMessage> { it.timestamp }.thenBy { it.id })
        seenMessageIDs.getOrPut(groupID) { mutableSetOf() }.add(event.id)
        viewModelScope.launch {
            try { store.saveMessage(msg) } catch (_: Exception) { }
        }
    }

    /** Send an encrypted image in a group via Blossom. */
    fun sendImage(groupID: String, imageData: ByteArray) {
        val group = groups.find { it.id == groupID } ?: return

        viewModelScope.launch {
            try {
                val compressed = withContext(Dispatchers.Default) {
                    MediaCrypto.compressImage(imageData)
                } ?: return@launch

                val dimensions = MediaCrypto.imageDimensions(compressed)
                val thumbData = withContext(Dispatchers.Default) {
                    MediaCrypto.generateThumbnail(compressed)
                }

                val (encryptedBlob, fileKey) = MediaCrypto.encryptMedia(compressed)
                val encThumb = thumbData?.let { MediaCrypto.encryptMedia(it, fileKey) }

                val blobHash = withContext(Dispatchers.IO) {
                    BlossomClient.upload(encryptedBlob, blossomServerURLs.toList(), keyManager)
                }

                val media = MediaAttachment(
                    blobHash = blobHash,
                    fileKey = fileKey,
                    mimeType = "image/jpeg",
                    width = dimensions?.first ?: 0,
                    height = dimensions?.second ?: 0,
                    size = encryptedBlob.size,
                    blossomServers = blossomServerURLs.toList(),
                    encryptedThumbnail = encThumb
                )

                // Build v2 wrapper with image type and media metadata
                val mediaJson = JSONObject().apply {
                    put("blobHash", media.blobHash)
                    put("fileKey", android.util.Base64.encodeToString(media.fileKey, android.util.Base64.NO_WRAP))
                    put("mimeType", media.mimeType)
                    put("width", media.width)
                    put("height", media.height)
                    put("size", media.size)
                    put("blossomServers", JSONArray(media.blossomServers))
                    if (encThumb != null) {
                        put("thumbnail", android.util.Base64.encodeToString(encThumb, android.util.Base64.NO_WRAP))
                    }
                }
                val wrapper = JSONObject().apply {
                    put("v", 2)
                    put("type", "image")
                    put("text", "\uD83D\uDDBC\uFE0F Image")
                    put("media", mediaJson)
                    put("senderBlsPubkey", android.util.Base64.encodeToString(
                        keyManager.blsPublicKey(), android.util.Base64.NO_WRAP))
                    put("ts", System.currentTimeMillis() / 1000)
                }

                val key = group.encryptionKey
                val envelopeJson = GroupCrypto.encrypt(wrapper.toString(), key)
                val content = android.util.Base64.encodeToString(
                    envelopeJson.toByteArray(), android.util.Base64.NO_WRAP)

                val tags = listOf(listOf("t", group.topicTag))
                val event = NostrEventBuilder.build(
                    kind = 44114,
                    tags = tags,
                    content = content,
                    keyManager = keyManager
                )

                // Publish to all relays
                transport.publish(event)

                val msg = ChatMessage(
                    id = event.id,
                    groupID = groupID,
                    senderPubkey = keyManager.publicKeyHex,
                    text = "\uD83D\uDDBC\uFE0F Image",
                    timestamp = Date(event.createdAt * 1000),
                    isMine = true,
                    status = MessageStatus.SENDING,
                    mediaAttachment = media
                )
                val current = (chatMessages[groupID] ?: emptyList()) + msg
                chatMessages[groupID] = current.sortedWith(compareBy<ChatMessage> { it.timestamp }.thenBy { it.id })
                seenMessageIDs.getOrPut(groupID) { mutableSetOf() }.add(event.id)
                launch {
                    try { store.saveMessage(msg) } catch (_: Exception) { }
                }
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) android.util.Log.e("GroupListVM", "sendImage failed: ${e.message}")
            }
        }
    }

    /** Set up handler for protocol messages received on the group channel. */
    private fun setupProtocolMessageHandler() {
        transport.onProtocolMessage = { groupID, json, eventID, senderPubkey ->
            // Replay protection: skip already-processed protocol events (H-7)
            // N-8: Evict oldest entries when set exceeds max size
            val isNew = synchronized(processedProtocolEventIDs) {
                if (processedProtocolEventIDs.size >= MAX_DEDUP_SET_SIZE) {
                    val iter = processedProtocolEventIDs.iterator()
                    if (iter.hasNext()) { iter.next(); iter.remove() }
                }
                processedProtocolEventIDs.add(eventID)
            }
            if (isNew) {
                try {
                    val obj = JSONObject(json)
                    val msgType = obj.optString("type")
                    if (BuildConfig.DEBUG) android.util.Log.d("GroupListVM", "Protocol msg type=$msgType group=${groupID.take(8)}")
                    when (msgType) {
                        SEPMemberJoined.MESSAGE_TYPE -> {
                            val memberObj = obj.getJSONObject("member")
                            val member = SEPGroupMemberLeaf(
                                publicKeyCompressed = android.util.Base64.decode(
                                    memberObj.getString("publicKeyCompressed"), android.util.Base64.NO_WRAP),
                                leafHash = android.util.Base64.decode(
                                    memberObj.getString("leafHash"), android.util.Base64.NO_WRAP)
                            )
                            handleMemberJoined(member, groupID)
                        }
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
                        SEPGroupRenamed.MESSAGE_TYPE -> {
                            val newName = obj.getString("name")
                            applyGroupRenamed(newName, groupID)
                        }
                        SEPMessageAck.MESSAGE_TYPE -> {
                            val ackEventID = obj.getString("eventID")
                            // Update message status to DELIVERED if we sent it
                            val messages = chatMessages[groupID]
                            if (messages != null) {
                                val idx = messages.indexOfFirst { it.id == ackEventID && it.isMine }
                                if (idx >= 0) {
                                    chatMessages[groupID] = messages.toMutableList().also {
                                        it[idx] = it[idx].copy(status = MessageStatus.DELIVERED)
                                    }
                                }
                            }
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
