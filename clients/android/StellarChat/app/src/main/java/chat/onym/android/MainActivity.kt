package chat.onym.android

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Chat
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavGraph.Companion.findStartDestination
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import chat.onym.android.ui.screens.ChatScreen
import chat.onym.android.ui.screens.ContactsScreen
import chat.onym.android.ui.screens.CreateGroupScreen
import chat.onym.android.ui.screens.ForkGroupScreen
import chat.onym.android.ui.screens.GroupInfoScreen
import chat.onym.android.ui.screens.GroupListScreen
import chat.onym.android.ui.screens.InviteMemberScreen
import chat.onym.android.ui.screens.SearchScreen
import chat.onym.android.ui.screens.JoinGroupScreen
import chat.onym.android.ui.screens.LegacyIdentityScreen
import chat.onym.android.ui.screens.OnboardLandingScreen
import chat.onym.android.ui.screens.PendingInvitationsScreen
import chat.onym.android.ui.screens.RecoveryPhraseScreen
import chat.onym.android.ui.screens.RestoreIdentityScreen
import chat.onym.android.ui.screens.SettingsScreen
import chat.onym.android.ui.screens.AdvancedSettingsScreen
import chat.onym.android.ui.screens.BlossomServersScreen
import chat.onym.android.ui.screens.RelaysScreen
import chat.onym.android.ui.screens.StellarContractScreen
import chat.onym.android.ui.screens.TurnServersScreen
import chat.onym.android.ui.theme.StellarChatTheme
import chat.onym.android.viewmodel.ChatViewModel
import chat.onym.android.viewmodel.CreateGroupViewModel
import chat.onym.android.viewmodel.GroupListViewModel
import chat.onym.android.viewmodel.JoinGroupViewModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {
    private val groupListViewModel: GroupListViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        // Handle deep link: stellarchat://join?code=<base64>
        val deepLinkCode = intent?.data?.getQueryParameter("code")
        // Handle deep link: onym://onboard?inviter=<hex>&nonce=<hex>
        val onboardInviter = intent?.data?.getQueryParameter("inviter")
        val onboardNonce = intent?.data?.getQueryParameter("nonce")
        val onboardPayload = if (onboardInviter != null && onboardNonce != null) {
            OnboardDeepLink(onboardInviter, onboardNonce)
        } else null
        // Handle notification tap: navigate to specific chat
        val notificationGroupId = intent?.getStringExtra("navigate_group_id")

        // Handle notification tap when activity is already running
        addOnNewIntentListener { newIntent ->
            val groupId = newIntent.getStringExtra("navigate_group_id") ?: return@addOnNewIntentListener
            groupListViewModel.pendingNavigationGroupId = groupId
        }

        setContent {
            StellarChatTheme {
                StellarChatNavHost(
                    groupListViewModel,
                    deepLinkInviteCode = deepLinkCode,
                    deepLinkOnboard = onboardPayload,
                    notificationGroupId = notificationGroupId
                )
            }
        }
    }
}

data class OnboardDeepLink(val inviter: String, val nonce: String)

private enum class Tab(val route: String, val label: String, val icon: ImageVector) {
    Contacts("contacts", "Contacts", Icons.Default.Person),
    Chats("groups", "Chats", Icons.AutoMirrored.Filled.Chat),
    Search("search", "Search", Icons.Default.Search),
    Settings("settings", "Settings", Icons.Default.Settings)
}

private val defaultTab = Tab.Chats
private val tabs = listOf(Tab.Contacts, Tab.Chats, Tab.Search, Tab.Settings)
private val tabRoutes = tabs.map { it.route }.toSet()

