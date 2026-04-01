package com.stellarmls.chat

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import com.stellarmls.chat.ui.screens.ChatScreen
import com.stellarmls.chat.ui.screens.CreateGroupScreen
import com.stellarmls.chat.ui.screens.GroupListScreen
import com.stellarmls.chat.ui.screens.InviteMemberScreen
import com.stellarmls.chat.ui.screens.JoinGroupScreen
import com.stellarmls.chat.ui.screens.PendingInvitationsScreen
import com.stellarmls.chat.ui.screens.SettingsScreen
import com.stellarmls.chat.ui.theme.StellarChatTheme
import com.stellarmls.chat.viewmodel.ChatViewModel
import com.stellarmls.chat.viewmodel.CreateGroupViewModel
import com.stellarmls.chat.viewmodel.GroupListViewModel
import com.stellarmls.chat.viewmodel.JoinGroupViewModel

class MainActivity : ComponentActivity() {
    private val groupListViewModel: GroupListViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        // Handle deep link: stellarchat://join?code=<base64>
        val deepLinkCode = intent?.data?.getQueryParameter("code")

        setContent {
            StellarChatTheme {
                StellarChatNavHost(groupListViewModel, deepLinkInviteCode = deepLinkCode)
            }
        }
    }
}

@Composable
fun StellarChatNavHost(groupListViewModel: GroupListViewModel, deepLinkInviteCode: String? = null) {
    val navController = rememberNavController()

    // Navigate to join screen if deep link contains invite code
    androidx.compose.runtime.LaunchedEffect(deepLinkInviteCode) {
        if (deepLinkInviteCode != null) {
            navController.navigate("join")
        }
    }

    NavHost(navController = navController, startDestination = "groups") {
        composable("groups") {
            GroupListScreen(
                groups = groupListViewModel.groups,
                pendingInvitationCount = groupListViewModel.pendingInvitations.size,
                onGroupClick = { group ->
                    navController.navigate("chat/${group.id}")
                },
                onInviteMember = { group ->
                    navController.navigate("invite/${group.id}")
                },
                onCreateGroup = { navController.navigate("create") },
                onJoinGroup = { navController.navigate("join") },
                onSettings = { navController.navigate("settings") },
                onInvitations = { navController.navigate("invitations") },
                onDeleteGroup = { id -> groupListViewModel.removeGroup(id) }
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
                onInvite = { navController.navigate("invite/$groupId") }
            )
        }

        composable("create") {
            val createViewModel: CreateGroupViewModel = viewModel()
            CreateGroupScreen(
                viewModel = createViewModel,
                keyManager = groupListViewModel.keyManager,
                groupListViewModel = groupListViewModel,
                onBack = { navController.popBackStack() },
                onGroupCreated = { group ->
                    groupListViewModel.addGroup(group)
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
                onBack = { navController.popBackStack() },
                onGroupJoined = { group ->
                    // Add ourselves to the member list so our messages pass BLS auth
                    val myLeaf = groupListViewModel.keyManager.memberLeaf()
                    if (group.members.none { it.publicKeyCompressed.contentEquals(myLeaf.publicKeyCompressed) }) {
                        group.members.add(myLeaf)
                    }
                    groupListViewModel.addGroup(group)
                    groupListViewModel.announceMemberJoined(group)
                    navController.popBackStack()
                }
            )
        }

        composable("settings") {
            SettingsScreen(
                viewModel = groupListViewModel,
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

        composable("invitations") {
            PendingInvitationsScreen(
                invitations = groupListViewModel.pendingInvitations,
                groupListViewModel = groupListViewModel,
                onAccept = { invitation, verificationResult ->
                    val group = invitation.payload.toChatGroup()
                    // Mark as published if on-chain verification passed
                    if (verificationResult is com.stellarmls.chat.onchain.OnChainVerificationResult.Verified) {
                        group.isPublishedOnChain = true
                    }
                    // Add ourselves to the member list so our messages pass BLS auth
                    val myLeaf = groupListViewModel.keyManager.memberLeaf()
                    if (group.members.none { it.publicKeyCompressed.contentEquals(myLeaf.publicKeyCompressed) }) {
                        group.members.add(myLeaf)
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
