package chat.onym.android.viewmodel

import android.app.Application
import android.content.Context
import android.util.Log
import chat.onym.android.BuildConfig
import chat.onym.android.SecurityLog
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import chat.onym.android.blossom.BlossomClient
import chat.onym.android.crypto.GroupCrypto
import chat.onym.android.crypto.KeyAttestation
import chat.onym.android.crypto.KeyManager
import chat.onym.android.crypto.MediaCrypto
import chat.onym.android.crypto.NostrEvent
import chat.onym.android.crypto.NostrEventBuilder
import chat.onym.android.model.BootstrapPayload
import chat.onym.android.model.hexToBytes
import chat.onym.android.model.ChatError
import chat.onym.android.model.ChatGroup
import chat.onym.android.model.ChatMessage
import chat.onym.android.model.EpochSnapshot
import chat.onym.android.model.MediaAttachment
import chat.onym.android.model.MessageStatus
import chat.onym.android.model.InviteCode
import chat.onym.android.model.PendingInvitation
import chat.onym.android.model.toHex
import chat.onym.android.nostr.InvitationTransport
import chat.onym.android.nostr.NostrMessageTransport
import chat.onym.android.onchain.OnChainService
import chat.onym.android.onchain.OnChainVerificationResult
import chat.onym.android.persistence.ContactAliasStore
import chat.onym.android.persistence.InvitedContactStore
import chat.onym.android.persistence.PersistenceStore
import chat.onym.android.crypto.StorageEncryption
import chat.onym.android.push.PushNotificationManager
import chat.onym.android.push.PushTokenProviderFactory
import com.stellarmls.mls.RustBackedNostrSigner
import com.stellarmls.mls.SEPCommitmentBuilder
import com.stellarmls.mls.SEPGroupMemberLeaf
import com.stellarmls.mls.SEPGroupRenamed
import com.stellarmls.mls.SEPGroupStateUpdate
import com.stellarmls.mls.SEPKeyAttestationPayload
import com.stellarmls.mls.SEPMemberJoined
import com.stellarmls.mls.SEPRekey
import com.stellarmls.mls.SEPRekeyAck
import com.stellarmls.mls.SEPRekeyEnvelope
import com.stellarmls.mls.SEPRekeyResendRequest
import com.stellarmls.mls.SEPRemovalNotice
import com.stellarmls.mls.SEPMemberTransportBundle
import com.stellarmls.mls.SEPMessageAck
import com.stellarmls.mls.SEPTier
import com.stellarmls.mls.SEPSaltRequest
import com.stellarmls.mls.SEPSaltResponse
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.security.SecureRandom
import java.util.Date

// -- Blockchain-First Epoch Authority Types --

/** Result of attempting an epoch transition through the chain-confirmed helper. */
enum class EpochTransitionResult {
    SUCCESS,
    CHAIN_REJECTED,
    CHAIN_UNAVAILABLE,
    CHAIN_MISMATCH,
    SYNC_REQUIRED,
    BUNDLES_MISSING
}

/** The kind of epoch transition being performed. */
sealed class EpochTransitionKind {
    data class MemberAdd(val member: SEPGroupMemberLeaf) : EpochTransitionKind()
    data class MemberRemove(val blsPubkey: ByteArray) : EpochTransitionKind()
    object KeyRotation : EpochTransitionKind()
}

/** Per-group pending transition state. While awaiting chain confirmation,
 *  no additional epoch-changing actions are permitted for the group. */
sealed class PendingTransitionState {
    object Idle : PendingTransitionState()
    data class AwaitingChainConfirmation(val targetEpoch: Long) : PendingTransitionState()
}

class GroupListViewModel(application: Application) : AndroidViewModel(application) {
    var keyManager by mutableStateOf(KeyManager.create(application))
        private set

    /** Replace the active key manager after a BIP39 restore.
     *  Reconnects relays so the inbox subscription uses the new identity's keys. */
    fun replaceKeyManager(newKeyManager: KeyManager) {
        keyManager = newKeyManager
        reconnectRelays()
    }
    val groups = mutableStateListOf<ChatGroup>()
    val pendingInvitations = mutableStateListOf<PendingInvitation>()

    /** Whether at least one relay is connected. Observable by Compose. */
    var isRelayConnected by mutableStateOf(false)
        private set

    // Persistent chat message storage — keyed by group ID, alive for entire app session.
    // Uses SnapshotStateMap so Compose recomposes when messages change.
    val chatMessages = androidx.compose.runtime.mutableStateMapOf<String, List<ChatMessage>>()
    private val seenMessageIDs = java.util.concurrent.ConcurrentHashMap<String, MutableSet<String>>()
    /** Unread message count per group. Reset when the user opens the chat. */
    val unreadCounts = androidx.compose.runtime.mutableStateMapOf<String, Int>()
    /** The group ID currently being viewed, used to suppress unread increments. */
    var activeGroupID: String? = null

    val store = PersistenceStore(application)
    val contactAliasStore = ContactAliasStore()
    val invitedContactStore = InvitedContactStore()

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

    // Default group capacity
    var defaultGroupTier by mutableStateOf(SEPTier.LARGE)
        private set

    // Relayer configuration (fee decoupling)
    var relayerURL by mutableStateOf("")
        private set
    var relayerAuthToken by mutableStateOf("")
        private set
    val isRelayerConfigured: Boolean
        get() = relayerURL.isNotBlank()

    // TURN server configuration
    val turnURLs = mutableStateListOf<String>()
    var turnEnabled by mutableStateOf(false)
        private set
    var turnUsername by mutableStateOf("")
        private set
    var turnPassword by mutableStateOf("")
        private set
    private val turnPrefs = application.getSharedPreferences("stellar_turn", Context.MODE_PRIVATE)

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
    /** Per-group pending epoch transition state (for UI status). */
    private val pendingTransitions = java.util.concurrent.ConcurrentHashMap<String, PendingTransitionState>()
    /** Per-group chain publish jobs — stored so they can be cancelled when a newer transition supersedes. */
    private val chainPublishJobs = java.util.concurrent.ConcurrentHashMap<String, kotlinx.coroutines.Job>()
    /** Last successfully published on-chain state per group — used as proof baseline for the next chain publish. */
    private data class ChainBaseline(val members: List<SEPGroupMemberLeaf>, val epoch: Long, val salt: ByteArray)
    private val chainBaseline = java.util.concurrent.ConcurrentHashMap<String, ChainBaseline>()

    /** In-memory transport bundle directory: groupID → (blsPubkeyHex → bundle). */
    private val transportBundles = java.util.concurrent.ConcurrentHashMap<String, MutableMap<String, com.stellarmls.mls.SEPMemberTransportBundle>>()

    /** Epoch snapshots for epoch branching: groupID → (epoch → snapshot). */
    val epochSnapshots = java.util.concurrent.ConcurrentHashMap<String, java.util.concurrent.ConcurrentHashMap<Long, EpochSnapshot>>()

    // Calls
    lateinit var callManager: chat.onym.android.call.CallManager
        private set

    // Push notifications
    private var pushManager: PushNotificationManager? = null
    private val pushPrefs = application.getSharedPreferences("stellar_push_config", Context.MODE_PRIVATE)
    private val pushTokenProvider = PushTokenProviderFactory.create(application)
    /** Observable push-enabled state per group — separate from groups list for reliable Compose recomposition. */
    val pushEnabledStates = androidx.compose.runtime.mutableStateMapOf<String, Boolean>()
    /** Set by MainActivity.onNewIntent to navigate to a chat from a notification tap. */
    var pendingNavigationGroupId by androidx.compose.runtime.mutableStateOf<String?>(null)

    // Transports
    lateinit var transport: NostrMessageTransport
        private set
    lateinit var invitationTransport: InvitationTransport
        private set

    /** Kind-aware event router. Prevents wrong-transport bugs by routing based on event kind.
     *  kind 34113 → invitationTransport (inbox, per-recipient)
     *  kind 44114 → transport (group broadcast) */
    private suspend fun publishEvent(event: NostrEvent, relayURLs: List<String> = emptyList()) {
        when (event.kind) {
            34113 -> invitationTransport.publishToRelays(event, relayURLs)
            44114 -> transport.publish(event)
            else -> error("publishEvent: unknown event kind ${event.kind}")
        }
    }

    private var connected = false

    private fun pushEnabledPrefKey(groupID: String) = "push_enabled_$groupID"

    private fun resolvedPushEnabled(groupID: String, fallback: Boolean): Boolean {
        return if (pushPrefs.contains(pushEnabledPrefKey(groupID))) {
            pushPrefs.getBoolean(pushEnabledPrefKey(groupID), fallback)
        } else {
            fallback
        }
    }

    private fun persistPushEnabled(groupID: String, enabled: Boolean) {
        pushPrefs.edit().putBoolean(pushEnabledPrefKey(groupID), enabled).apply()
    }

    private fun applyLocalPushState(group: ChatGroup): ChatGroup {
        val enabled = resolvedPushEnabled(group.id, group.pushNotificationsEnabled)
        return if (group.pushNotificationsEnabled == enabled) {
            group
        } else {
            group.copy(pushNotificationsEnabled = enabled)
        }
    }

    init {
        // Load relay URLs
        val savedRelays = relayPrefs.getString("relay_urls", null)
        if (savedRelays != null) {
            relayURLs.addAll(savedRelays.split(",").filter { it.isNotBlank() })
        } else {
            if (BuildConfig.DEFAULT_NOSTR_RELAY.isNotEmpty()) {
                relayURLs.add(BuildConfig.DEFAULT_NOSTR_RELAY)
            }
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
            if (BuildConfig.DEFAULT_BLOSSOM_SERVER.isNotEmpty()) {
                blossomServerURLs.add(BuildConfig.DEFAULT_BLOSSOM_SERVER)
            }
            blossomServerURLs.add("https://nostr.download")
        }

        // Load default group tier
        val savedTierId = if (contractPrefs.contains("default_tier")) contractPrefs.getInt("default_tier", SEPTier.MEDIUM.id) else SEPTier.MEDIUM.id
        defaultGroupTier = SEPTier.entries.find { it.id == savedTierId } ?: SEPTier.MEDIUM

        // Load contract + relayer config (fall back to build-time defaults from relayer/.env)
        contractEndpoint = contractPrefs.getString("endpoint", null)
            ?: BuildConfig.DEFAULT_CONTRACT_ENDPOINT
        contractID = contractPrefs.getString("contract_id", null)
            ?: BuildConfig.DEFAULT_CONTRACT_ID
        relayerURL = contractPrefs.getString("relayer_url", null)
            ?: BuildConfig.DEFAULT_RELAYER_URL
        // N-5: Load auth token from encrypted prefs (migrate from plaintext if needed)
        val legacyToken = contractPrefs.getString("relayer_auth_token", null)
        if (legacyToken != null && legacyToken.isNotBlank()) {
            keyManager.saveRelayerAuthToken(legacyToken)
            contractPrefs.edit().remove("relayer_auth_token").apply()
        }
        relayerAuthToken = keyManager.loadRelayerAuthToken()
            .ifEmpty { BuildConfig.DEFAULT_RELAYER_AUTH_TOKEN }

        // Load TURN server config
        turnEnabled = turnPrefs.getBoolean("turn_enabled", false)
        val savedTurnURLs = turnPrefs.getString("turn_urls", null)
        if (savedTurnURLs != null) {
            turnURLs.addAll(savedTurnURLs.split(",").filter { it.isNotBlank() })
        }
        turnUsername = keyManager.loadTurnUsername()
        turnPassword = keyManager.loadTurnPassword()

        // Initialize transports
        transport = NostrMessageTransport(keyManager, relayURLs.toList())
        invitationTransport = InvitationTransport(keyManager)

        // Initialize call manager and telecom integration
        chat.onym.android.call.CallManager.initialize(application)
        callManager = chat.onym.android.call.CallManager(application)
        chat.onym.android.call.CallConnectionService.initialize(application, callManager)
        syncTurnConfig()

        // Initialize on-chain service if configured
        configureContract()

        // Wire push token refresh callback
        pushTokenProvider.setTokenRefreshCallback { token ->
            onNewPushToken(token)
        }

        // Load contact aliases
        viewModelScope.launch {
            try {
                contactAliasStore.load(store.dao)
            } catch (e: Exception) {
                Log.e("GroupListVM", "Failed to load contact aliases", e)
            }
        }

        // Load invited contacts (Telegram-invite tracking)
        viewModelScope.launch {
            try {
                invitedContactStore.load(store.dao)
            } catch (e: Exception) {
                Log.e("GroupListVM", "Failed to load invited contacts", e)
            }
        }

        // Load persisted groups, messages, and initialize salt history
        viewModelScope.launch {
            try {
                val loaded = store.loadGroups().map { applyLocalPushState(it) }
                Log.d("GroupListVM", "Loaded ${loaded.size} groups from store")
                groups.addAll(loaded)
                // Populate push states from local preferences
                for (g in loaded) { pushEnabledStates[g.id] = g.pushNotificationsEnabled }
                // Populate currentMembers from all persisted groups
                transport.currentMembers.addAll(loaded.flatMap { it.members })
                for (group in loaded) {
                    storeSalt(group.id, group.epoch, group.salt)
                    // Load persisted chat messages
                    try {
                        val msgs = store.loadMessages(group.id)
                        chatMessages[group.id] = msgs
                        seenMessageIDs[group.id] = java.util.Collections.synchronizedSet(msgs.map { it.id }.toMutableSet())
                    } catch (e: Exception) {
                        Log.e("GroupListVM", "Failed to load messages for group ${group.id.take(8)}", e)
                    }
                    // Load transport bundles
                    try {
                        val bundles = store.loadTransportBundles(group.id)
                        if (bundles.isNotEmpty()) {
                            transportBundles[group.id] = bundles.toMutableMap()
                        }
                    } catch (_: Exception) { }
                    // Load epoch snapshots
                    try {
                        val snapshots = store.loadEpochSnapshots(group.id)
                        if (snapshots.isNotEmpty()) {
                            epochSnapshots[group.id] = java.util.concurrent.ConcurrentHashMap(snapshots)
                        }
                    } catch (_: Exception) { }
                }
            } catch (e: Exception) {
                Log.e("GroupListVM", "Failed to load groups from store", e)
            }
            // Clean up expired pending rekeys (>24h) on startup
            try {
                val allPending = store.loadAllPendingRekeys()
                val now = System.currentTimeMillis()
                val expiryMs = 24 * 60 * 60 * 1000L
                for (record in allPending) {
                    if (now - record.createdAt > expiryMs) {
                        store.deletePendingRekey(record.groupID, record.epoch)
                    }
                }
            } catch (_: Exception) { }

            // Always connect to relays, even if group loading fails
            connectIfNeeded()
        }
    }