@Composable
fun StellarChatNavHost(
    groupListViewModel: GroupListViewModel,
    deepLinkInviteCode: String? = null,
    deepLinkOnboard: OnboardDeepLink? = null,
    notificationGroupId: String? = null
) {
    val navController = rememberNavController()

    // Navigate to join screen if deep link contains invite code
    androidx.compose.runtime.LaunchedEffect(deepLinkInviteCode) {
        if (deepLinkInviteCode != null) {
            navController.navigate("join")
        }
    }

    // Navigate to onboard landing if deep link contains inviter + nonce
    androidx.compose.runtime.LaunchedEffect(deepLinkOnboard) {
        if (deepLinkOnboard != null) {
            navController.navigate("onboard/${deepLinkOnboard.inviter}/${deepLinkOnboard.nonce}")
        }
    }

    // Navigate to chat from notification tap (cold start — wait for groups to load)
    androidx.compose.runtime.LaunchedEffect(notificationGroupId, groupListViewModel.groups.size) {
        if (notificationGroupId != null && groupListViewModel.groups.any { it.id == notificationGroupId }) {
            navController.navigate("chat/$notificationGroupId")
        }
    }

    // Navigate to chat from notification tap (warm start — activity already running)
    val pendingGroupId = groupListViewModel.pendingNavigationGroupId
    androidx.compose.runtime.LaunchedEffect(pendingGroupId) {
        if (pendingGroupId != null) {
            if (groupListViewModel.groups.any { it.id == pendingGroupId }) {
                navController.navigate("chat/$pendingGroupId")
            }
            groupListViewModel.pendingNavigationGroupId = null
        }
    }

    // Notification permission handling for push toggle (Android 13+)
    val context = LocalContext.current
    val notificationPermissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { _ -> /* Permission result doesn't gate registration — FCM works regardless */ }

    // Call permission handling — request RECORD_AUDIO (+ CAMERA for video) before starting a call
    var pendingCallVideo by remember { androidx.compose.runtime.mutableStateOf<Boolean?>(null) }
    var pendingCallGroupId by remember { androidx.compose.runtime.mutableStateOf<String?>(null) }
    val callPermissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { grants ->
        val audioGranted = grants[Manifest.permission.RECORD_AUDIO] == true
        val video = pendingCallVideo ?: false
        val groupId = pendingCallGroupId
        val cameraGranted = !video || grants[Manifest.permission.CAMERA] == true
        if (audioGranted && cameraGranted && groupId != null) {
            val cm = groupListViewModel.callManager
            cm.sendSignal = { callJson ->
                groupListViewModel.sendCallSignal(groupId, callJson)
            }
            cm.startCall(video)
        }
        pendingCallVideo = null
        pendingCallGroupId = null
    }

    val navBackStackEntry by navController.currentBackStackEntryAsState()
    val currentRoute = navBackStackEntry?.destination?.route
    val showBottomBar = currentRoute in tabRoutes

    Scaffold(
        bottomBar = {
            if (showBottomBar) {
                NavigationBar {
                    tabs.forEach { tab ->
                        NavigationBarItem(
                            selected = currentRoute == tab.route,
                            onClick = {
                                navController.navigate(tab.route) {
                                    popUpTo(navController.graph.findStartDestination().id) {
                                        saveState = true
                                    }
                                    launchSingleTop = true
                                    restoreState = true
                                }
                            },
                            icon = { Icon(tab.icon, contentDescription = tab.label) },
                            label = { Text(tab.label) }
                        )
                    }
                }
            }
        }
    ) { padding ->
        NavHost(
            navController = navController,
            startDestination = defaultTab.route,
            modifier = Modifier
                .padding(padding)
                .consumeWindowInsets(padding)
        ) {
            composable("contacts") {
                ContactsScreen(
                    groups = groupListViewModel.groups,
                    chatMessages = groupListViewModel.chatMessages,
                    contactAliasStore = groupListViewModel.contactAliasStore,
                    invitedContactStore = groupListViewModel.invitedContactStore,
                    myKeyAgreementPublicKeyHex = groupListViewModel.keyManager.keyAgreementPublicKeyHex,
                    dao = groupListViewModel.store.dao
                )
            }

            composable("search") {
                SearchScreen(
                    groups = groupListViewModel.groups,
                    chatMessages = groupListViewModel.chatMessages,
                    onGroupClick = { group ->
                        navController.navigate("chat/${group.id}")
                    }
                )
            }

            composable("groups") {
                GroupListScreen(
                    groups = groupListViewModel.groups,
                    pendingInvitationCount = groupListViewModel.pendingInvitations.size,
                    chatMessages = groupListViewModel.chatMessages,
                    unreadCounts = groupListViewModel.unreadCounts,
                    isRelayConnected = groupListViewModel.isRelayConnected,
                    onGroupClick = { group ->
                        navController.navigate("chat/${group.id}")
                    },
                    onInviteMember = { group ->
                        navController.navigate("invite/${group.id}")
                    },
                    onCreateGroup = { navController.navigate("create") },
                    onJoinGroup = { navController.navigate("join") },
                    onInvitations = { navController.navigate("invitations") },
                    onDeleteGroup = { id -> groupListViewModel.removeGroup(id) },
                    onTogglePin = { id -> groupListViewModel.togglePinGroup(id) },
                    onRefresh = { groupListViewModel.reconnectRelays() },
                    onRestore = { navController.navigate("restore_identity") }
                )
            }

            composable(
                "chat/{groupId}",
                arguments = listOf(navArgument("groupId") { type = NavType.StringType })
            ) { backStackEntry ->
                val groupId = backStackEntry.arguments?.getString("groupId") ?: return@composable
                if (groupListViewModel.groups.none { it.id == groupId }) return@composable

                val chatViewModel = remember(groupId) {
                    ChatViewModel(
                        groupID = groupId,
                        groupListViewModel = groupListViewModel
                    )
                }

                ChatScreen(
                    viewModel = chatViewModel,
                    onBack = { navController.popBackStack() },
                    onGroupInfo = { navController.navigate("groupinfo/$groupId") },
                    onStartCall = { video ->
                        val permissions = if (video) {
                            arrayOf(Manifest.permission.RECORD_AUDIO, Manifest.permission.CAMERA)
                        } else {
                            arrayOf(Manifest.permission.RECORD_AUDIO)
                        }
                        val allGranted = permissions.all {
                            ContextCompat.checkSelfPermission(context, it) == PackageManager.PERMISSION_GRANTED
                        }
                        if (allGranted) {
                            val cm = groupListViewModel.callManager
                            cm.sendSignal = { callJson ->
                                groupListViewModel.sendCallSignal(groupId, callJson)
                            }
                            cm.startCall(video)
                        } else {
                            pendingCallVideo = video
                            pendingCallGroupId = groupId
                            callPermissionLauncher.launch(permissions)
                        }
                    },
                    contactAliasStore = groupListViewModel.contactAliasStore,
                    onUnpinEpoch = {
                        groupListViewModel.unpinEpoch(groupId)
                    },
                    groupListViewModel = groupListViewModel,
                    onForkGroup = if (groupListViewModel.canForkGroup(
                        groupListViewModel.groups.find { it.id == groupId } ?: return@composable
                    )) {{ navController.navigate("fork/$groupId") }} else null,
                    pushNotificationsEnabled = groupListViewModel.pushEnabledStates[groupId]
                        ?: (groupListViewModel.groups.find { it.id == groupId }?.pushNotificationsEnabled == true),
                    onEnablePushNotifications = {
                        groupListViewModel.setPushNotifications(true, groupId)
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                        ) {
                            notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                        }
                    }
                )

                // Show call screen overlay when a call is active
                val callState = groupListViewModel.callManager.state
                if (callState != chat.onym.android.call.CallState.IDLE) {
                    chat.onym.android.call.CallScreen(
                        callManager = groupListViewModel.callManager,
                        remoteName = chatViewModel.groupName,
                        onDismiss = {}
                    )
                }
            }

            composable("create") {
                val createViewModel: CreateGroupViewModel = viewModel()
                CreateGroupScreen(
                    viewModel = createViewModel,
                    keyManager = groupListViewModel.keyManager,
                    groupListViewModel = groupListViewModel,
                    invitationTransport = groupListViewModel.invitationTransport,
                    onBack = { navController.popBackStack() },
                    onNavigateToChat = { groupId ->
                        navController.navigate("chat/$groupId") {
                            popUpTo("create") { inclusive = true }
                        }
                    }
                )
            }

            composable("join") {
                val joinViewModel: JoinGroupViewModel = viewModel()
                // Pre-fill from deep link if available
                if (deepLinkInviteCode != null && joinViewModel.inviteText.isEmpty()) {
                    joinViewModel.inviteText = deepLinkInviteCode
                }
                JoinGroupScreen(
                    viewModel = joinViewModel,
                    groupListViewModel = groupListViewModel,
                    onBack = { navController.popBackStack() },
                    onGroupJoined = { group ->
                        // Mark as published if on-chain verification passed
                        if (joinViewModel.verificationResult is chat.onym.android.onchain.OnChainVerificationResult.Verified) {
                            group.isPublishedOnChain = true
                        }
                        // Add ourselves to the member list so our messages pass BLS auth
                        val myLeaf = groupListViewModel.keyManager.memberLeaf()
                        if (group.members.none { it.publicKeyCompressed.contentEquals(myLeaf.publicKeyCompressed) }) {
                            group.members.add(myLeaf)
                            group.members.sortWith(GroupListViewModel.memberLeafComparator)
                            // Recompute so the joiner's local state matches what the
                            // creator will publish on SEPMemberJoined (critical for 1v1,
                            // where the creator publishes at epoch=0 with the two-member
                            // commitment).
                            group.recomputeCommitment()
                        }
                        groupListViewModel.addGroup(group)
                        groupListViewModel.announceMemberJoined(group)
                        navController.popBackStack()
                    }
                )
            }

            composable("settings") {
                val settingsScope = androidx.compose.runtime.rememberCoroutineScope()
                SettingsScreen(
                    viewModel = groupListViewModel,
                    onBackupRecoveryPhrase = { navController.navigate("recovery_phrase") },
                    onRegenerateAndBackup = { onError ->
                        settingsScope.launch(Dispatchers.IO) {
                            try {
                                val newKM = chat.onym.android.crypto.KeyManager.regenerateWithBip39(context)
                                withContext(Dispatchers.Main) {
                                    groupListViewModel.replaceKeyManager(newKM)
                                    navController.navigate("recovery_phrase")
                                }
                            } catch (e: Exception) {
                                withContext(Dispatchers.Main) {
                                    onError(e.message ?: "Unknown error")
                                }
                            }
                        }
                    },
                    onOpenRelays = { navController.navigate("relays") },
                    onOpenBlossom = { navController.navigate("blossom_servers") },
                    onOpenTurn = { navController.navigate("turn_servers") },
                    onOpenStellarContract = { navController.navigate("stellar_contract") },
                    onOpenAdvanced = { navController.navigate("advanced_settings") },
                    onJoinGroup = { navController.navigate("join") }
                )
            }

            composable("relays") {
                RelaysScreen(
                    viewModel = groupListViewModel,
                    onBack = { navController.popBackStack() }
                )
            }

            composable("blossom_servers") {
                BlossomServersScreen(
                    viewModel = groupListViewModel,
                    onBack = { navController.popBackStack() }
                )
            }

            composable("turn_servers") {
                TurnServersScreen(
                    viewModel = groupListViewModel,
                    onBack = { navController.popBackStack() }
                )
            }

            composable("stellar_contract") {
                StellarContractScreen(
                    viewModel = groupListViewModel,
                    onBack = { navController.popBackStack() }
                )
            }

            composable("advanced_settings") {
                AdvancedSettingsScreen(
                    viewModel = groupListViewModel,
                    onBack = { navController.popBackStack() }
                )
            }

            composable("recovery_phrase") {
                val phrase = groupListViewModel.keyManager.recoveryPhrase
                if (phrase != null) {
                    RecoveryPhraseScreen(
                        recoveryPhrase = phrase,
                        onBack = { navController.popBackStack() }
                    )
                } else {
                    // Legacy (pre-BIP39) install — no recovery phrase available
                    LegacyIdentityScreen(onBack = { navController.popBackStack() })
                }
            }

            composable("restore_identity") {
                val coroutineScope = androidx.compose.runtime.rememberCoroutineScope()
                RestoreIdentityScreen(
                    onRestore = { mnemonic, onResult ->
                        coroutineScope.launch(Dispatchers.IO) {
                            try {
                                val newKeyManager = chat.onym.android.crypto.KeyManager.restoreFromMnemonic(
                                    context,
                                    mnemonic
                                )
                                withContext(Dispatchers.Main) {
                                    groupListViewModel.replaceKeyManager(newKeyManager)
                                    onResult(true)
                                }
                            } catch (e: Exception) {
                                withContext(Dispatchers.Main) {
                                    onResult(false)
                                }
                            }
                        }
                    },
                    onBack = { navController.popBackStack() }
                )
            }

            composable(
                "invite/{groupId}",
                arguments = listOf(navArgument("groupId") { type = NavType.StringType })
            ) { backStackEntry ->
                val groupId = backStackEntry.arguments?.getString("groupId") ?: return@composable
                val group = groupListViewModel.groups.find { it.id == groupId } ?: return@composable

                InviteMemberScreen(
                    group = group,
                    invitationTransport = groupListViewModel.invitationTransport,
                    keyManager = groupListViewModel.keyManager,
                    onBack = { navController.popBackStack() }
                )
            }

            composable(
                "groupinfo/{groupId}",
                arguments = listOf(navArgument("groupId") { type = NavType.StringType })
            ) { backStackEntry ->
                val groupId = backStackEntry.arguments?.getString("groupId") ?: return@composable
                val group = groupListViewModel.groups.find { it.id == groupId } ?: return@composable

                val inviteCode = remember(group.id, group.epoch) {
                    chat.onym.android.model.InviteCode(
                        groupID = group.groupIDData,
                        groupSecret = group.groupSecret,
                        name = group.name,
                        relayHints = group.relayHints,
                        members = group.members.toList(),
                        epoch = group.epoch,
                        salt = group.salt,
                        commitment = group.commitment,
                        tierRawValue = group.tier.id,
                        groupTypeRawValue = group.groupType.id,
                        adminPubkeys = group.adminPubkeys.toList()
                    )
                }

                val pushEnabled = groupListViewModel.pushEnabledStates[groupId] ?: group.pushNotificationsEnabled

                GroupInfoScreen(
                    group = group.copy(pushNotificationsEnabled = pushEnabled),
                    myBlsPubkey = groupListViewModel.keyManager.blsPublicKey(),
                    onRemoveMember = { blsPubkey, onResult ->
                        groupListViewModel.removeMember(blsPubkey, groupId, onResult)
                    },
                    onRotateKey = {
                        groupListViewModel.rotateGroupKey(groupId)
                    },
                    onRenameGroup = { newName ->
                        groupListViewModel.renameGroup(groupId, newName)
                    },
                    epochSnapshots = groupListViewModel.epochSnapshots[groupId] ?: emptyMap(),
                    onPinEpoch = { epoch ->
                        groupListViewModel.pinEpoch(groupId, epoch)
                    },
                    onUnpinEpoch = {
                        groupListViewModel.unpinEpoch(groupId)
                    },
                    canForkGroup = groupListViewModel.canForkGroup(group),
                    onForkGroup = {
                        navController.navigate("fork/$groupId")
                    },
                    onSendInvitation = { recipientKeyHex, onResult ->
                        groupListViewModel.sendInvitation(groupId, recipientKeyHex, onResult)
                    },
                    inviteLink = "https://onym.chat/join?code=${inviteCode.encode()}",
                    onTogglePushNotifications = { enabled ->
                        groupListViewModel.setPushNotifications(enabled, groupId)
                        if (enabled && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                        ) {
                            notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                        }
                    },
                    onProposeRemovalVote = { targetHex ->
                        groupListViewModel.proposeRemovalVote(groupId, targetHex)
                    },
                    onPromoteToAdmin = { targetHex ->
                        groupListViewModel.promoteToAdmin(groupId, targetHex)
                    },
                    onDemoteFromAdmin = { targetHex ->
                        groupListViewModel.demoteFromAdmin(groupId, targetHex)
                    },
                    contactAliasStore = groupListViewModel.contactAliasStore,
                    dao = groupListViewModel.store.dao,
                    onBack = { navController.popBackStack() }
                )
            }

            composable(
                "fork/{groupId}",
                arguments = listOf(navArgument("groupId") { type = NavType.StringType })
            ) { backStackEntry ->
                val groupId = backStackEntry.arguments?.getString("groupId") ?: return@composable
                val group = groupListViewModel.groups.find { it.id == groupId } ?: return@composable

                ForkGroupScreen(
                    originalGroup = group,
                    myBlsPubkey = groupListViewModel.keyManager.blsPublicKey(),
                    epochSnapshots = groupListViewModel.epochSnapshots[groupId] ?: emptyMap(),
                    removedByPubkeyHex = group.removedByPubkeyHex,
                    onCreateFork = { excludingMembers, forkName, onResult ->
                        groupListViewModel.forkGroup(groupId, excludingMembers, forkName, onResult = onResult)
                    },
                    onNavigateToGroup = { newGroupId ->
                        navController.navigate("chat/$newGroupId") {
                            popUpTo("fork/$groupId") { inclusive = true }
                        }
                    },
                    contactAliasStore = groupListViewModel.contactAliasStore,
                    onBack = { navController.popBackStack() }
                )
            }

            composable(
                "onboard/{inviter}/{nonce}",
                arguments = listOf(
                    navArgument("inviter") { type = NavType.StringType },
                    navArgument("nonce") { type = NavType.StringType }
                )
            ) { backStackEntry ->
                val inviter = backStackEntry.arguments?.getString("inviter") ?: return@composable
                val nonce = backStackEntry.arguments?.getString("nonce") ?: return@composable
                OnboardLandingScreen(
                    inviterX25519Hex = inviter,
                    nonceHex = nonce,
                    groupListViewModel = groupListViewModel,
                    onDone = { navController.popBackStack() }
                )
            }

            composable("invitations") {
                PendingInvitationsScreen(
                    invitations = groupListViewModel.pendingInvitations,
                    groupListViewModel = groupListViewModel,
                    contactAliasStore = groupListViewModel.contactAliasStore,
                    onAccept = { invitation, verificationResult ->
                        val group = invitation.payload.toChatGroup()
                        // Mark as published if on-chain verification passed
                        if (verificationResult is chat.onym.android.onchain.OnChainVerificationResult.Verified) {
                            group.isPublishedOnChain = true
                        }
                        // Add ourselves to the member list so our messages pass BLS auth
                        val myLeaf = groupListViewModel.keyManager.memberLeaf()
                        if (group.members.none { it.publicKeyCompressed.contentEquals(myLeaf.publicKeyCompressed) }) {
                            group.members.add(myLeaf)
                            group.members.sortWith(GroupListViewModel.memberLeafComparator)
                            group.recomputeCommitment()
                        }
                        groupListViewModel.addGroup(group)
                        groupListViewModel.announceMemberJoined(group)
                        groupListViewModel.removePendingInvitation(invitation.id)
                        navController.popBackStack()
                    },
                    onDecline = { invitation ->
                        groupListViewModel.removePendingInvitation(invitation.id)
                    },
                    onBack = { navController.popBackStack() }
                )
            }
        }
    }
}