    fun connectIfNeeded() {
        if (!connected) {
            transport.onConnectionChange = {
                viewModelScope.launch { isRelayConnected = transport.isAnyRelayConnected }
            }
            transport.connect()
            invitationTransport.connect(relayURLs.toList())
            startInboxListener()
            setupChatMessageHandler()
            setupImageMessageHandler()
            setupProtocolMessageHandler()
            setupCallSignalHandler()
            setupOKHandler()
            connected = true

            // Subscribe all persisted groups for chat + protocol messages
            for (group in groups) {
                transport.subscribe(
                    group = group,
                    keyResolver = { epoch -> epochKey(group.id, epoch) }
                )
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
        val resolvedGroup = applyLocalPushState(group)
        if (groups.none { it.id == resolvedGroup.id }) {
            groups.add(resolvedGroup)
            pushEnabledStates[resolvedGroup.id] = resolvedGroup.pushNotificationsEnabled
            chatMessages[resolvedGroup.id] = emptyList()
            seenMessageIDs[resolvedGroup.id] = java.util.Collections.synchronizedSet(mutableSetOf())
            // Update transport members
            updateCurrentMembers(resolvedGroup.id)
            connectIfNeeded()
            transport.subscribe(
                group = resolvedGroup,
                keyResolver = { epoch -> epochKey(resolvedGroup.id, epoch) }
            )
            viewModelScope.launch {
                try { store.saveGroup(resolvedGroup) } catch (_: Exception) { }
            }
        }
    }

    fun updateGroup(group: ChatGroup) {
        val resolvedGroup = applyLocalPushState(group)
        val index = groups.indexOfFirst { it.id == group.id }
        if (index >= 0) {
            groups[index] = resolvedGroup
        }
        pushEnabledStates[resolvedGroup.id] = resolvedGroup.pushNotificationsEnabled
        viewModelScope.launch {
            try { store.saveGroup(resolvedGroup) } catch (_: Exception) { }
        }
    }

    fun togglePinGroup(id: String) {
        val idx = groups.indexOfFirst { it.id == id }
        if (idx < 0) return
        val g = groups[idx].copy(isPinned = !groups[idx].isPinned)
        groups[idx] = g
        // Re-sort so pinned/unpinned chat moves immediately
        val currentIDs = groups.map { it.id }
        val sortedIDs = groups.sortedWith(compareByDescending<ChatGroup> { it.isPinned }
            .thenByDescending { if (it.lastMessageAt > 0) it.lastMessageAt else it.createdAt.time }
        ).map { it.id }
        if (sortedIDs != currentIDs) {
            val byID = groups.associateBy { it.id }
            groups.clear()
            sortedIDs.forEach { sid -> byID[sid]?.let { groups.add(it) } }
        }
        viewModelScope.launch {
            try { store.saveGroup(g) } catch (e: Exception) {
                Log.e("GroupListVM", "Failed to persist pin toggle for group ${id.take(8)}", e)
            }
        }
    }

    fun removeGroup(id: String) {
        groups.removeAll { it.id == id }
        chatMessages.remove(id)
        seenMessageIDs.remove(id)
        epochSnapshots.remove(id)
        pushEnabledStates.remove(id)
        viewModelScope.launch {
            try {
                pushPrefs.edit().remove(pushEnabledPrefKey(id)).apply()
                store.deleteGroup(id)
            } catch (_: Exception) { }
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
                commitment = group.commitment,
                tierRawValue = group.tier.id,
                groupTypeRawValue = group.groupType.id,
                adminPubkeys = group.adminPubkeys.toList()
            )

            addGroup(group)
            ensureOwnTransportBundle(group.id)
            // Capture initial epoch snapshot
            captureEpochSnapshot(group, "Group created")
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
        if (BuildConfig.DEBUG) {
            Log.d(
                "InvitationTransport",
                "startInboxListener inboxTag=${keyManager.inboxTag} relays=${relayURLs.joinToString(prefix = "[", postfix = "]")}"
            )
        }
        invitationTransport.onInvitation = { invitation ->
            // Deduplicate
            if (BuildConfig.DEBUG) {
                Log.d(
                    "InvitationTransport",
                    "onInvitation id=${invitation.id.take(12)} group=${invitation.payload.groupID.toHex().take(24)} epoch=${invitation.payload.epoch}"
                )
            }
            viewModelScope.launch {
                // Reconcile any pending InvitedContact when an onboard-ack arrives
                invitation.payload.onboardReplyNonce?.let { nonce ->
                    try {
                        val invited = invitedContactStore.contacts
                            .firstOrNull { it.handshakeNonce == nonce }
                        invitedContactStore.reconcile(
                            nonce = nonce,
                            resolvedPubkey = invitation.payload.senderNostrPubkey,
                            dao = store.dao
                        )
                        // Seed the alias from the phonebook displayName so the
                        // newly-reconciled contact shows up with a human-readable
                        // name instead of a raw hex pubkey.
                        if (invited != null &&
                            contactAliasStore.displayName(invitation.payload.senderNostrPubkey) == null
                        ) {
                            contactAliasStore.setAlias(
                                pubkey = invitation.payload.senderNostrPubkey,
                                name = invited.displayName,
                                dao = store.dao
                            )
                        }
                    } catch (e: Exception) {
                        if (BuildConfig.DEBUG) {
                            Log.w("GroupListVM", "Failed to reconcile invited contact: ${e.message}")
                        }
                    }
                }
                if (pendingInvitations.none { it.id == invitation.id } &&
                    groups.none { it.id == invitation.payload.groupID.toHex() }) {
                    pendingInvitations.add(invitation)
                }
            }
        }
        invitationTransport.onRekeyEnvelope = { envelopeObj ->
            viewModelScope.launch {
                applyRekeyEnvelope(envelopeObj)
            }
        }
        invitationTransport.subscribeToInbox(
            inboxTag = keyManager.inboxTag,
            privateKey = keyManager.keyAgreementPrivateKey
        )
        // Start rekey resend timer for reliability
        startRekeyResendTimer()
    }

    /**
     * Fetch on-chain state with a bounded retry for RPC lag. Delegates to the
     * top-level [chat.onym.android.onchain.fetchOnChainStateAwaitingEpoch]
     * helper (extracted so the retry logic can be unit-tested without
     * instantiating the full ViewModel).
     */
    private suspend fun fetchOnChainStateAwaitingEpoch(
        service: OnChainService,
        groupIDData: ByteArray,
        expectedEpoch: Long,
        maxAttempts: Int = 4,
        initialDelayMs: Long = 250L
    ): chat.onym.android.onchain.SEPCommitmentEntry {
        return chat.onym.android.onchain.fetchOnChainStateAwaitingEpoch(
            fetch = { withContext(Dispatchers.IO) { service.fetchOnChainState(groupIDData) } },
            expectedEpoch = expectedEpoch,
            maxAttempts = maxAttempts,
            initialDelayMs = initialDelayMs
        )
    }

    /** Apply a secure rekey envelope received via inbox. Installs fresh groupSecret/salt and migrates topic. */
    private fun applyRekeyEnvelope(obj: JSONObject) {
        val groupIDBytes = android.util.Base64.decode(obj.getString("groupID"), android.util.Base64.NO_WRAP)
        val groupIDHex = groupIDBytes.toHex()
        val epoch = obj.getLong("epoch")
        if (BuildConfig.DEBUG) Log.d("Rekey", "RECEIVED groupID=${groupIDHex.take(8)} epoch=$epoch")
        val index = groups.indexOfFirst { it.id == groupIDHex }
        if (index < 0) {
            if (BuildConfig.DEBUG) Log.w("GroupListVM", "applyRekeyEnvelope: group not found ${groupIDHex.take(8)}")
            return
        }
        var group = cloneGroup(groups[index])

        // Only apply if epoch is ahead
        if (epoch <= group.epoch) {
            if (BuildConfig.DEBUG) Log.w("GroupListVM", "applyRekeyEnvelope: stale epoch=$epoch <= current=${group.epoch}")
            return
        }

        // Verify sender bundle signature AND membership
        val senderBundleObj = obj.optJSONObject("senderBundle")
        if (senderBundleObj == null) {
            if (BuildConfig.DEBUG) Log.w("GroupListVM", "applyRekeyEnvelope: missing senderBundle")
            return
        }
        val senderBundle = BootstrapPayload.deserializeTransportBundle(senderBundleObj)
        if (!keyManager.verifyTransportBundle(senderBundle)) {
            if (BuildConfig.DEBUG) Log.w("GroupListVM", "applyRekeyEnvelope: sender bundle signature FAILED")
            SecurityLog.invalidAttestation("rekey envelope sender bundle verification failed")
            return
        }
        // Sender must be a current member of the group (not a removed ex-member)
        val senderHex = senderBundle.blsPubkey.toHex()
        if (group.members.none { it.publicKeyCompressed.toHex() == senderHex }) {
            if (BuildConfig.DEBUG) Log.w("GroupListVM", "Rejected rekey envelope: sender ${senderHex.take(8)} not a member")
            SecurityLog.nonMemberMessageRejected(groupIDHex)
            return
        }

        val oldTopicTag = group.topicTag

        // Compute candidate state from the envelope WITHOUT mutating `groups[index]` yet.
        // `cloneGroup` deep-copies `members`, `groupSecret`, `salt`, `commitment`
        // so `candidate.members.removeAll { ... }` below cannot leak back into
        // the stored group if we end up rejecting at chain verification.
        val candidate = cloneGroup(group).copy(
            groupSecret = android.util.Base64.decode(obj.getString("groupSecret"), android.util.Base64.NO_WRAP),
            salt = android.util.Base64.decode(obj.getString("salt"), android.util.Base64.NO_WRAP),
            epoch = epoch,
            commitment = obj.optString("commitment", "").takeIf { it.isNotEmpty() }?.let {
                android.util.Base64.decode(it, android.util.Base64.NO_WRAP)
            } ?: group.commitment?.copyOf()
        )

        // Apply member changes to candidate
        val removedArr = obj.optJSONArray("removedMemberKeys") ?: JSONArray()
        for (i in 0 until removedArr.length()) {
            val removedKey = android.util.Base64.decode(removedArr.getString(i), android.util.Base64.NO_WRAP)
            candidate.members.removeAll { it.publicKeyCompressed.contentEquals(removedKey) }
        }
        sortMembers(candidate)
        // Recompute commitment locally from the final member set so we can
        // cross-check against the chain's stored commitment.
        candidate.recomputeCommitment()

        // --- Chain-first verification (async for published groups). If the chain
        // disagrees (sender hasn't published, proof rejected, or RPC lagging),
        // refuse to install fresh secrets — otherwise we'd rotate away from a
        // key the rest of the group still uses. For unpublished groups, apply
        // directly.
        if (candidate.isPublishedOnChain) {
            val service = onChainService
            if (service == null) {
                if (BuildConfig.DEBUG) Log.w("Rekey", "DROPPED: onChainService unavailable for published group=${groupIDHex.take(8)}")
                return
            }
            viewModelScope.launch {
                val chainMatches: Boolean = try {
                    val entry = fetchOnChainStateAwaitingEpoch(service, groupIDBytes, expectedEpoch = epoch)
                    val localCommitment = candidate.commitment
                    localCommitment != null
                        && entry.active
                        && entry.epoch == epoch
                        && entry.commitment.contentEquals(localCommitment)
                } catch (e: Exception) {
                    if (BuildConfig.DEBUG) Log.w("Rekey", "DROPPED: chain fetch failed: ${e.message} group=${groupIDHex.take(8)}")
                    false
                }
                if (!chainMatches) {
                    if (BuildConfig.DEBUG) Log.w("Rekey", "REJECTED: chain disagrees at epoch=$epoch group=${groupIDHex.take(8)}")
                    return@launch
                }
                commitRemoteRekey(
                    groupIDHex = groupIDHex,
                    groupIDBytes = groupIDBytes,
                    candidate = candidate,
                    senderBundle = senderBundle,
                    obj = obj,
                    oldTopicTag = oldTopicTag,
                    epoch = epoch,
                    removedArr = removedArr
                )
            }
            return
        }

        // Unpublished draft — no chain authority, commit directly.
        commitRemoteRekey(
            groupIDHex = groupIDHex,
            groupIDBytes = groupIDBytes,
            candidate = candidate,
            senderBundle = senderBundle,
            obj = obj,
            oldTopicTag = oldTopicTag,
            epoch = epoch,
            removedArr = removedArr
        )
    }

    /**
     * Install a chain-verified candidate from an incoming rekey envelope. Called
     * from [applyRekeyEnvelope] after on-chain verification succeeds.
     */
    private fun commitRemoteRekey(
        groupIDHex: String,
        groupIDBytes: ByteArray,
        candidate: ChatGroup,
        senderBundle: com.stellarmls.mls.SEPMemberTransportBundle,
        obj: JSONObject,
        oldTopicTag: String,
        epoch: Long,
        removedArr: JSONArray
    ) {
        val idx = groups.indexOfFirst { it.id == groupIDHex }
        if (idx < 0) return
        val stored = groups[idx]
        // Re-check staleness: something could have advanced while we were
        // awaiting chain confirmation.
        if (epoch <= stored.epoch) return

        // Capture epoch snapshot of the pre-rekey state now that we're committing.
        captureEpochSnapshot(stored, "Secure rekey (member removed)")

        // Update transport bundles
        val bundlesArr = obj.optJSONArray("memberBundles") ?: JSONArray()
        for (i in 0 until bundlesArr.length()) {
            try {
                val bundle = BootstrapPayload.deserializeTransportBundle(bundlesArr.getJSONObject(i))
                processTransportBundle(bundle, groupIDHex)
            } catch (_: Exception) { }
        }
        processTransportBundle(senderBundle, groupIDHex)

        // Refresh chain baseline to match the now-confirmed state.
        if (candidate.isPublishedOnChain) {
            chainBaseline[groupIDHex] = ChainBaseline(
                candidate.members.toList(), candidate.epoch, candidate.salt.copyOf()
            )
        }

        groups[idx] = candidate
        storeSalt(groupIDHex, epoch, candidate.salt)

        // Migrate topic
        if (!BuildConfig.DEBUG) transport.unsubscribe(oldTopicTag)
        syncTransportAndSubscribe(candidate)

        viewModelScope.launch {
            try { store.saveGroup(candidate) } catch (_: Exception) { }
        }

        // System messages
        for (i in 0 until removedArr.length()) {
            val removedKey = android.util.Base64.decode(removedArr.getString(i), android.util.Base64.NO_WRAP)
            val name = memberDisplayName(removedKey)
            insertSystemMessage(groupIDHex, "$name was removed from the group", "member-remove", epoch)
        }

        if (BuildConfig.DEBUG) Log.d("Rekey", "INSTALLED groupID=${groupIDHex.take(8)} epoch=$epoch newTopic=${candidate.topicTag.take(8)}")

        // Send ack on the NEW topic
        val myBls = keyManager.blsPublicKey()
        val ackJson = JSONObject().apply {
            put("type", com.stellarmls.mls.SEPRekeyAck.MESSAGE_TYPE)
            put("groupID", android.util.Base64.encodeToString(groupIDBytes, android.util.Base64.NO_WRAP))
            put("epoch", epoch)
            put("senderBlsPubkey", android.util.Base64.encodeToString(myBls, android.util.Base64.NO_WRAP))
        }.toString()
        transport.sendProtocolMessage(candidate, ackJson)
    }

    /** Handle a resend request: look up stored envelope, verify requester is authorized, and re-send. */
    private fun handleRekeyResendRequest(groupID: String, epoch: Long, requesterBundle: com.stellarmls.mls.SEPMemberTransportBundle) {
        if (!keyManager.verifyTransportBundle(requesterBundle)) return
        viewModelScope.launch {
            try {
                val records = store.loadPendingRekeys(groupID)
                val record = records.firstOrNull { it.epoch == epoch.toInt() } ?: return@launch

                // Verify requester is an authorized surviving member for this epoch
                val requesterHex = requesterBundle.blsPubkey.toHex()
                val unackedKeys = try {
                    val arr = org.json.JSONArray(record.unackedMemberKeysJSON)
                    (0 until arr.length()).map { arr.getString(it) }
                } catch (_: Exception) { emptyList() }
                // Must also check against post-removal membership (in case requester was already acked)
                val group = groups.find { it.id == groupID }
                val isCurrentMember = group?.members?.any { it.publicKeyCompressed.toHex() == requesterHex } ?: false
                if (!unackedKeys.contains(requesterHex) && !isCurrentMember) {
                    if (BuildConfig.DEBUG) Log.w("GroupListVM", "Resend denied: requester ${requesterHex.take(8)} not authorized for epoch=$epoch")
                    SecurityLog.nonMemberMessageRejected(groupID)
                    return@launch
                }

                val envelopeJson = String(StorageEncryption.decrypt(record.encryptedEnvelope))

                val sealed = GroupCrypto.encryptInvitation(
                    envelopeJson.toByteArray(), requesterBundle.x25519InboxPubkey, keyManager)
                val contentBase64 = android.util.Base64.encodeToString(sealed.toByteArray(), android.util.Base64.NO_WRAP)
                val recipientInboxTag = GroupCrypto.hiddenInboxTag(requesterBundle.x25519InboxPubkey)
                val tags = InvitationTransport.eventTags(recipientInboxTag)
                val event = chat.onym.android.crypto.NostrEventBuilder.build(
                    34113, tags, contentBase64, keyManager,
                    ephemeralSigner = RustBackedNostrSigner.ephemeral()
                )
                publishEvent(event, relayURLs.toList())
                if (BuildConfig.DEBUG) Log.d("GroupListVM", "Re-sent rekey envelope epoch=$epoch to requester")
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) Log.e("GroupListVM", "Resend request failed: ${e.message}")
            }
        }
    }

    /** Periodically resend rekey envelopes to unacked members. Max 3 retries, 24h expiry. */
    private fun startRekeyResendTimer() {
        viewModelScope.launch {
            while (true) {
                kotlinx.coroutines.delay(30_000) // 30 seconds
                resendPendingRekeys()
            }
        }
    }

    private suspend fun resendPendingRekeys() {
        val allPending = try { store.loadAllPendingRekeys() } catch (_: Exception) { return }
        val now = System.currentTimeMillis()
        val maxRetries = 3
        val expiryMs = 24 * 60 * 60 * 1000L // 24 hours

        for (record in allPending) {
            // Expire old pending rekeys
            if (now - record.createdAt > expiryMs) {
                try { store.deletePendingRekey(record.groupID, record.epoch) } catch (_: Exception) { }
                continue
            }
            if (record.retryCount >= maxRetries) continue

            val envelopeJson = try {
                String(StorageEncryption.decrypt(record.encryptedEnvelope))
            } catch (_: Exception) { continue }

            val unackedKeys = try {
                val arr = org.json.JSONArray(record.unackedMemberKeysJSON)
                (0 until arr.length()).map { arr.getString(it) }
            } catch (_: Exception) { emptyList() }

            if (unackedKeys.isEmpty()) {
                try { store.deletePendingRekey(record.groupID, record.epoch) } catch (_: Exception) { }
                continue
            }

            val groupBundles = transportBundles[record.groupID] ?: continue

            for (hex in unackedKeys) {
                val bundle = groupBundles[hex] ?: continue
                try {
                    val sealed = GroupCrypto.encryptInvitation(
                        envelopeJson.toByteArray(), bundle.x25519InboxPubkey, keyManager)
                    val contentBase64 = android.util.Base64.encodeToString(sealed.toByteArray(), android.util.Base64.NO_WRAP)
                    val recipientInboxTag = GroupCrypto.hiddenInboxTag(bundle.x25519InboxPubkey)
                    val tags = InvitationTransport.eventTags(recipientInboxTag)
                    val event = chat.onym.android.crypto.NostrEventBuilder.build(
                        34113, tags, contentBase64, keyManager,
                        ephemeralSigner = RustBackedNostrSigner.ephemeral()
                    )
                    publishEvent(event, relayURLs.toList())
                } catch (e: Exception) {
                    if (BuildConfig.DEBUG) Log.e("GroupListVM", "Resend failed for ${hex.take(8)}: ${e.message}")
                }
            }
            try { store.incrementPendingRekeyRetry(record.groupID, record.epoch) } catch (_: Exception) { }
        }
    }

    /** Phase 6.2: Check for groups with stale rekey state. Called on app foreground. */
    fun rekeyHealthCheck() {
        for (group in groups) {
            val bundleCount = transportBundles[group.id]?.size ?: 0
            val memberCount = group.members.size
            if (bundleCount < memberCount) {
                if (BuildConfig.DEBUG) Log.w("RekeyHealth", "WARN groupID=${group.id.take(8)} bundles=$bundleCount/$memberCount — missing transport bundles")
            }
        }
        viewModelScope.launch {
            val allPending = try { store.loadAllPendingRekeys() } catch (_: Exception) { return@launch }
            for (rekey in allPending) {
                val ageMs = System.currentTimeMillis() - rekey.createdAt
                if (ageMs > 300_000) {
                    if (BuildConfig.DEBUG) Log.w("RekeyHealth", "WARN groupID=${rekey.groupID.take(8)} epoch=${rekey.epoch} pending for ${ageMs/1000}s — triggering resend")
                    resendPendingRekeys()
                    break
                }
            }
        }
    }

    /** Phase 6.3: Dump transport and group diagnostics for debugging. */
    fun dumpDiagnostics() {
        if (!BuildConfig.DEBUG) return
        Log.d("Diagnostics", "=== StellarChat Diagnostics ===")
        Log.d("Diagnostics", "Groups: ${groups.size}")
        for (group in groups) {
            val bundleCount = transportBundles[group.id]?.size ?: 0
            Log.d("Diagnostics", "  group=${group.id.take(8)} epoch=${group.epoch} members=${group.members.size} bundles=$bundleCount topic=${group.topicTag.take(8)}")
        }
        viewModelScope.launch {
            val allPending = try { store.loadAllPendingRekeys() } catch (_: Exception) { emptyList() }
            Log.d("Diagnostics", "Pending rekeys: ${allPending.size}")
            for (rekey in allPending) {
                Log.d("Diagnostics", "  group=${rekey.groupID.take(8)} epoch=${rekey.epoch} retries=${rekey.retryCount} age=${(System.currentTimeMillis() - rekey.createdAt)/1000}s")
            }
            Log.d("Diagnostics", "Chat transport: connected=${transport.isAnyRelayConnected} relays=${transport.connections.size}")
            Log.d("Diagnostics", "=== End Diagnostics ===")
        }
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

    // -- Default group tier --

    fun saveDefaultGroupTier(tier: SEPTier) {
        defaultGroupTier = tier
        contractPrefs.edit().putInt("default_tier", tier.id).apply()
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

    // -- TURN server configuration --

    fun syncTurnConfig() {
        callManager.turnEnabled = turnEnabled
        callManager.turnURLs = turnURLs.toList()
        callManager.turnUsername = turnUsername
        callManager.turnPassword = turnPassword
    }

    fun addTurnURL(urlString: String): Boolean {
        val trimmed = urlString.trim()
        if (!trimmed.startsWith("turn:") && !trimmed.startsWith("turns:")) return false
        if (turnURLs.contains(trimmed)) return false
        turnURLs.add(trimmed)
        persistTurnURLs()
        syncTurnConfig()
        return true
    }

    fun removeTurnURL(index: Int) {
        if (index in turnURLs.indices) {
            turnURLs.removeAt(index)
            persistTurnURLs()
            syncTurnConfig()
        }
    }

    fun updateTurnEnabled(enabled: Boolean) {
        turnEnabled = enabled
        turnPrefs.edit().putBoolean("turn_enabled", enabled).apply()
        syncTurnConfig()
    }

    fun saveTurnCredentials(username: String, password: String) {
        turnUsername = username.trim()
        turnPassword = password.trim()
        keyManager.saveTurnUsername(turnUsername)
        keyManager.saveTurnPassword(turnPassword)
        syncTurnConfig()
    }

    private fun persistTurnURLs() {
        turnPrefs.edit().putString("turn_urls", turnURLs.joinToString(",")).apply()
    }

    companion object {
        /** Known-good Soroban RPC endpoints (M-13). */
        val KNOWN_RPC_ENDPOINTS = listOf(
            "https://soroban-testnet.stellar.org",
            "https://soroban.stellar.org",
            "https://rpc-futurenet.stellar.org",
        )

        /** Chat prefixes that render as system cards and may mutate group state. Keep in
         *  sync with `processGovernanceEvent` and the matching iOS list. */
        private val GOVERNANCE_PREFIXES = listOf(
            "VOTE_PROPOSAL::", "VOTE_CAST::", "ADMIN_PROMOTE::",
            "ADMIN_DEMOTE::", "DEMOCRACY_FINALIZED::", "PEER_LEFT::"
        )

        fun isGovernancePayload(text: String): Boolean =
            GOVERNANCE_PREFIXES.any { text.startsWith(it) }

        /** Default ballot lifetime: 7 days. Ballots past this can no longer
         *  accept new casts or be finalized. On-chain enforcement replaces this
         *  once the Democracy circuit VK ships. */
        const val BALLOT_LIFETIME_SECONDS: Long = 7L * 24 * 3600

        /** Check if a Soroban RPC endpoint URL is well-formed and uses HTTPS. */
        fun isValidRPCEndpoint(url: String): Boolean {
            return try {
                val parsed = java.net.URL(url)
                parsed.protocol == "https" && parsed.host.isNotBlank()
            } catch (_: Exception) { false }
        }

        private const val SALT_HISTORY_WINDOW = 64
        /** Maximum number of epoch snapshots retained per group. */
        private const val EPOCH_SNAPSHOT_WINDOW = 64
        /** N-8: Max entries for dedup sets to prevent unbounded memory growth. */
        private const val MAX_DEDUP_SET_SIZE = 10_000

        /**
         * Canonical byte-wise lexicographic ordering on compressed BLS public
         * keys. Must match `ChatGroup.addMember` and every join/admin path
         * that reconstructs the Merkle commitment locally — any divergence
         * yields a different Poseidon root.
         */
        val memberLeafComparator: Comparator<com.stellarmls.mls.SEPGroupMemberLeaf> =
            Comparator { a, b ->
                val aKey = a.publicKeyCompressed
                val bKey = b.publicKeyCompressed
                for (i in 0 until minOf(aKey.size, bKey.size)) {
                    val cmp = (aKey[i].toInt() and 0xFF) - (bKey[i].toInt() and 0xFF)
                    if (cmp != 0) return@Comparator cmp
                }
                aKey.size - bKey.size
            }
    }

    /** Reconfigure the on-chain service when contract settings change. */
    fun configureContract() {
        if (isContractConfigured && isValidRPCEndpoint(contractEndpoint)) {
            onChainService = if (isRelayerConfigured) {
                OnChainService(
                    getApplication(),
                    contractID,
                    relayerURL,
                    relayerAuthToken.ifBlank { null }
                )
            } else {
                OnChainService(getApplication(), contractID, contractEndpoint)
            }
        } else {
            onChainService = null
        }
    }

    /**
     * Publish a group on-chain (runs on IO dispatcher).
     *
     * 1v1 creation is deferred until the second member joins: the contract
     * hard-rejects `create_group_v2(group_type=1)` unless `member_count == 2`.
     * The creator publishes only once the peer has accepted, still at local
     * epoch 0 with the full two-member commitment.
     */
    fun publishGroupOnChain(group: ChatGroup, onResult: (Result<Unit>) -> Unit) {
        val service = onChainService
        if (service == null) {
            onResult(Result.failure(ChatError.ContractNotConfigured))
            return
        }
        if (group.groupType == com.stellarmls.mls.SEPGroupType.ONE_ON_ONE && group.members.size < 2) {
            onResult(Result.success(Unit))
            return
        }
        viewModelScope.launch {
            try {
                val response = withContext(Dispatchers.IO) {
                    val adminLeaves: List<com.stellarmls.mls.SEPGroupMemberLeaf>?
                    val adminSalt: ByteArray?
                    if (group.groupType == com.stellarmls.mls.SEPGroupType.OLIGARCHY) {
                        // Bootstrap: creator is the sole admin at creation.
                        // Future admin rotations are ceremony-gated (§6.4.3)
                        // and will rebind admin_commitment through
                        // `update_admin_commitment`. The admin salt is
                        // deterministic per groupID so any client can
                        // recompute it without extra persisted state.
                        adminLeaves = listOf(keyManager.memberLeaf())
                        adminSalt = deriveAdminSalt(group.groupIDData)
                    } else {
                        adminLeaves = null
                        adminSalt = null
                    }
                    service.publishGroupCreation(
                        groupIDData = group.groupIDData,
                        members = group.members,
                        blsSecretKey = keyManager.blsSecretKey,
                        epoch = group.epoch,
                        salt = group.salt,
                        tier = group.tier,
                        callerAddress = keyManager.stellarAccountID,
                        groupType = group.groupType,
                        memberCount = group.members.size,
                        adminLeaves = adminLeaves,
                        adminSalt = adminSalt
                    )
                }
                if (response.accepted) {
                    group.isPublishedOnChain = true
                    updateGroup(group)
                    // Store on-chain baseline for future epoch transitions
                    chainBaseline[group.id] = ChainBaseline(group.members.toList(), group.epoch, group.salt.copyOf())
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

    /**
     * 32-byte admin salt derived from the group ID. See §6.2.2: the
     * admin_commitment = Poseidon(Poseidon(admin_root, 0), admin_salt).
     * Matches the iOS [AppState.deriveAdminSalt] so cross-platform clients
     * agree on the seed.
     */
    private fun deriveAdminSalt(groupID: ByteArray): ByteArray {
        val digest = java.security.MessageDigest.getInstance("SHA-256")
        digest.update("sep-admin-salt-v1".toByteArray(Charsets.UTF_8))
        digest.update(groupID)
        return digest.digest()
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

    // -- Epoch Branching --

    /** Capture a snapshot of the group's current state before an epoch transition. */
    fun captureEpochSnapshot(group: ChatGroup, changeDescription: String) {
        val snapshot = EpochSnapshot(
            epoch = group.epoch,
            members = group.members.toList(),
            salt = group.salt.copyOf(),
            groupSecret = group.groupSecret.copyOf(),
            changeDescription = changeDescription
        )
        val groupSnapshots = epochSnapshots.getOrPut(group.id) { java.util.concurrent.ConcurrentHashMap() }
        groupSnapshots[snapshot.epoch] = snapshot

        // Trim in-memory to EPOCH_SNAPSHOT_WINDOW
        if (groupSnapshots.size > EPOCH_SNAPSHOT_WINDOW) {
            val oldest = groupSnapshots.keys.sorted().take(groupSnapshots.size - EPOCH_SNAPSHOT_WINDOW)
            for (key in oldest) { groupSnapshots.remove(key) }
        }

        // Persist
        viewModelScope.launch {
            try {
                store.saveEpochSnapshot(snapshot, group.id)
                store.trimEpochSnapshots(group.id, EPOCH_SNAPSHOT_WINDOW)
            } catch (_: Exception) { }
        }
    }

    /** Resolve encryption key for a historical epoch from snapshots. */
    fun epochKey(groupID: String, epoch: Long): ByteArray? {
        return epochSnapshots[groupID]?.get(epoch)?.encryptionKey()
    }

    /** Pin the user to a historical epoch. */
    fun pinEpoch(groupID: String, epoch: Long) {
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) return
        val group = groups[index]
        group.pinnedEpoch = epoch
        groups[index] = group
        updateCurrentMembers(groupID)

        // If pinned epoch has a different groupSecret (secure rekey boundary),
        // subscribe to the old topic so messages are visible.
        val snapshot = epochSnapshots[groupID]?.get(epoch)
        if (snapshot != null) {
            val pinnedTopic = snapshot.topicTag
            val currentTopic = group.topicTag
            if (pinnedTopic != currentTopic) {
                val pinnedGroup = group.copy(
                    groupSecret = snapshot.groupSecret,
                    salt = snapshot.salt,
                    epoch = snapshot.epoch
                )
                transport.subscribe(
                    group = pinnedGroup,
                    keyResolver = { e -> epochKey(groupID, e) },
                    sinceTimestamp = if (group.lastEventTimestamp > 0) group.lastEventTimestamp else null
                )
            }
        }

        viewModelScope.launch {
            try { store.saveGroup(group) } catch (_: Exception) { }
        }
    }

    /** Unpin the user from a historical epoch, returning to the latest. */
    fun unpinEpoch(groupID: String) {
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) return
        val group = groups[index]
        val oldPinned = group.pinnedEpoch
        group.pinnedEpoch = null
        groups[index] = group
        updateCurrentMembers(groupID)

        // Unsubscribe from old topic if it was across a rekey boundary
        if (oldPinned != null) {
            val snapshot = epochSnapshots[groupID]?.get(oldPinned)
            if (snapshot != null && snapshot.topicTag != group.topicTag) {
                transport.unsubscribe(snapshot.topicTag)
            }
        }

        viewModelScope.launch {
            try { store.saveGroup(group) } catch (_: Exception) { }
        }
    }

    /** The effective encryption key, considering pinned epoch. */
    fun effectiveEncryptionKey(group: ChatGroup): ByteArray {
        val pinned = group.pinnedEpoch ?: return group.encryptionKey
        return epochKey(group.id, pinned) ?: group.encryptionKey
    }

    /** The effective topic tag, considering pinned epoch. */
    fun effectiveTopicTag(group: ChatGroup): String {
        val pinned = group.pinnedEpoch ?: return group.topicTag
        val snapshot = epochSnapshots[group.id]?.get(pinned)
        return snapshot?.topicTag ?: group.topicTag
    }

    /** The effective epoch value for tagging outgoing messages. */
    fun effectiveEpoch(group: ChatGroup): Long {
        return group.pinnedEpoch ?: group.epoch
    }

    /** Update transport currentMembers to include pinned epoch members in the union. */
    private fun updateCurrentMembers(groupID: String) {
        transport.currentMembers.clear()
        val allMembers = mutableListOf<SEPGroupMemberLeaf>()
        for (g in groups) {
            allMembers.addAll(g.members)
            // Include members from pinned epoch snapshot
            val pinned = g.pinnedEpoch
            if (pinned != null) {
                val snapshot = epochSnapshots[g.id]?.get(pinned)
                if (snapshot != null) {
                    for (m in snapshot.members) {
                        if (allMembers.none { it.publicKeyCompressed.contentEquals(m.publicKeyCompressed) }) {
                            allMembers.add(m)
                        }
                    }
                }
            }
        }
        transport.currentMembers.addAll(allMembers)
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

        val myBundle = try { keyManager.createTransportBundle() } catch (_: Exception) { null }

        // Include all known transport bundles so every member can collect bundles
        // from members who joined before them — required for secure rekey.
        val allBundles = transportBundles[group.id]?.values?.toList() ?: emptyList()

        return SEPGroupStateUpdate(
            epoch = group.epoch,
            salt = group.salt,
            addedMembers = addedMembers,
            removedMemberKeys = removedMemberKeys,
            commitment = group.commitment,
            senderAttestation = attestation,
            senderTransportBundle = myBundle,
            memberBundles = allBundles
        )
    }

    /** Broadcast a state update to all members via the encrypted group channel. */
    fun broadcastStateUpdate(group: ChatGroup, update: SEPGroupStateUpdate, overrideKey: ByteArray? = null) {
        val json = stateUpdateToJson(update)
        transport.sendProtocolMessage(group, json, overrideKey)
    }

    // -- Blockchain-First Epoch Transition Helper --

    /**
     * The single entry point for all epoch-advancing operations on published groups.
     * Computes candidate next state, submits to chain, confirms, then persists locally
     * and broadcasts. For unpublished groups, applies the transition locally.
     */
    private suspend fun performEpochTransition(
        groupID: String,
        kind: EpochTransitionKind,
        previousKey: ByteArray
    ): EpochTransitionResult {
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) return EpochTransitionResult.CHAIN_REJECTED

        val currentGroup = cloneGroup(groups[index])

        // Note: we no longer block transitions when a chain publish is pending.
        // Chain publishes are awaited synchronously before mutating local state
        // (see chain-first block below), so concurrent transitions queue naturally.

        // Compute candidate next state
        var candidate = cloneGroup(currentGroup)
        val addedMembers = mutableListOf<SEPGroupMemberLeaf>()
        val removedMemberKeys = mutableListOf<ByteArray>()
        var isOneOnOneFinalize = false

        when (kind) {
            is EpochTransitionKind.MemberAdd -> {
                val member = kind.member
                if (candidate.members.any { it.publicKeyCompressed.contentEquals(member.publicKeyCompressed) }) {
                    // Already a member — ensure system message exists (dedup-safe)
                    val name = memberDisplayName(member.publicKeyCompressed)
                    insertSystemMessage(groupID, "$name joined the group", "member-add", currentGroup.epoch)
                    return EpochTransitionResult.SUCCESS
                }
                if (candidate.members.size >= candidate.tier.maxMembers) {
                    return EpochTransitionResult.CHAIN_REJECTED
                }
                // 1v1 finalization: contract requires epoch=0 + member_count=2 at create.
                // Creator publishes only once the peer joins; the local state stays at
                // epoch 0 with the invite-era salt so both sides compute the same
                // commitment the contract will see.
                isOneOnOneFinalize = currentGroup.groupType == com.stellarmls.mls.SEPGroupType.ONE_ON_ONE &&
                    !currentGroup.isPublishedOnChain &&
                    currentGroup.members.size == 1
                candidate.members.add(member)
                sortMembers(candidate)
                if (!isOneOnOneFinalize) {
                    candidate.epoch++
                    candidate.salt = SEPCommitmentBuilder.deriveSalt(candidate.salt, member.publicKeyCompressed)
                }
                addedMembers.add(member)
            }
            is EpochTransitionKind.MemberRemove -> {
                val pubkey = kind.blsPubkey
                if (candidate.members.none { it.publicKeyCompressed.contentEquals(pubkey) }) {
                    return EpochTransitionResult.CHAIN_REJECTED
                }
                candidate.members.removeAll { it.publicKeyCompressed.contentEquals(pubkey) }
                candidate.epoch++
                candidate.salt = SEPCommitmentBuilder.generateSalt()
                removedMemberKeys.add(pubkey)
            }
            is EpochTransitionKind.KeyRotation -> {
                candidate.epoch++
                candidate.salt = SEPCommitmentBuilder.generateSalt()
            }
        }

        candidate.recomputeCommitment()

        // Determine secure rekey BEFORE chain publish so we publish the final state
        val isRemoval = removedMemberKeys.isNotEmpty()
        val useSecureRekey = isRemoval && allMembersHaveBundles(groupID, candidate.members)
        if (BuildConfig.DEBUG) {
            val bundleKeys = transportBundles[groupID]?.keys?.map { it.take(8) } ?: emptyList()
            val memberKeys = candidate.members.map { it.publicKeyCompressed.toHex().take(8) }
            Log.d("GroupListVM", "Removal flow: useSecureRekey=$useSecureRekey isRemoval=$isRemoval bundles=$bundleKeys members=$memberKeys")
        }

        // Removals REQUIRE inbox delivery (secure rekey). If bundles are missing,
        // bail before advancing chain state — otherwise chain epoch advances
        // while peers can't follow. Caller must rotate the key first to
        // distribute bundles.
        if (isRemoval && !useSecureRekey) {
            if (BuildConfig.DEBUG) Log.w("GroupListVM", "Removal blocked pre-chain: not all remaining members have transport bundles. Rotate key first.")
            return EpochTransitionResult.BUNDLES_MISSING
        }

        // If secure rekey, apply fresh groupSecret + salt now so the chain publish
        // uses the final commitment (one publish instead of two).
        val preRekeyCandidate = cloneGroup(candidate)
        if (useSecureRekey) {
            val random = SecureRandom()
            val freshSecret = ByteArray(32).also { random.nextBytes(it) }
            candidate = candidate.copy(groupSecret = freshSecret)
            candidate.salt = SEPCommitmentBuilder.generateSalt()
            candidate.recomputeCommitment()
        }

        // --- Chain-first: confirm on-chain BEFORE mutating local state ---
        // Any group whose state is tracked on-chain MUST have its next-epoch
        // commitment accepted by the contract before we persist, subscribe,
        // or broadcast. If the contract rejects (e.g. Oligarchy non-admin
        // removal, proof mismatch, stale baseline), we return without
        // touching local state so sender and peers stay consistent with
        // chain authority.
        if (BuildConfig.DEBUG) Log.d("GroupListVM", "Chain-first check: isPublishedOnChain=${currentGroup.isPublishedOnChain} isOneOnOneFinalize=$isOneOnOneFinalize onChainService=${onChainService != null} group=${groupID.take(8)} newEpoch=${candidate.epoch}")
        if (isOneOnOneFinalize) {
            if (onChainService == null) return EpochTransitionResult.CHAIN_UNAVAILABLE
            chainPublishJobs[groupID]?.cancel()
            chainPublishJobs.remove(groupID)
            val deferred = kotlinx.coroutines.CompletableDeferred<Result<Unit>>()
            publishGroupOnChain(cloneGroup(candidate)) { deferred.complete(it) }
            val result = deferred.await()
            if (result.isFailure) {
                if (BuildConfig.DEBUG) Log.e("GroupListVM", "1v1 finalize chain publish FAILED group=${groupID.take(8)} error=${result.exceptionOrNull()?.message}")
                return EpochTransitionResult.CHAIN_REJECTED
            }
            // publishGroupOnChain flips isPublishedOnChain=true on the stored
            // group via updateGroup(...). Re-read the live group so subsequent
            // local mutations build on that flag.
            groups.find { it.id == groupID }?.let {
                candidate.isPublishedOnChain = it.isPublishedOnChain
            }
        } else if (currentGroup.isPublishedOnChain) {
            if (onChainService == null) return EpochTransitionResult.CHAIN_UNAVAILABLE
            chainPublishJobs[groupID]?.cancel()
            chainPublishJobs.remove(groupID)
            val targetEpoch = candidate.epoch
            pendingTransitions[groupID] = PendingTransitionState.AwaitingChainConfirmation(targetEpoch)
            val baseline = chainBaseline[groupID]
            val baselineGroup = if (baseline != null) {
                val g = cloneGroup(currentGroup)
                g.members.clear()
                g.members.addAll(baseline.members)
                g.epoch = baseline.epoch
                g.salt = baseline.salt
                g
            } else {
                cloneGroup(currentGroup)
            }
            if (BuildConfig.DEBUG) Log.d("GroupListVM", "Chain publish STARTING: baselineEpoch=${baselineGroup.epoch} targetEpoch=$targetEpoch baselineMembers=${baselineGroup.members.size} candidateMembers=${candidate.members.size} hasStoredBaseline=${baseline != null}")
            val result = try {
                publishMembershipUpdateIfNeeded(baselineGroup, candidate)
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                Result.failure(e)
            }
            pendingTransitions[groupID] = PendingTransitionState.Idle
            if (result.isFailure) {
                if (BuildConfig.DEBUG) Log.e("GroupListVM", "Chain publish REJECTED epoch=${candidate.epoch} baselineEpoch=${baselineGroup.epoch} group=${groupID.take(8)} error=${result.exceptionOrNull()?.message}")
                return EpochTransitionResult.CHAIN_REJECTED
            }
            chainBaseline[groupID] = ChainBaseline(
                candidate.members.toList(), candidate.epoch, candidate.salt.copyOf()
            )
            candidate.isPublishedOnChain = currentGroup.isPublishedOnChain
            if (BuildConfig.DEBUG) Log.d("GroupListVM", "Chain publish SUCCEEDED epoch=${candidate.epoch} group=${groupID.take(8)}")
        } else {
            // Unpublished draft group (e.g. 1v1 creator waiting for peer).
            // No chain authority yet — safe to mutate locally.
            candidate.isPublishedOnChain = currentGroup.isPublishedOnChain
        }

        // Chain confirmed (or not required). Capture snapshot of the pre-transition
        // state for history/fork resolution, then persist the candidate locally.
        val changeDesc = when (kind) {
            is EpochTransitionKind.MemberAdd -> "Member added"
            is EpochTransitionKind.MemberRemove -> "Member removed"
            is EpochTransitionKind.KeyRotation -> "Key rotated"
        }
        captureEpochSnapshot(currentGroup, changeDesc)

        // Re-locate the group by ID — `index` captured above may be stale after
        // the suspend points on chain publish (groups list can be mutated during
        // the await). If the group disappeared entirely, abort without persisting.
        val liveIndex = groups.indexOfFirst { it.id == groupID }
        if (liveIndex < 0) {
            if (BuildConfig.DEBUG) Log.w("GroupListVM", "Group $groupID disappeared during chain publish; dropping transition")
            return EpochTransitionResult.CHAIN_REJECTED
        }
        groups[liveIndex] = candidate
        try { store.saveGroup(candidate) } catch (_: Exception) { }
        storeSalt(groupID, candidate.epoch, candidate.salt)

        // Sync transport and ensure subscription uses the latest key.
        // This must happen BEFORE the secure rekey / legacy blocks so A can
        // at least communicate on the current state even if the blocks fail.
        updateCurrentMembers(groupID)
        if (useSecureRekey) {
            // Secure rekey: unsubscribe old topic now, subscribe new topic
            if (!BuildConfig.DEBUG) transport.unsubscribe(currentGroup.topicTag)
            syncTransportAndSubscribe(candidate)
        } else {
            syncTransportAndSubscribe(candidate)
        }

        if (useSecureRekey) {
            if (BuildConfig.DEBUG) Log.d("Rekey", "START groupID=${groupID.take(8)} epoch=${candidate.epoch} members=${candidate.members.size} secure=true")
            var sentCount = 0

            // 1. Broadcast SEPRemovalNotice on old channel (no secrets)
            val noticeJson = JSONObject().apply {
                put("type", com.stellarmls.mls.SEPRemovalNotice.MESSAGE_TYPE)
                put("groupID", android.util.Base64.encodeToString(currentGroup.groupIDData, android.util.Base64.NO_WRAP))
                put("epoch", candidate.epoch)
                val removedArr = JSONArray()
                for (k in removedMemberKeys) removedArr.put(android.util.Base64.encodeToString(k, android.util.Base64.NO_WRAP))
                put("removedMemberKeys", removedArr)
                candidate.commitment?.let {
                    put("commitment", android.util.Base64.encodeToString(it, android.util.Base64.NO_WRAP))
                }
            }.toString()
            // Fire-and-forget: removal notice is non-critical courtesy — must NOT block rekey delivery
            viewModelScope.launch {
                try {
                    transport.sendProtocolMessage(currentGroup, noticeJson, previousKey)
                    if (BuildConfig.DEBUG) Log.d("Rekey", "NOTICE_SENT groupID=${groupID.take(8)} epoch=${candidate.epoch} (fire-and-forget)")
                } catch (e: Exception) {
                    if (BuildConfig.DEBUG) Log.w("Rekey", "Removal notice failed (non-critical): ${e.message}")
                }
            }

            // 2. Build rekey envelope JSON
            val myBundle = keyManager.createTransportBundle()
            val memberBundlesArr = JSONArray()
            for (member in candidate.members) {
                val hex = member.publicKeyCompressed.toHex()
                transportBundles[groupID]?.get(hex)?.let {
                    memberBundlesArr.put(BootstrapPayload.serializeTransportBundle(it))
                }
            }
            val removedKeysArr = JSONArray()
            for (k in removedMemberKeys) removedKeysArr.put(android.util.Base64.encodeToString(k, android.util.Base64.NO_WRAP))
            val relayHintsArr = JSONArray(candidate.relayHints)
            val envelopeJson = JSONObject().apply {
                put("type", com.stellarmls.mls.SEPRekeyEnvelope.MESSAGE_TYPE)
                put("groupID", android.util.Base64.encodeToString(currentGroup.groupIDData, android.util.Base64.NO_WRAP))
                put("epoch", candidate.epoch)
                put("groupSecret", android.util.Base64.encodeToString(candidate.groupSecret, android.util.Base64.NO_WRAP))
                put("salt", android.util.Base64.encodeToString(candidate.salt, android.util.Base64.NO_WRAP))
                candidate.commitment?.let { put("commitment", android.util.Base64.encodeToString(it, android.util.Base64.NO_WRAP)) }
                put("memberBundles", memberBundlesArr)
                put("removedMemberKeys", removedKeysArr)
                put("relayHints", relayHintsArr)
                put("senderBundle", BootstrapPayload.serializeTransportBundle(myBundle))
            }.toString()

            // 3. Send per-member rekey envelopes via inbox
            val myLeaf = keyManager.memberLeaf()
            for (member in candidate.members) {
                if (member.publicKeyCompressed.contentEquals(myLeaf.publicKeyCompressed)) continue
                val hex = member.publicKeyCompressed.toHex()
                val bundle = transportBundles[groupID]?.get(hex) ?: continue
                try {
                    val sealed = GroupCrypto.encryptInvitation(
                        envelopeJson.toByteArray(),
                        bundle.x25519InboxPubkey,
                        keyManager
                    )
                    val contentBase64 = android.util.Base64.encodeToString(sealed.toByteArray(), android.util.Base64.NO_WRAP)
                    val recipientInboxTag = GroupCrypto.hiddenInboxTag(bundle.x25519InboxPubkey)
                    val tags = InvitationTransport.eventTags(recipientInboxTag)
                    val event = chat.onym.android.crypto.NostrEventBuilder.build(
                        34113, tags, contentBase64, keyManager,
                        ephemeralSigner = RustBackedNostrSigner.ephemeral()
                    )
                    publishEvent(event, relayURLs.toList())
                    sentCount++
                    if (BuildConfig.DEBUG) Log.d("Rekey", "ENVELOPE_SENT groupID=${groupID.take(8)} epoch=${candidate.epoch} recipient=${hex.take(8)} eventID=${event.id.take(12)}")
                } catch (e: Exception) {
                    if (BuildConfig.DEBUG) Log.e("Rekey", "ENVELOPE_FAILED groupID=${groupID.take(8)} epoch=${candidate.epoch} recipient=${hex.take(8)} error=${e.message}")
                }
            }
            if (BuildConfig.DEBUG) Log.d("Rekey", "COMPLETE groupID=${groupID.take(8)} epoch=${candidate.epoch} sent=$sentCount/${candidate.members.size}")

            // Group state was persisted synchronously above (chain-first block).
            // Persist the pending rekey so retry can re-deliver envelopes if a
            // recipient is offline.
            val unackedKeys = candidate.members
                .filter { !it.publicKeyCompressed.contentEquals(myLeaf.publicKeyCompressed) }
                .map { it.publicKeyCompressed.toHex() }
            try {
                store.savePendingRekey(groupID, candidate.epoch.toInt(), envelopeJson, unackedKeys, isRemovalEpoch = true)
            } catch (_: Exception) { }

        } else {
            // --- LEGACY FLOW: non-removal operations only (add member, key rotation) ---
            val update = buildStateUpdate(candidate, addedMembers, removedMemberKeys)
            broadcastStateUpdate(candidate, update, overrideKey = previousKey)
        }

        // Insert system message for the transition
        when (kind) {
            is EpochTransitionKind.MemberAdd -> {
                val name = memberDisplayName(kind.member.publicKeyCompressed)
                insertSystemMessage(groupID, "$name joined the group", "member-add", candidate.epoch)
            }
            is EpochTransitionKind.MemberRemove -> {
                val name = memberDisplayName(kind.blsPubkey)
                insertSystemMessage(groupID, "$name was removed from the group", "member-remove", candidate.epoch)
            }
            is EpochTransitionKind.KeyRotation -> {
                insertSystemMessage(groupID, "Encryption key was rotated", "key-rotation", candidate.epoch)
            }
        }

        return EpochTransitionResult.SUCCESS
    }

    /** Check if a group has a pending epoch transition (for UI locking). */
    fun pendingTransitionState(groupID: String): PendingTransitionState {
        return pendingTransitions[groupID] ?: PendingTransitionState.Idle
    }

    // -- System Messages --

    /** Display name for a BLS member key: "You" for self, alias if set, or truncated hex. */
    private fun memberDisplayName(blsPubkey: ByteArray): String {
        val myLeaf = keyManager.memberLeaf()
        if (myLeaf.publicKeyCompressed.contentEquals(blsPubkey)) return "You"
        val hex = blsPubkey.toHex()
        return contactAliasStore.displayName(hex) ?: (hex.take(8) + "...")
    }

    /** Broadcast a Democracy ballot to remove a member. The proposal is sent via the
     *  chat transport with a hard expiry timestamp so every current member sees the
     *  ballot's deadline. On-chain proof enforcement arrives with the Democracy
     *  circuit ceremony; until then the client aggregates `VOTE_CAST` responses
     *  locally, ignores casts past expiry, and any member can finalize once quorum
     *  is met before expiry.
     *
     *  Wire format: `VOTE_PROPOSAL::v1::remove::<targetHex>::<ballotID>::<expiryUnixSeconds>` */
    fun proposeRemovalVote(groupID: String, targetPubkeyHex: String) {
        val group = groups.firstOrNull { it.id == groupID } ?: return
        if (group.groupType != com.stellarmls.mls.SEPGroupType.DEMOCRACY) return
        val ballotID = java.util.UUID.randomUUID().toString().take(8)
        val expiry = (System.currentTimeMillis() / 1000L) + BALLOT_LIFETIME_SECONDS
        val text = "VOTE_PROPOSAL::v1::remove::$targetPubkeyHex::$ballotID::$expiry"
        sendMessage(groupID, text)
    }

    /**
     * Cast a Yes/No vote on an open ballot.
     *
     * v2 wire format (docs/democracy-circuit-ceremony.md §5):
     *   `VOTE_CAST::v2::<ballotID>::<yes|no>::<sigHex>`
     *
     * The signature is a 64-byte Ed25519 signature from the sender's
     * Stellar key over the prefix `VOTE_CAST::v2::<ballotID>::<yes|no>`
     * (UTF-8 bytes). Stellar is used rather than a raw BLS signature
     * because the app already has an attested BLS ↔ Stellar binding
     * via `KeyAttestation`, and no BLS12-381 sign primitive has been
     * exposed through the JNI yet. A future protocol bump will migrate
     * to a raw BLS signature.
     *
     * The Democracy circuit witness path (task #26) does NOT consume
     * this signature — it takes the voter's BLS secret key as a private
     * witness. The signature exists purely for off-chain audit so a
     * third party can later confirm the vote came from the claimed
     * member. Latest cast per sender wins during tallying.
     */
    fun castVote(groupID: String, ballotID: String, yes: Boolean) {
        val choice = if (yes) "yes" else "no"
        val prefix = "VOTE_CAST::v2::$ballotID::$choice"
        val sigHex: String? = try {
            keyManager.stellarSign(prefix.toByteArray(Charsets.UTF_8))
                .joinToString("") { "%02x".format(it) }
        } catch (_: Throwable) {
            null
        }
        val text = if (sigHex != null) {
            "$prefix::$sigHex"
        } else {
            // Fallback: emit v1 if signing fails. Vote still registers
            // for local tally; the audit slot is lost for this cast.
            "VOTE_CAST::v1::$ballotID::$choice"
        }
        sendMessage(groupID, text)
    }

    /**
     * Aggregate Yes/No counts from chatMessages, deduping by senderPubkey (last wins).
     * Casts arriving after `expiryUnixSeconds` (when non-null) are ignored so the
     * tally matches what on-chain enforcement will canonicalize post-circuit deployment.
     *
     * Accepts both v1 (`VOTE_CAST::v1::<bid>::<yes|no>`) and v2
     * (`VOTE_CAST::v2::<bid>::<yes|no>::<sigHex>`) formats. v2's signature
     * is for off-chain audit only; tally does not enforce it today because
     * senderPubkey is already authenticated by the transport envelope.
     */
    fun ballotTally(groupID: String, ballotID: String, expiryUnixSeconds: Long? = null): Pair<Int, Int> {
        val messages = chatMessages[groupID] ?: return 0 to 0
        val prefixV1 = "VOTE_CAST::v1::$ballotID::"
        val prefixV2 = "VOTE_CAST::v2::$ballotID::"
        val latestPerSender = mutableMapOf<String, String>()
        for (m in messages) {
            if (m.senderPubkey.isEmpty()) continue
            if (expiryUnixSeconds != null && m.timestamp.time / 1000L > expiryUnixSeconds) continue
            val choice: String? = when {
                m.text.startsWith(prefixV2) -> {
                    // v2 payload tail: "<yes|no>::<sigHex>" — split once
                    m.text.removePrefix(prefixV2).substringBefore("::")
                }
                m.text.startsWith(prefixV1) -> m.text.removePrefix(prefixV1)
                else -> null
            }
            if (choice == "yes" || choice == "no") {
                latestPerSender[m.senderPubkey] = choice
            }
        }
        var yes = 0
        var no = 0
        for (choice in latestPerSender.values) {
            when (choice) {
                "yes" -> yes++
                "no" -> no++
            }
        }
        return yes to no
    }

    /** Majority quorum for Democracy: `(members / 2) + 1`. */
    fun ballotQuorum(groupID: String): Int {
        val total = groups.firstOrNull { it.id == groupID }?.members?.size ?: 0
        return (total / 2) + 1
    }

    fun ballotFinalized(groupID: String, ballotID: String): Boolean {
        val needle = "DEMOCRACY_FINALIZED::v1::$ballotID"
        return chatMessages[groupID]?.any { it.text == needle } == true
    }

    /** Finalize a ballot that has reached quorum: broadcast the finalize marker and
     *  execute the actual member removal via the existing rekey pipeline. */
    fun finalizeBallot(groupID: String, ballotID: String, targetPubkeyHex: String) {
        sendMessage(groupID, "DEMOCRACY_FINALIZED::v1::$ballotID")
        val target = groups.firstOrNull { it.id == groupID }
            ?.members?.firstOrNull { it.publicKeyCompressed.toHex() == targetPubkeyHex }
            ?: return
        removeMember(target.publicKeyCompressed, groupID)
    }

    /** Oligarchy admin promotion. Caller must already be an admin. Updates local state
     *  immediately and broadcasts `ADMIN_PROMOTE::v1::<hex>` for peer sync. */
    fun promoteToAdmin(groupID: String, targetPubkeyHex: String) {
        val idx = groups.indexOfFirst { it.id == groupID }
        if (idx < 0) return
        val group = groups[idx]
        if (group.groupType != com.stellarmls.mls.SEPGroupType.OLIGARCHY) return
        val myHex = keyManager.blsPublicKey().toHex()
        if (!group.adminPubkeys.contains(myHex)) return
        if (group.adminPubkeys.contains(targetPubkeyHex)) return
        group.adminPubkeys.add(targetPubkeyHex)
        groups[idx] = group
        viewModelScope.launch { try { store.saveGroup(group) } catch (_: Exception) { } }
        sendMessage(groupID, "ADMIN_PROMOTE::v1::$targetPubkeyHex")
    }

    /** Oligarchy admin demotion. The last admin cannot be demoted. */
    fun demoteFromAdmin(groupID: String, targetPubkeyHex: String) {
        val idx = groups.indexOfFirst { it.id == groupID }
        if (idx < 0) return
        val group = groups[idx]
        if (group.groupType != com.stellarmls.mls.SEPGroupType.OLIGARCHY) return
        val myHex = keyManager.blsPublicKey().toHex()
        if (!group.adminPubkeys.contains(myHex)) return
        if (!group.adminPubkeys.contains(targetPubkeyHex)) return
        if (group.adminPubkeys.size <= 1) return
        group.adminPubkeys.remove(targetPubkeyHex)
        groups[idx] = group
        viewModelScope.launch { try { store.saveGroup(group) } catch (_: Exception) { } }
        sendMessage(groupID, "ADMIN_DEMOTE::v1::$targetPubkeyHex")
    }

    /** Mutate group state in response to an inbound governance system message. The chat
     *  transport already verifies BLS signatures before decryption; membership is
     *  checked here so that spoofed admin-state changes are rejected. */
    private fun processGovernanceEvent(groupID: String, text: String, senderBlsHex: String) {
        val idx = groups.indexOfFirst { it.id == groupID }
        if (idx < 0) return
        val group = groups[idx]
        when {
            text.startsWith("ADMIN_PROMOTE::v1::") -> {
                if (group.groupType != com.stellarmls.mls.SEPGroupType.OLIGARCHY) return
                if (!group.adminPubkeys.contains(senderBlsHex)) return
                val targetHex = text.removePrefix("ADMIN_PROMOTE::v1::")
                if (group.adminPubkeys.contains(targetHex)) return
                group.adminPubkeys.add(targetHex)
                groups[idx] = group
                viewModelScope.launch { try { store.saveGroup(group) } catch (_: Exception) { } }
            }
            text.startsWith("ADMIN_DEMOTE::v1::") -> {
                if (group.groupType != com.stellarmls.mls.SEPGroupType.OLIGARCHY) return
                if (!group.adminPubkeys.contains(senderBlsHex)) return
                val targetHex = text.removePrefix("ADMIN_DEMOTE::v1::")
                if (!group.adminPubkeys.contains(targetHex)) return
                if (group.adminPubkeys.size <= 1) return
                group.adminPubkeys.remove(targetHex)
                groups[idx] = group
                viewModelScope.launch { try { store.saveGroup(group) } catch (_: Exception) { } }
            }
        }
    }

    /** Insert a system message into chatMessages (and persist). Uses deterministic IDs for dedup. */
    private fun insertSystemMessage(groupID: String, text: String, event: String, epoch: Long) {
        val id = "sys-$event-${groupID.take(8)}-$epoch"
        if (chatMessages[groupID]?.any { it.id == id } == true) return
        val msg = ChatMessage(
            id = id,
            groupID = groupID,
            senderPubkey = "",
            text = text,
            timestamp = java.util.Date(),
            isMine = false,
            isSystemMessage = true
        )
        chatMessages[groupID] = (chatMessages[groupID] ?: emptyList()) + msg
        bumpGroupLastMessage(groupID, msg.timestamp.time)
        viewModelScope.launch { try { store.saveMessage(msg) } catch (_: Exception) { } }
    }

    /** Set the owning group's `lastMessageAt` to max(existing, timestampMs) and re-sort
     *  the in-memory [groups] list so the chat list reorders live (most recent first).
     *  Persists the bumped group row so the order survives a restart. */
    private fun bumpGroupLastMessage(groupID: String, timestampMs: Long) {
        val idx = groups.indexOfFirst { it.id == groupID }
        if (idx < 0) return
        val g = groups[idx]
        if (g.lastMessageAt >= timestampMs) return
        g.lastMessageAt = timestampMs
        groups[idx] = g
        val currentIDs = groups.map { it.id }
        val sortedIDs = groups.sortedWith(compareByDescending<ChatGroup> { it.isPinned }
            .thenByDescending { if (it.lastMessageAt > 0) it.lastMessageAt else it.createdAt.time }
        ).map { it.id }
        if (sortedIDs != currentIDs) {
            val byID = groups.associateBy { it.id }
            groups.clear()
            sortedIDs.forEach { id -> byID[id]?.let { groups.add(it) } }
        }
        viewModelScope.launch { try { store.saveGroup(g) } catch (_: Exception) { } }
    }

    // -- Transport Bundle Management --

    /** Verify and persist a transport bundle for a group member. */
    private fun processTransportBundle(bundle: com.stellarmls.mls.SEPMemberTransportBundle, groupID: String) {
        if (!bundle.hasValidStructure) return
        if (!keyManager.verifyTransportBundle(bundle)) {
            SecurityLog.invalidAttestation("transport bundle signature verification failed")
            return
        }
        val hex = bundle.blsPubkey.toHex()
        val existing = transportBundles[groupID]?.get(hex)
        if (existing != null && existing.version >= bundle.version) return
        transportBundles.getOrPut(groupID) { mutableMapOf() }[hex] = bundle
        viewModelScope.launch {
            try { store.saveTransportBundle(bundle, groupID) } catch (_: Exception) { }
        }
    }

    /** Create and persist own transport bundle for a group. */
    private fun ensureOwnTransportBundle(groupID: String) {
        val bundle = keyManager.createTransportBundle()
        val hex = bundle.blsPubkey.toHex()
        transportBundles.getOrPut(groupID) { mutableMapOf() }[hex] = bundle
        viewModelScope.launch {
            try { store.saveTransportBundle(bundle, groupID) } catch (_: Exception) { }
        }
    }

    fun lookupContactInboxKeys(): Map<String, String> {
        val ownBlsHex = keyManager.blsPublicKey().toHex()
        val result = mutableMapOf<String, String>()
        for ((_, bundles) in transportBundles) {
            for ((blsHex, bundle) in bundles) {
                if (blsHex == ownBlsHex) continue
                if (result.containsKey(blsHex)) continue
                result[blsHex] = bundle.x25519InboxPubkey.toHex()
            }
        }
        return result
    }

    /** Check if all members have transport bundles for a group. */
    private fun allMembersHaveBundles(groupID: String, members: List<SEPGroupMemberLeaf>): Boolean {
        val bundles = transportBundles[groupID] ?: return false
        return members.all { bundles.containsKey(it.publicKeyCompressed.toHex()) }
    }

    /**
     * Apply a received state update. For published groups, chain-confirm-first:
     * the update is only applied if on-chain epoch/commitment matches.
     * For unpublished groups, applies directly (local-first).
     */
    fun applyStateUpdate(update: SEPGroupStateUpdate, groupID: String) {
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) return
        val group = groups[index]

        // Stale update — ignore
        if (update.epoch < group.epoch) return

        // Capture epoch snapshot BEFORE mutation for epoch branching
        val changeDesc = when {
            update.removedMemberKeys.isNotEmpty() -> "Member removed"
            update.addedMembers.isNotEmpty() -> "Member added"
            else -> "Key rotated"
        }
        captureEpochSnapshot(group, changeDesc)

        // Verify sender attestation BEFORE mutating state
        val senderAtt = update.senderAttestation
        if (senderAtt != null) {
            val att = KeyAttestation(
                blsPubkey = senderAtt.blsPubkey,
                ed25519Pubkey = senderAtt.ed25519Pubkey,
                signature = senderAtt.signature
            )
            if (!KeyAttestation.verify(att)) {
                return
            }
        }

        // Extract and persist sender's transport bundle if present
        update.senderTransportBundle?.let { processTransportBundle(it, groupID) }
        // Process all member bundles — enables secure rekey for members who
        // joined after others and never received their bundles directly.
        for (b in update.memberBundles) { processTransportBundle(b, groupID) }

        if (update.epoch == group.epoch) {
            // Same epoch + same salt = own echo — ignore
            if (update.salt.contentEquals(group.salt)) {
                if (BuildConfig.DEBUG) Log.d("GroupListVM", "Own echo at epoch=${update.epoch} group=${groupID.take(8)}")
                return
            }

            // Epoch fork: for published groups, fetch chain state and adopt winner.
            // For unpublished groups, accept remote state.
            if (group.isPublishedOnChain) {
                val service = onChainService
                if (service != null) {
                    viewModelScope.launch {
                        try {
                            val entry = withContext(Dispatchers.IO) {
                                service.fetchOnChainState(group.groupIDData)
                            }
                            if (BuildConfig.DEBUG) Log.d("GroupListVM", "Fork at epoch=${group.epoch}: chain=${entry.epoch} group=${groupID.take(8)}")
                            val idx = groups.indexOfFirst { it.id == groupID }
                            if (idx < 0) return@launch
                            val g = groups[idx]

                            if (entry.epoch > g.epoch) {
                                // Chain moved ahead — adopt chain epoch with update's member delta
                                applyMemberDelta(update, g)
                                g.epoch = entry.epoch
                                g.salt = update.salt
                                if (update.commitment != null) g.commitment = update.commitment
                                pendingTransitions[groupID] = PendingTransitionState.Idle

                                groups[idx] = g
                                storeSalt(groupID, g.epoch, g.salt)
                                syncTransportAndSubscribe(g)
                                try { store.saveGroup(g) } catch (_: Exception) { }
                            }
                            // else: our local state was confirmed — ignore remote fork
                        } catch (e: Exception) {
                            if (BuildConfig.DEBUG) Log.d("GroupListVM", "Fork: chain fetch failed: ${e.message}")
                            // Fall back to accepting remote
                            val idx = groups.indexOfFirst { it.id == groupID }
                            if (idx < 0) return@launch
                            val g = groups[idx]
                            applyMemberDelta(update, g)
                            g.epoch = update.epoch + 1
                            g.salt = update.salt
                            g.recomputeCommitment()
                            groups[idx] = g
                            storeSalt(groupID, g.epoch, g.salt)
                            syncTransportAndSubscribe(g)
                            try { store.saveGroup(g) } catch (_: Exception) { }
                        }
                    }
                    return // Handled async
                }
            }

            // Unpublished: accept remote, bump epoch
            applyMemberDelta(update, group)
            group.epoch += 1
            group.salt = update.salt
            group.recomputeCommitment()

            groups[index] = group
            storeSalt(groupID, group.epoch, group.salt)
            syncTransportAndSubscribe(group)

            viewModelScope.launch {
                try { store.saveGroup(group) } catch (_: Exception) { }
            }
        } else {
            // Normal case: update.epoch > group.epoch
            val selfRemoved = isSelfRemoved(update)

            // Compute candidate state from the update without mutating `group` yet.
            val candidate = cloneGroup(group)
            applyMemberDelta(update, candidate)
            candidate.epoch = update.epoch
            candidate.salt = update.salt
            if (update.commitment != null) candidate.commitment = update.commitment
            else candidate.recomputeCommitment()

            // --- Chain-first verification: for on-chain groups, the contract is
            // authoritative. Accept the update only if the chain's stored
            // commitment at this epoch matches what we computed from the
            // update. Reject otherwise — sender forged the update, raced
            // ahead of chain confirmation, or our RPC hasn't caught up. The
            // sender's retry will eventually reconcile.
            if (candidate.isPublishedOnChain) {
                val service = onChainService
                if (service != null) {
                    viewModelScope.launch {
                        val chainMatches: Boolean = try {
                            val entry = fetchOnChainStateAwaitingEpoch(service, group.groupIDData, expectedEpoch = update.epoch)
                            val localCommitment = candidate.commitment
                            localCommitment != null
                                && entry.active
                                && entry.epoch == update.epoch
                                && entry.commitment.contentEquals(localCommitment)
                        } catch (e: Exception) {
                            if (BuildConfig.DEBUG) Log.w("GroupListVM", "Remote update DROPPED: chain fetch failed: ${e.message} group=${groupID.take(8)}")
                            false
                        }
                        if (!chainMatches) {
                            if (BuildConfig.DEBUG) Log.w("GroupListVM", "Remote update REJECTED: chain disagrees at epoch=${update.epoch} group=${groupID.take(8)}")
                            return@launch
                        }
                        commitRemoteStateUpdate(groupID, candidate, update, selfRemoved)
                    }
                    return
                }
                // No contract service configured but group is published —
                // refuse to apply unverified updates.
                if (BuildConfig.DEBUG) Log.w("GroupListVM", "Remote update DROPPED: onChainService unavailable for published group=${groupID.take(8)}")
                return
            }

            // Unpublished draft group — no chain authority, commit directly.
            commitRemoteStateUpdate(groupID, candidate, update, selfRemoved)
        }
    }

    /**
     * Install a chain-verified candidate state from an incoming SEPGroupStateUpdate.
     * Called from [applyStateUpdate] after on-chain verification succeeds (or for
     * unpublished groups that have no chain authority).
     */
    private fun commitRemoteStateUpdate(
        groupID: String,
        candidate: ChatGroup,
        update: SEPGroupStateUpdate,
        selfRemoved: Boolean
    ) {
        val idx = groups.indexOfFirst { it.id == groupID }
        if (idx < 0) return
        val stored = groups[idx]
        // Re-check staleness: something could have advanced while awaiting chain.
        if (update.epoch <= stored.epoch) return

        // Discard any superseded pending transition
        val pt = pendingTransitions[groupID]
        if (pt is PendingTransitionState.AwaitingChainConfirmation && update.epoch >= pt.targetEpoch) {
            pendingTransitions[groupID] = PendingTransitionState.Idle
        }

        // Refresh chain baseline to match the now-confirmed state.
        if (candidate.isPublishedOnChain) {
            chainBaseline[groupID] = ChainBaseline(
                candidate.members.toList(), candidate.epoch, candidate.salt.copyOf()
            )
        }

        groups[idx] = candidate
        storeSalt(groupID, update.epoch, update.salt)

        if (selfRemoved) {
            if (!BuildConfig.DEBUG) transport.unsubscribe(candidate.topicTag)
            val removerBlsHex = update.senderAttestation?.blsPubkey?.toHex()
            val removerName = removerBlsHex?.let { memberDisplayName(update.senderAttestation!!.blsPubkey) } ?: "unknown"
            candidate.removedByPubkeyHex = removerBlsHex
            groups[idx] = candidate
            viewModelScope.launch { try { store.saveGroup(candidate) } catch (_: Exception) { } }
            insertSystemMessage(groupID, "You were removed from this group by $removerName", "self-removed", update.epoch)
        } else {
            syncTransportAndSubscribe(candidate)
            viewModelScope.launch {
                try { store.saveGroup(candidate) } catch (_: Exception) { }
            }
            insertSystemMessagesForStateUpdate(update, groupID)
        }
    }

    /** Insert system messages for each member change in a state update. */
    private fun insertSystemMessagesForStateUpdate(update: SEPGroupStateUpdate, groupID: String) {
        for (added in update.addedMembers) {
            val name = memberDisplayName(added.publicKeyCompressed)
            insertSystemMessage(groupID, "$name joined the group", "member-add", update.epoch)
        }
        for (removed in update.removedMemberKeys) {
            val name = memberDisplayName(removed)
            insertSystemMessage(groupID, "$name was removed from the group", "member-remove", update.epoch)
        }
        if (update.addedMembers.isEmpty() && update.removedMemberKeys.isEmpty()) {
            insertSystemMessage(groupID, "Encryption key was rotated", "key-rotation", update.epoch)
        }
    }

    /** Apply the member delta (adds/removes) from a state update to a group. */
    private fun applyMemberDelta(update: SEPGroupStateUpdate, group: ChatGroup) {
        for (removed in update.removedMemberKeys) {
            group.members.removeAll { it.publicKeyCompressed.contentEquals(removed) }
        }
        for (added in update.addedMembers) {
            if (group.members.none { it.publicKeyCompressed.contentEquals(added.publicKeyCompressed) }) {
                group.members.add(added)
            }
        }
        sortMembers(group)
    }

    /** Check if the current user's BLS pubkey is in the removal list. */
    private fun isSelfRemoved(update: SEPGroupStateUpdate): Boolean {
        val myLeaf = keyManager.memberLeaf()
        return update.removedMemberKeys.any { it.contentEquals(myLeaf.publicKeyCompressed) }
    }

    /** Check if the current user is a member of the given group. */
    fun isMember(group: ChatGroup): Boolean {
        val myLeaf = keyManager.memberLeaf()
        return group.members.any { it.publicKeyCompressed.contentEquals(myLeaf.publicKeyCompressed) }
    }

    /** Check if the current user can send in this group (considers pinned epoch). */
    fun canSendInGroup(group: ChatGroup): Boolean {
        val myLeaf = keyManager.memberLeaf()
        val pinned = group.pinnedEpoch
        if (pinned != null) {
            val snapshot = epochSnapshots[group.id]?.get(pinned)
            if (snapshot != null) {
                return snapshot.members.any { it.publicKeyCompressed.contentEquals(myLeaf.publicKeyCompressed) }
            }
        }
        return group.members.any { it.publicKeyCompressed.contentEquals(myLeaf.publicKeyCompressed) }
    }

    /** Sync transport members and resubscribe group. */
    private fun syncTransportAndSubscribe(group: ChatGroup) {
        updateCurrentMembers(group.id)
        transport.subscribe(
            group = group,
            keyResolver = { epoch -> epochKey(group.id, epoch) }
        )
    }

    // -- Fork Group --

    /** Whether a removed member can fork this group (epoch snapshots exist where they were a member). */
    fun canForkGroup(group: ChatGroup): Boolean {
        if (isMember(group)) return false
        val myLeaf = keyManager.memberLeaf()
        val snapshots = epochSnapshots[group.id] ?: return false
        return snapshots.values.any { snapshot ->
            snapshot.members.any { it.publicKeyCompressed.contentEquals(myLeaf.publicKeyCompressed) }
        }
    }

    /** Fork a group from the latest epoch where the current user was a member,
     *  excluding specified members. Creates a new, cryptographically independent group. */
    fun forkGroup(
        originalGroupID: String,
        excludingMembers: List<ByteArray>,
        forkName: String? = null,
        onResult: (Result<ChatGroup>) -> Unit
    ) {
        viewModelScope.launch {
            try {
                val originalGroup = groups.find { it.id == originalGroupID }
                    ?: throw ChatError.VerificationFailed("Group not found")
                val myLeaf = keyManager.memberLeaf()
                val snapshots = epochSnapshots[originalGroupID]
                    ?: throw ChatError.VerificationFailed("No epoch snapshots available")

                // Find the latest epoch where the user was a member
                val myKey = myLeaf.publicKeyCompressed
                val forkSnapshot = snapshots.values
                    .filter { snapshot -> snapshot.members.any { it.publicKeyCompressed.contentEquals(myKey) } }
                    .maxByOrNull { it.epoch }
                    ?: throw ChatError.VerificationFailed("No epoch found where you were a member")

                // Generate new group ID and secret
                val random = java.security.SecureRandom()
                val newGroupID = ByteArray(32).also { random.nextBytes(it) }
                val newGroupSecret = ByteArray(32).also { random.nextBytes(it) }

                // Build member list: snapshot members minus excluded, ensuring self is included
                val forkMembers = forkSnapshot.members.filter { member ->
                    excludingMembers.none { it.contentEquals(member.publicKeyCompressed) }
                }.toMutableList()
                if (forkMembers.none { it.publicKeyCompressed.contentEquals(myKey) }) {
                    forkMembers.add(myLeaf)
                }

                val name = forkName?.takeIf { it.isNotBlank() } ?: "Fork of ${originalGroup.name}"
                val forkedGroup = ChatGroup(
                    id = newGroupID.toHex(),
                    name = name,
                    groupSecret = newGroupSecret,
                    members = forkMembers,
                    epoch = 0,
                    salt = SEPCommitmentBuilder.generateSalt(),
                    tier = originalGroup.tier,
                    relayHints = originalGroup.relayHints,
                    forkedFromGroupID = originalGroupID,
                    forkedAtEpoch = forkSnapshot.epoch
                )
                sortMembers(forkedGroup)
                forkedGroup.recomputeCommitment()

                addGroup(forkedGroup)
                captureEpochSnapshot(forkedGroup, "Forked from ${originalGroup.name} at epoch ${forkSnapshot.epoch}")
                ensureOwnTransportBundle(forkedGroup.id)

                // Copy messages from the original group up to the fork epoch
                val originalMessages = chatMessages[originalGroupID] ?: emptyList()
                val copiedMessages = originalMessages.mapNotNull { msg ->
                    if (msg.epoch != null && msg.epoch > forkSnapshot.epoch) return@mapNotNull null
                    msg.copy(
                        id = "fork-${msg.id}",
                        groupID = forkedGroup.id
                    )
                }
                chatMessages[forkedGroup.id] = copiedMessages.toMutableList()
                copiedMessages.maxOfOrNull { it.timestamp.time }?.let {
                    bumpGroupLastMessage(forkedGroup.id, it)
                }
                withContext(Dispatchers.IO) {
                    for (msg in copiedMessages) {
                        try { store.saveMessage(msg) } catch (_: Exception) { }
                    }
                }

                val excludedNames = excludingMembers.map { memberDisplayName(it) }.joinToString(", ")
                val sysText = if (excludedNames.isEmpty())
                    "Group forked from ${originalGroup.name}."
                else
                    "Group forked from ${originalGroup.name}. Excluded: $excludedNames."
                insertSystemMessage(forkedGroup.id, sysText, "group-forked", 0)

                // Publish on-chain if configured (fire-and-forget)
                if (isContractConfigured && onChainService != null) {
                    publishGroupOnChain(forkedGroup) { /* best-effort */ }
                }

                if (BuildConfig.DEBUG) Log.d("GroupListVM", "forkGroup created=${forkedGroup.id.take(8)} from=${originalGroupID.take(8)} epoch=${forkSnapshot.epoch} members=${forkMembers.size}")

                onResult(Result.success(forkedGroup))

                // Send invitations in background (fire-and-forget) — don't block fork creation
                val otherMembers = forkMembers.filter { !it.publicKeyCompressed.contentEquals(myKey) }
                if (otherMembers.isNotEmpty()) {
                    val originalBundles = store.loadTransportBundles(originalGroupID)
                    val payload = chat.onym.android.model.BootstrapPayload.from(
                        forkedGroup, keyManager.publicKeyHex
                    )
                    viewModelScope.launch {
                        var invitedCount = 0
                        var failedCount = 0
                        for ((index, member) in otherMembers.withIndex()) {
                            if (index > 0) delay(500) // Space out relay publishes to avoid rate limiting
                            val hex = member.publicKeyCompressed.toHex()
                            val bundle = originalBundles[hex]
                            if (bundle == null) { failedCount++; continue }
                            try {
                                withContext(Dispatchers.IO) {
                                    invitationTransport.sendInvitation(payload, bundle.x25519InboxPubkey, keyManager)
                                }
                                invitedCount++
                            } catch (e: Exception) {
                                failedCount++
                                if (BuildConfig.DEBUG) Log.w("GroupListVM", "forkGroup: failed to invite $hex: ${e.message}")
                            }
                        }
                        val inviteMsg = when {
                            invitedCount > 0 && failedCount > 0 ->
                                "Sent $invitedCount invitation${if (invitedCount == 1) "" else "s"}, $failedCount failed."
                            invitedCount > 0 ->
                                "Sent invitations to $invitedCount member${if (invitedCount == 1) "" else "s"}."
                            failedCount > 0 ->
                                "Failed to send $failedCount invitation${if (failedCount == 1) "" else "s"}."
                            else -> null
                        }
                        if (inviteMsg != null) {
                            insertSystemMessage(forkedGroup.id, inviteMsg, "fork-invitations-sent", 0)
                        }
                    }
                }
            } catch (e: Exception) {
                onResult(Result.failure(e))
            }
        }
    }

    /** Rotate the group key via chain-confirmed epoch transition. */
    fun rotateGroupKey(groupID: String) {
        val group = groups.find { it.id == groupID } ?: return
        val previousKey = group.encryptionKey

        viewModelScope.launch {
            val result = performEpochTransition(
                groupID = groupID,
                kind = EpochTransitionKind.KeyRotation,
                previousKey = previousKey
            )
            if (BuildConfig.DEBUG) Log.d("GroupListVM", "rotateGroupKey result=$result group=${groupID.take(8)}")
        }
    }

    private fun cloneGroup(group: ChatGroup): ChatGroup {
        return group.copy(
            groupSecret = group.groupSecret.copyOf(),
            members = group.members.toMutableList(),
            salt = group.salt.copyOf(),
            commitment = group.commitment?.copyOf()
        )
    }

    private suspend fun publishMembershipUpdateIfNeeded(
        oldGroup: ChatGroup,
        newGroup: ChatGroup
    ): Result<Unit> {
        if (!oldGroup.isPublishedOnChain) return Result.success(Unit)

        val service = onChainService
            ?: return Result.failure(ChatError.ContractNotConfigured)

        return try {
            val response = withContext(Dispatchers.IO) {
                service.publishCommitmentUpdate(
                    groupIDData = oldGroup.groupIDData,
                    oldMembers = oldGroup.members.toList(),
                    oldEpoch = oldGroup.epoch,
                    oldSalt = oldGroup.salt.copyOf(),
                    newMembers = newGroup.members,
                    newSalt = newGroup.salt,
                    blsSecretKey = keyManager.blsSecretKey,
                    tier = oldGroup.tier
                )
            }
            if (response.accepted) {
                Result.success(Unit)
            } else {
                val reason = response.message ?: "Rejected"
                SecurityLog.onChainOperationFailed("update_commitment", reason)
                Result.failure(ChatError.OnChainPublishFailed(reason))
            }
        } catch (e: Exception) {
            val reason = e.message ?: "Unknown error"
            SecurityLog.onChainOperationFailed("update_commitment", reason)
            Result.failure(ChatError.OnChainPublishFailed(reason))
        }
    }

    /** Remove a member from a group via chain-confirmed epoch transition. */
    fun removeMember(blsPubkey: ByteArray, groupID: String, onResult: (Result<Unit>) -> Unit = {}) {
        viewModelScope.launch {
            val group = groups.find { it.id == groupID }
            if (group == null) {
                onResult(Result.failure(ChatError.VerificationFailed("Group not found")))
                return@launch
            }
            if (group.members.none { it.publicKeyCompressed.contentEquals(blsPubkey) }) {
                onResult(Result.failure(ChatError.VerificationFailed("Member not found")))
                return@launch
            }

            val previousKey = group.encryptionKey

            val result = performEpochTransition(
                groupID = groupID,
                kind = EpochTransitionKind.MemberRemove(blsPubkey),
                previousKey = previousKey
            )

            when (result) {
                EpochTransitionResult.SUCCESS -> onResult(Result.success(Unit))
                EpochTransitionResult.CHAIN_REJECTED -> onResult(Result.failure(ChatError.OnChainPublishFailed("Chain rejected")))
                EpochTransitionResult.CHAIN_UNAVAILABLE -> onResult(Result.failure(ChatError.ContractNotConfigured))
                EpochTransitionResult.CHAIN_MISMATCH -> onResult(Result.failure(ChatError.VerificationFailed("Chain epoch mismatch")))
                EpochTransitionResult.SYNC_REQUIRED -> onResult(Result.failure(ChatError.VerificationFailed("Pending transition")))
                EpochTransitionResult.BUNDLES_MISSING -> onResult(Result.failure(ChatError.VerificationFailed("Transport bundles missing. Rotate key first to distribute bundles, then retry removal.")))
            }
        }
    }

    private fun sortMembers(group: ChatGroup) {
        group.members.sortWith(memberLeafComparator)
    }

    private fun lexCompare(a: ByteArray, b: ByteArray): Int {
        for (i in 0 until minOf(a.size, b.size)) {
            val cmp = (a[i].toInt() and 0xFF) - (b[i].toInt() and 0xFF)
            if (cmp != 0) return cmp
        }
        return a.size - b.size
    }

    /** Handle a member_joined announcement via chain-confirmed epoch transition. */
    private fun handleMemberJoined(member: SEPGroupMemberLeaf, groupID: String, transportBundle: com.stellarmls.mls.SEPMemberTransportBundle? = null) {
        viewModelScope.launch {
            if (BuildConfig.DEBUG) Log.d("GroupListVM", "handleMemberJoined group=${groupID.take(8)}")

            // Extract and persist joiner's transport bundle
            transportBundle?.let { processTransportBundle(it, groupID) }

            val group = groups.find { it.id == groupID } ?: return@launch

            if (group.members.any { it.publicKeyCompressed.contentEquals(member.publicKeyCompressed) }) {
                if (BuildConfig.DEBUG) Log.d("GroupListVM", "handleMemberJoined: already a member")
                return@launch
            }

            val previousKey = group.encryptionKey

            val result = performEpochTransition(
                groupID = groupID,
                kind = EpochTransitionKind.MemberAdd(member),
                previousKey = previousKey
            )

            if (BuildConfig.DEBUG) Log.d("GroupListVM", "handleMemberJoined result=$result group=${groupID.take(8)}")

            if (result != EpochTransitionResult.SUCCESS) {
                SecurityLog.onChainOperationFailed("member_add_transition", result.name)
            }
        }
    }

    /** Apply a re-key message: replace the poisoned salt with the real salt and resubscribe. */
    private fun applyRekey(epoch: Long, salt: ByteArray, groupID: String) {
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) return
        val group = groups[index]
        // Don't apply rekey if we were removed from this group
        if (!isMember(group)) {
            if (BuildConfig.DEBUG) Log.d("GroupListVM", "Ignoring rekey — no longer a member of group=${groupID.take(8)}")
            return
        }
        if (group.epoch != epoch) return
        if (group.salt.contentEquals(salt)) return
        group.salt = salt
        group.recomputeCommitment()
        groups[index] = group
        storeSalt(groupID, epoch, salt)
        syncTransportAndSubscribe(group)
        viewModelScope.launch {
            try { store.saveGroup(group) } catch (_: Exception) { }
            // Update push subscription with new topic tag
            if (group.pushNotificationsEnabled) {
                pushManager?.updateGroupSubscription(
                    groupID = groupID,
                    newTopicTag = group.topicTag,
                    epoch = epoch,
                    groupSecret = group.groupSecret,
                    salt = salt
                )
            }
        }
        if (BuildConfig.DEBUG) Log.d("GroupListVM", "Applied re-key for epoch=$epoch group=${groupID.take(8)}")
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
        insertSystemMessage(groupID, "Group renamed to \"$newName\"", "renamed", updated.epoch)
    }

    /** Derive push relay URL from the first Nostr relay.
     *  e.g. wss://nostr.onym.chat → https://push.onym.chat */
    private fun derivePushRelayURL(): String? {
        val nostrRelay = relayURLs.firstOrNull() ?: return null
        val host = try { java.net.URI(nostrRelay).host } catch (_: Exception) { return null }
            ?: return null
        val domain = host.removePrefix("nostr.")
        return "https://push.$domain"
    }

    /** Get or create PushNotificationManager. */
    private fun getOrCreatePushManager(): PushNotificationManager? {
        pushManager?.let { return it }
        val url = derivePushRelayURL() ?: return null
        val mgr = PushNotificationManager(getApplication(), url)
        pushManager = mgr
        return mgr
    }

    /** Get push token from the platform-specific provider (FCM or UnifiedPush). */
    private suspend fun getPushToken(): String? {
        return try {
            pushTokenProvider.getToken()
        } catch (e: Exception) {
            Log.e("GroupListVM", "Failed to get push token: ${e.message}")
            null
        }
    }

    /** Toggle push notifications for a group and persist the change. */
    fun setPushNotifications(enabled: Boolean, groupID: String) {
        val index = groups.indexOfFirst { it.id == groupID }
        if (index < 0) return
        persistPushEnabled(groupID, enabled)
        groups[index] = groups[index].copy(pushNotificationsEnabled = enabled)
        pushEnabledStates[groupID] = enabled
        val group = groups[index]
        viewModelScope.launch {
            withContext(Dispatchers.IO) { store.saveGroup(group) }
            if (enabled) {
                val mgr = getOrCreatePushManager()
                if (mgr == null) {
                    Log.w("GroupListVM", "Cannot register push: no relay URL")
                    return@launch
                }
                val token = getPushToken()
                if (token == null) {
                    Log.w("GroupListVM", "Cannot register push: no push token")
                    return@launch
                }
                val registered = mgr.registerGroup(group, token, pushTokenProvider.getPlatformName())
                if (registered) {
                    if (BuildConfig.DEBUG) Log.d("GroupListVM", "Push registered for group ${groupID.take(8)}")
                } else {
                    Log.w("GroupListVM", "Push registration failed for group ${groupID.take(8)}")
                }
            } else {
                pushManager?.unregisterGroup(groupID)
                if (BuildConfig.DEBUG) Log.d("GroupListVM", "Push unregistered for group ${groupID.take(8)}")
            }
        }
    }

    /** Called when push token is refreshed — re-register all push-enabled groups. */
    fun onNewPushToken(token: String) {
        viewModelScope.launch {
            val mgr = getOrCreatePushManager() ?: return@launch
            val pushGroups = groups.filter { it.pushNotificationsEnabled }
            if (pushGroups.isNotEmpty()) {
                mgr.reregisterAll(pushGroups, token, pushTokenProvider.getPlatformName())
                if (BuildConfig.DEBUG) Log.d("GroupListVM", "Re-registered ${pushGroups.size} groups with new push token")
            }
        }
    }

    /** Rename a group and broadcast the change to all members. */
    fun renameGroup(groupID: String, newName: String) {
        val trimmed = newName.trim()
        if (trimmed.isEmpty()) return
        val group = groups.find { it.id == groupID } ?: return

        applyGroupRenamed(trimmed, groupID)

        val json = JSONObject().apply {
            put("type", SEPGroupRenamed.MESSAGE_TYPE)
            put("name", trimmed)
        }
        transport.sendProtocolMessage(group, json.toString())
    }

    /** Send an invitation for a group to a recipient identified by their X25519 inbox key hex.
     *  Fire-and-forget: validates inputs synchronously, then sends in background. */
    fun sendInvitation(groupID: String, recipientKeyHex: String, onResult: (Result<Unit>) -> Unit) {
        try {
            val group = groups.find { it.id == groupID }
                ?: throw IllegalStateException("Group not found")
            val pubkeyBytes = recipientKeyHex.hexToBytes()
            require(pubkeyBytes.size == 32) { "Key must be 32 bytes (64 hex chars)" }
            val payload = BootstrapPayload.from(group, keyManager.publicKeyHex)
            // Fire-and-forget: send in background, report success immediately
            viewModelScope.launch {
                try {
                    withContext(Dispatchers.IO) {
                        invitationTransport.sendInvitation(payload, pubkeyBytes, keyManager)
                    }
                } catch (e: Exception) {
                    if (BuildConfig.DEBUG) Log.w("GroupListVM", "sendInvitation failed: ${e.message}")
                }
            }
            onResult(Result.success(Unit))
        } catch (e: Exception) {
            onResult(Result.failure(e))
        }
    }

    /** Create a 1:1 onboarding chat seeded with the inviter, and dispatch an onboard-ack
     *  invitation carrying [nonceHex] so the inviter can reconcile their pending InvitedContact.
     *  Returns the created group, or null on failure. */
    suspend fun completeOnboardInvite(
        inviterKeyAgreementKeyHex: String,
        nonceHex: String,
        displayName: String
    ): ChatGroup? {
        return try {
            val inviterKey = inviterKeyAgreementKeyHex.hexToBytes()
            require(inviterKey.size == 32) { "Inviter key must be 32 bytes" }

            val result = createGroup(displayName) ?: return null
            val group = result.first

            val payload = BootstrapPayload.from(
                group = group,
                senderPubkey = keyManager.publicKeyHex,
                onboardReplyNonce = nonceHex
            )

            withContext(Dispatchers.IO) {
                invitationTransport.sendInvitation(payload, inviterKey, keyManager)
            }
            group
        } catch (e: Exception) {
            if (BuildConfig.DEBUG) Log.w("GroupListVM", "completeOnboardInvite failed: ${e.message}")
            null
        }
    }

    /** Announce ourselves as a new member to the group over the Nostr transport. */
    fun announceMemberJoined(group: ChatGroup) {
        ensureOwnTransportBundle(group.id)
        val member = keyManager.memberLeaf()
        val myBundle = try { keyManager.createTransportBundle() } catch (_: Exception) { null }
        val json = JSONObject().apply {
            put("type", SEPMemberJoined.MESSAGE_TYPE)
            put("member", JSONObject().apply {
                put("publicKeyCompressed", android.util.Base64.encodeToString(
                    member.publicKeyCompressed, android.util.Base64.NO_WRAP))
                put("leafHash", android.util.Base64.encodeToString(
                    member.leafHash, android.util.Base64.NO_WRAP))
            })
            if (myBundle != null) {
                put("transportBundle", BootstrapPayload.serializeTransportBundle(myBundle))
            }
        }.toString()
        transport.sendProtocolMessage(group, json)
    }

    /** Set up handler for incoming chat messages — stores in chatMessages, persists to store. */
    private fun setupChatMessageHandler() {
        transport.onMessage = { groupID, senderPubkey, text, eventID, timestampMs, epoch, replyToID ->
            val seen = seenMessageIDs.computeIfAbsent(groupID) { java.util.Collections.synchronizedSet(mutableSetOf()) }
            if (seen.add(eventID)) {
                val isGovernance = isGovernancePayload(text)
                if (isGovernance) {
                    processGovernanceEvent(groupID, text, senderPubkey)
                }
                val msg = ChatMessage(
                    id = eventID,
                    groupID = groupID,
                    senderPubkey = senderPubkey,
                    text = text,
                    timestamp = Date(timestampMs),
                    isMine = senderPubkey == keyManager.blsPublicKey().toHex(),
                    isSystemMessage = isGovernance,
                    epoch = epoch,
                    replyToID = replyToID
                )
                // Append to trigger Compose recomposition. Arrival order prevents
                // messages received after epoch transition from jumping into the middle.
                chatMessages[groupID] = (chatMessages[groupID] ?: emptyList()) + msg
                bumpGroupLastMessage(groupID, msg.timestamp.time)
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
                // Update lastEventTimestamp (stored in seconds for relay catch-up filters)
                val group = groups.find { it.id == groupID }
                val timestampSec = timestampMs / 1000
                if (group != null && timestampSec > group.lastEventTimestamp) {
                    group.lastEventTimestamp = timestampSec
                    viewModelScope.launch {
                        try { store.saveGroup(group) } catch (_: Exception) { }
                    }
                }
            }
        }
    }

    /** Set up handler for incoming image messages — stores in chatMessages with media attachment. */
    private fun setupImageMessageHandler() {
        transport.onImageMessage = { groupID, text, media, eventID, senderPubkey, timestampMs, epoch, replyToID ->
            val seen = seenMessageIDs.computeIfAbsent(groupID) { java.util.Collections.synchronizedSet(mutableSetOf()) }
            if (seen.add(eventID)) {
                val msg = ChatMessage(
                    id = eventID,
                    groupID = groupID,
                    senderPubkey = senderPubkey,
                    text = text,
                    timestamp = Date(timestampMs),
                    isMine = senderPubkey == keyManager.blsPublicKey().toHex(),
                    mediaAttachment = media,
                    epoch = epoch,
                    replyToID = replyToID
                )
                chatMessages[groupID] = (chatMessages[groupID] ?: emptyList()) + msg
                bumpGroupLastMessage(groupID, msg.timestamp.time)
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
                val timestampSec = timestampMs / 1000
                if (group != null && timestampSec > group.lastEventTimestamp) {
                    group.lastEventTimestamp = timestampSec
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
                    viewModelScope.launch {
                        try { store.updateMessageStatus(eventID, newStatus) } catch (_: Exception) { }
                    }
                    break
                }
            }
        }
    }

    /** Send a chat message in a group. */
    fun sendMessage(groupID: String, text: String, replyToID: String? = null) {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) return
        val group = groups.find { it.id == groupID } ?: return

        // Block sending if we're no longer a member (considers pinned epoch)
        if (!canSendInGroup(group)) return

        // Use effective key/topic/epoch when pinned to a historical epoch
        val key = effectiveEncryptionKey(group)
        val topicTag = effectiveTopicTag(group)
        val epoch = effectiveEpoch(group)

        // Use the deterministic NIP-01 event ID so the relay echo is deduplicated,
        // and the event's createdAt so timestamp matches what other clients see.
        val event = transport.send(
            group, trimmed,
            overrideKey = key,
            overrideTopicTag = topicTag,
            epoch = epoch,
            replyToID = replyToID
        )

        val msg = ChatMessage(
            id = event.id,
            groupID = groupID,
            senderPubkey = keyManager.blsPublicKey().toHex(),
            text = trimmed,
            timestamp = Date(event.displayMilliseconds),
            isMine = true,
            isSystemMessage = isGovernancePayload(trimmed),
            status = MessageStatus.SENDING,
            epoch = epoch,
            replyToID = replyToID
        )
        chatMessages[groupID] = (chatMessages[groupID] ?: emptyList()) + msg
        seenMessageIDs.computeIfAbsent(groupID) { java.util.Collections.synchronizedSet(mutableSetOf()) }.add(event.id)
        bumpGroupLastMessage(groupID, msg.timestamp.time)
        viewModelScope.launch {
            try { store.saveMessage(msg) } catch (_: Exception) { }
        }
    }

    /** Retry a failed message by re-encrypting and re-publishing. */
    private val retryingMessageIDs = java.util.Collections.synchronizedSet(mutableSetOf<String>())

    fun retryMessage(groupID: String, messageID: String) {
        val group = groups.find { it.id == groupID } ?: return
        val messages = chatMessages[groupID] ?: return
        val idx = messages.indexOfFirst { it.id == messageID && it.status == MessageStatus.FAILED }
        if (idx < 0) return
        if (!retryingMessageIDs.add(messageID)) return

        val failedMsg = messages[idx]
        val updated = messages.toMutableList()
        updated[idx] = failedMsg.copy(status = MessageStatus.SENDING)
        chatMessages[groupID] = updated

        viewModelScope.launch {
            try {
                withContext(Dispatchers.IO) {
                    store.updateMessageStatus(messageID, MessageStatus.SENDING)
                }

                // transport.send() serialises NIP-01 events and publishes to relays.
                // Keep it off the Main dispatcher to match the surrounding pattern.
                val event = withContext(Dispatchers.IO) {
                    transport.send(
                        group, failedMsg.text,
                        overrideKey = effectiveEncryptionKey(group),
                        overrideTopicTag = effectiveTopicTag(group),
                        epoch = effectiveEpoch(group),
                        replyToID = failedMsg.replyToID
                    )
                }

                seenMessageIDs.computeIfAbsent(groupID) {
                    java.util.Collections.synchronizedSet(mutableSetOf())
                }.add(event.id)

                val newMsg = failedMsg.copy(id = event.id, status = MessageStatus.SENDING)

                // Update the in-memory list BEFORE the IO write. setupOKHandler
                // looks up chatMessages by event.id when the relay acks, and the
                // ack can arrive while this coroutine is suspended inside the
                // replaceMessage IO call. If we updated in-memory after the IO
                // write, the OK handler would miss the message (still keyed by
                // the old ID) and leave it stuck in SENDING until next restart.
                val current = chatMessages[groupID]?.toMutableList() ?: return@launch
                val i = current.indexOfFirst { it.id == messageID }
                if (i >= 0) {
                    current[i] = newMsg
                    chatMessages[groupID] = current
                }

                withContext(Dispatchers.IO) {
                    store.replaceMessage(messageID, newMsg)
                }
            } catch (e: Exception) {
                Log.e("GroupListVM", "retryMessage failed", e)
                val reverted = chatMessages[groupID]?.toMutableList() ?: return@launch
                val ri = reverted.indexOfFirst { it.id == messageID }
                if (ri >= 0) {
                    reverted[ri] = reverted[ri].copy(status = MessageStatus.FAILED)
                    chatMessages[groupID] = reverted
                }
                try {
                    withContext(Dispatchers.IO) {
                        store.updateMessageStatus(messageID, MessageStatus.FAILED)
                    }
                } catch (_: Exception) { }
            } finally {
                retryingMessageIDs.remove(messageID)
            }
        }
    }

    /** Send an encrypted image in a group via Blossom. */
    fun sendImage(groupID: String, imageData: ByteArray, replyToID: String? = null) {
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
                    if (replyToID != null) put("replyTo", replyToID)
                }

                val effKey = effectiveEncryptionKey(group)
                val effTopicTag = effectiveTopicTag(group)
                val effEpoch = effectiveEpoch(group)
                val envelopeJson = GroupCrypto.encrypt(wrapper.toString(), effKey)
                val content = android.util.Base64.encodeToString(
                    envelopeJson.toByteArray(), android.util.Base64.NO_WRAP)

                val tags = mutableListOf(listOf("t", effTopicTag))
                tags.add(listOf("epoch", effEpoch.toString()))
                val event = NostrEventBuilder.build(
                    kind = 44114,
                    tags = tags,
                    content = content,
                    keyManager = keyManager,
                    ephemeralSigner = RustBackedNostrSigner.ephemeral()
                )

                // Publish to all relays
                transport.publish(event)

                val msg = ChatMessage(
                    id = event.id,
                    groupID = groupID,
                    senderPubkey = keyManager.blsPublicKey().toHex(),
                    text = "\uD83D\uDDBC\uFE0F Image",
                    timestamp = Date(event.displayMilliseconds),
                    isMine = true,
                    status = MessageStatus.SENDING,
                    mediaAttachment = media,
                    epoch = effEpoch,
                    replyToID = replyToID
                )
                chatMessages[groupID] = (chatMessages[groupID] ?: emptyList()) + msg
                seenMessageIDs.computeIfAbsent(groupID) { java.util.Collections.synchronizedSet(mutableSetOf()) }.add(event.id)
                bumpGroupLastMessage(groupID, msg.timestamp.time)
                launch {
                    try { store.saveMessage(msg) } catch (_: Exception) { }
                }
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) android.util.Log.e("GroupListVM", "sendImage failed: ${e.message}")
            }
        }
    }

    fun sendVideo(groupID: String, context: Context, uri: android.net.Uri, replyToID: String? = null) {
        val group = groups.find { it.id == groupID } ?: return

        viewModelScope.launch {
            try {
                // Read video bytes and check size
                val videoData = withContext(Dispatchers.IO) {
                    context.contentResolver.openInputStream(uri)?.readBytes()
                } ?: return@launch

                if (videoData.size > MediaCrypto.MAX_VIDEO_BYTES) {
                    if (BuildConfig.DEBUG) Log.w("GroupListVM", "Video too large: ${videoData.size} bytes")
                    return@launch
                }

                // Extract metadata
                val meta = withContext(Dispatchers.Default) {
                    MediaCrypto.videoMetadata(context, uri)
                } ?: return@launch

                // Generate thumbnail from first frame
                val thumbData = withContext(Dispatchers.Default) {
                    MediaCrypto.generateVideoThumbnail(context, uri)
                }

                // Encrypt video with a fresh per-file key
                val (encryptedBlob, fileKey) = MediaCrypto.encryptMedia(videoData)
                val encThumb = thumbData?.let { MediaCrypto.encryptMedia(it, fileKey) }

                // Upload to Blossom
                val blobHash = withContext(Dispatchers.IO) {
                    BlossomClient.upload(encryptedBlob, blossomServerURLs.toList(), keyManager)
                }

                val media = MediaAttachment(
                    blobHash = blobHash,
                    fileKey = fileKey,
                    mimeType = "video/mp4",
                    width = meta.first,
                    height = meta.second,
                    size = encryptedBlob.size,
                    blossomServers = blossomServerURLs.toList(),
                    encryptedThumbnail = encThumb,
                    duration = meta.third
                )

                // Build v2 wrapper with video type
                val mediaJson = JSONObject().apply {
                    put("blobHash", media.blobHash)
                    put("fileKey", android.util.Base64.encodeToString(media.fileKey, android.util.Base64.NO_WRAP))
                    put("mimeType", media.mimeType)
                    put("width", media.width)
                    put("height", media.height)
                    put("size", media.size)
                    put("blossomServers", JSONArray(media.blossomServers))
                    put("duration", meta.third)
                    if (encThumb != null) {
                        put("thumbnail", android.util.Base64.encodeToString(encThumb, android.util.Base64.NO_WRAP))
                    }
                }
                val wrapper = JSONObject().apply {
                    put("v", 2)
                    put("type", "video")
                    put("text", "Sent a video")
                    put("media", mediaJson)
                    put("senderBlsPubkey", android.util.Base64.encodeToString(
                        keyManager.blsPublicKey(), android.util.Base64.NO_WRAP))
                    put("ts", System.currentTimeMillis() / 1000)
                    if (replyToID != null) put("replyTo", replyToID)
                }

                val effKey = effectiveEncryptionKey(group)
                val effTopicTag = effectiveTopicTag(group)
                val effEpoch = effectiveEpoch(group)
                val envelopeJson = GroupCrypto.encrypt(wrapper.toString(), effKey)
                val content = android.util.Base64.encodeToString(
                    envelopeJson.toByteArray(), android.util.Base64.NO_WRAP)

                val tags = mutableListOf(listOf("t", effTopicTag))
                tags.add(listOf("epoch", effEpoch.toString()))
                val event = NostrEventBuilder.build(
                    kind = 44114,
                    tags = tags,
                    content = content,
                    keyManager = keyManager,
                    ephemeralSigner = RustBackedNostrSigner.ephemeral()
                )

                transport.publish(event)

                val msg = ChatMessage(
                    id = event.id,
                    groupID = groupID,
                    senderPubkey = keyManager.blsPublicKey().toHex(),
                    text = "Sent a video",
                    timestamp = Date(event.displayMilliseconds),
                    isMine = true,
                    status = MessageStatus.SENDING,
                    mediaAttachment = media,
                    epoch = effEpoch,
                    replyToID = replyToID
                )
                chatMessages[groupID] = (chatMessages[groupID] ?: emptyList()) + msg
                seenMessageIDs.computeIfAbsent(groupID) { java.util.Collections.synchronizedSet(mutableSetOf()) }.add(event.id)
                bumpGroupLastMessage(groupID, msg.timestamp.time)
                launch {
                    try { store.saveMessage(msg) } catch (_: Exception) { }
                }
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) Log.e("GroupListVM", "sendVideo failed: ${e.message}")
            }
        }
    }

    /** Send an encrypted voice message in a group via Blossom. */
    fun sendVoice(groupID: String, audioFile: java.io.File, replyToID: String? = null) {
        val group = groups.find { it.id == groupID } ?: return

        viewModelScope.launch {
            try {
                val audioData = withContext(Dispatchers.IO) { audioFile.readBytes() }
                if (audioData.size > MediaCrypto.MAX_AUDIO_BYTES) {
                    if (BuildConfig.DEBUG) Log.w("GroupListVM", "Audio too large: ${audioData.size} bytes")
                    return@launch
                }

                val durationSecs = withContext(Dispatchers.Default) {
                    MediaCrypto.audioMetadata(getApplication(), android.net.Uri.fromFile(audioFile))
                } ?: return@launch

                val (encryptedBlob, fileKey) = MediaCrypto.encryptMedia(audioData)

                val blobHash = withContext(Dispatchers.IO) {
                    BlossomClient.upload(encryptedBlob, blossomServerURLs.toList(), keyManager)
                }

                val media = chat.onym.android.model.MediaAttachment(
                    blobHash = blobHash,
                    fileKey = fileKey,
                    mimeType = "audio/aac",
                    width = 0,
                    height = 0,
                    size = encryptedBlob.size,
                    blossomServers = blossomServerURLs.toList(),
                    encryptedThumbnail = null,
                    duration = durationSecs
                )

                val mediaJson = org.json.JSONObject().apply {
                    put("blobHash", media.blobHash)
                    put("fileKey", android.util.Base64.encodeToString(media.fileKey, android.util.Base64.NO_WRAP))
                    put("mimeType", media.mimeType)
                    put("width", 0)
                    put("height", 0)
                    put("size", media.size)
                    put("blossomServers", org.json.JSONArray(media.blossomServers))
                    put("duration", durationSecs)
                }
                val wrapper = org.json.JSONObject().apply {
                    put("v", 2)
                    put("type", "audio")
                    put("text", "Sent a voice message")
                    put("media", mediaJson)
                    put("senderBlsPubkey", android.util.Base64.encodeToString(
                        keyManager.blsPublicKey(), android.util.Base64.NO_WRAP))
                    put("ts", System.currentTimeMillis() / 1000)
                    if (replyToID != null) put("replyTo", replyToID)
                }

                val effKey = effectiveEncryptionKey(group)
                val effTopicTag = effectiveTopicTag(group)
                val effEpoch = effectiveEpoch(group)
                val envelopeJson = chat.onym.android.crypto.GroupCrypto.encrypt(wrapper.toString(), effKey)
                val content = android.util.Base64.encodeToString(
                    envelopeJson.toByteArray(), android.util.Base64.NO_WRAP)
                val tags = mutableListOf(listOf("t", effTopicTag))
                tags.add(listOf("epoch", effEpoch.toString()))
                val event = chat.onym.android.crypto.NostrEventBuilder.build(
                    kind = 44114, tags = tags, content = content, keyManager = keyManager,
                    ephemeralSigner = RustBackedNostrSigner.ephemeral())

                transport.publish(event)

                val msg = chat.onym.android.model.ChatMessage(
                    id = event.id,
                    groupID = groupID,
                    senderPubkey = keyManager.blsPublicKey().toHex(),
                    text = "Sent a voice message",
                    timestamp = java.util.Date(event.displayMilliseconds),
                    isMine = true,
                    status = chat.onym.android.model.MessageStatus.SENDING,
                    mediaAttachment = media,
                    epoch = effEpoch,
                    replyToID = replyToID
                )
                val current = (chatMessages[groupID] ?: emptyList()) + msg
                chatMessages[groupID] = current.sortedWith(
                    compareBy<chat.onym.android.model.ChatMessage> { it.timestamp }.thenBy { it.id })
                seenMessageIDs.computeIfAbsent(groupID) { java.util.Collections.synchronizedSet(mutableSetOf()) }.add(event.id)
                bumpGroupLastMessage(groupID, msg.timestamp.time)
                launch {
                    try { store.saveMessage(msg) } catch (_: Exception) { }
                }

                // Clean up temp file
                audioFile.delete()
            } catch (e: Exception) {
                if (BuildConfig.DEBUG) Log.e("GroupListVM", "sendVoice failed: ${e.message}")
            }
        }
    }

    // MARK: - Call Signaling

    /** Send a call signaling message (offer/answer/ice/hangup) to a group. */
    fun sendCallSignal(groupID: String, callJson: JSONObject) {
        val group = groups.find { it.id == groupID } ?: return
        val wrapper = JSONObject().apply {
            put("v", 2)
            put("type", "call")
            put("text", "")
            put("senderBlsPubkey", android.util.Base64.encodeToString(
                keyManager.blsPublicKey(), android.util.Base64.NO_WRAP))
            put("ts", System.currentTimeMillis() / 1000)
            put("call", callJson)
        }
        val key = effectiveEncryptionKey(group)
        val envelopeJson = GroupCrypto.encrypt(wrapper.toString(), key)
        val content = android.util.Base64.encodeToString(
            envelopeJson.toByteArray(), android.util.Base64.NO_WRAP)
        val tags = listOf(listOf("t", effectiveTopicTag(group)))
        val event = NostrEventBuilder.build(
            kind = 44114, tags = tags, content = content, keyManager = keyManager,
            ephemeralSigner = RustBackedNostrSigner.ephemeral())
        transport.publish(event)
    }

    private fun setupCallSignalHandler() {
        transport.onCallSignal = { groupID, callJson, senderPubkey, eventID ->
            viewModelScope.launch {
                // Configure signaling channel for incoming calls before handling
                callManager.sendSignal = { outgoingJson ->
                    sendCallSignal(groupID, outgoingJson)
                }
                callManager.handleSignal(callJson, senderPubkey)
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
                            val bundle = if (obj.has("transportBundle")) {
                                try { BootstrapPayload.deserializeTransportBundle(obj.getJSONObject("transportBundle")) }
                                catch (_: Exception) { null }
                            } else null
                            handleMemberJoined(member, groupID, bundle)
                        }
                        SEPGroupStateUpdate.MESSAGE_TYPE -> {
                            val update = parseStateUpdate(obj)
                            applyStateUpdate(update, groupID)
                        }
                        SEPSaltRequest.MESSAGE_TYPE -> {
                            val epoch = obj.getLong("epoch")
                            // Phase 4: Refuse salt responses for removal epochs — the requester
                            // should use SEPRekeyResendRequest via inbox instead.
                            val isRemoval = try {
                                kotlinx.coroutines.runBlocking { store.isRemovalEpoch(groupID, epoch.toInt()) }
                            } catch (_: Exception) { false }
                            if (isRemoval) {
                                if (BuildConfig.DEBUG) Log.d("GroupListVM", "Salt request for removal epoch $epoch — refusing")
                            } else {
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
                        }
                        SEPSaltResponse.MESSAGE_TYPE -> {
                            val epoch = obj.getLong("epoch")
                            val saltB64 = obj.getString("salt")
                            val salt = android.util.Base64.decode(saltB64, android.util.Base64.NO_WRAP)
                            storeSalt(groupID, epoch, salt)
                        }
                        SEPRekey.MESSAGE_TYPE -> {
                            val rekeyEpoch = obj.getLong("epoch")
                            val rekeySalt = android.util.Base64.decode(obj.getString("salt"), android.util.Base64.NO_WRAP)
                            applyRekey(rekeyEpoch, rekeySalt, groupID)
                        }
                        SEPGroupRenamed.MESSAGE_TYPE -> {
                            val newName = obj.getString("name")
                            applyGroupRenamed(newName, groupID)
                        }
                        SEPRemovalNotice.MESSAGE_TYPE -> {
                            // Non-secret removal notice — check if we were removed
                            val removedArr = obj.optJSONArray("removedMemberKeys") ?: JSONArray()
                            val myBls = keyManager.blsPublicKey()
                            var selfRemoved = false
                            for (i in 0 until removedArr.length()) {
                                val key = android.util.Base64.decode(removedArr.getString(i), android.util.Base64.NO_WRAP)
                                if (key.contentEquals(myBls)) { selfRemoved = true; break }
                            }
                            if (selfRemoved) {
                                val noticeEpoch = obj.getLong("epoch")
                                // Use BLS pubkey from the message wrapper (extracted by transport)
                                val removerBlsHex = senderPubkey
                                val idx = groups.indexOfFirst { it.id == groupID }
                                if (idx >= 0) {
                                    var g = cloneGroup(groups[idx])
                                    // Capture epoch snapshot BEFORE mutation so fork can find all members
                                    captureEpochSnapshot(g, "Member removed (you)")
                                    // Update epoch and remove self from member list
                                    g = g.copy(epoch = noticeEpoch)
                                    g.removedByPubkeyHex = removerBlsHex
                                    g.members.removeAll { it.publicKeyCompressed.contentEquals(myBls) }
                                    groups[idx] = g
                                    viewModelScope.launch { try { store.saveGroup(g) } catch (_: Exception) { } }
                                    if (!BuildConfig.DEBUG) transport.unsubscribe(g.topicTag)
                                    // Rebuild currentMembers so BLS check rejects our own messages too
                                    updateCurrentMembers(groupID)
                                }
                                val removerName = contactAliasStore.displayName(senderPubkey) ?: (senderPubkey.take(8) + "...")
                                insertSystemMessage(groupID, "You were removed from this group by $removerName", "self-removed", noticeEpoch)
                                if (BuildConfig.DEBUG) Log.d("GroupListVM", "Self-removed from group=${groupID.take(8)} epoch=$noticeEpoch")
                            }
                        }
                        SEPRekeyAck.MESSAGE_TYPE -> {
                            val ackEpoch = obj.getLong("epoch")
                            val senderHex = android.util.Base64.decode(
                                obj.getString("senderBlsPubkey"), android.util.Base64.NO_WRAP).toHex()
                            viewModelScope.launch {
                                try { store.updatePendingRekeyAck(groupID, ackEpoch.toInt(), senderHex) } catch (_: Exception) { }
                            }
                        }
                        SEPRekeyResendRequest.MESSAGE_TYPE -> {
                            val reqEpoch = obj.getLong("epoch")
                            val bundleObj = obj.optJSONObject("requesterBundle")
                            if (bundleObj != null) {
                                val requesterBundle = BootstrapPayload.deserializeTransportBundle(bundleObj)
                                handleRekeyResendRequest(groupID, reqEpoch, requesterBundle)
                            }
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
        val bundle = update.senderTransportBundle
        if (bundle != null) {
            obj.put("senderTransportBundle", BootstrapPayload.serializeTransportBundle(bundle))
        }
        if (update.memberBundles.isNotEmpty()) {
            val bundlesArr = JSONArray()
            for (b in update.memberBundles) {
                bundlesArr.put(BootstrapPayload.serializeTransportBundle(b))
            }
            obj.put("memberBundles", bundlesArr)
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

        val transportBundle = if (obj.has("senderTransportBundle")) {
            try { BootstrapPayload.deserializeTransportBundle(obj.getJSONObject("senderTransportBundle")) }
            catch (_: Exception) { null }
        } else null

        val memberBundles = if (obj.has("memberBundles")) {
            val arr = obj.getJSONArray("memberBundles")
            (0 until arr.length()).mapNotNull { i ->
                try { BootstrapPayload.deserializeTransportBundle(arr.getJSONObject(i)) }
                catch (_: Exception) { null }
            }
        } else emptyList()

        return SEPGroupStateUpdate(
            epoch = obj.getLong("epoch"),
            salt = salt,
            addedMembers = addedMembers,
            removedMemberKeys = removedKeys,
            commitment = commitment,
            senderAttestation = attestation,
            senderTransportBundle = transportBundle,
            memberBundles = memberBundles
        )
    }

    override fun onCleared() {
        transport.disconnect()
        invitationTransport.disconnect()
        super.onCleared()
    }
}
