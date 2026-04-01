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
import com.stellarmls.chat.ui.screens.JoinGroupScreen
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
        setContent {
            StellarChatTheme {
                StellarChatNavHost(groupListViewModel)
            }
        }
    }
}

@Composable
fun StellarChatNavHost(groupListViewModel: GroupListViewModel) {
    val navController = rememberNavController()

    NavHost(navController = navController, startDestination = "groups") {
        composable("groups") {
            GroupListScreen(
                groups = groupListViewModel.groups,
                onGroupClick = { group ->
                    navController.navigate("chat/${group.id}")
                },
                onCreateGroup = { navController.navigate("create") },
                onJoinGroup = { navController.navigate("join") },
                onSettings = { navController.navigate("settings") },
                onDeleteGroup = { index -> groupListViewModel.removeGroup(index) }
            )
        }

        composable(
            "chat/{groupId}",
            arguments = listOf(navArgument("groupId") { type = NavType.StringType })
        ) { backStackEntry ->
            val groupId = backStackEntry.arguments?.getString("groupId") ?: return@composable
            val group = groupListViewModel.groups.find { it.id == groupId } ?: return@composable

            val chatViewModel = remember(groupId) {
                ChatViewModel(
                    group = group,
                    transport = groupListViewModel.transport,
                    myPubkey = groupListViewModel.keyManager.publicKeyHex
                )
            }

            ChatScreen(
                groupName = group.name,
                viewModel = chatViewModel,
                onBack = { navController.popBackStack() }
            )
        }

        composable("create") {
            val createViewModel: CreateGroupViewModel = viewModel()
            CreateGroupScreen(
                viewModel = createViewModel,
                keyManager = groupListViewModel.keyManager,
                onBack = { navController.popBackStack() },
                onGroupCreated = { group ->
                    groupListViewModel.addGroup(group)
                    navController.popBackStack()
                }
            )
        }

        composable("join") {
            val joinViewModel: JoinGroupViewModel = viewModel()
            JoinGroupScreen(
                viewModel = joinViewModel,
                onBack = { navController.popBackStack() },
                onGroupJoined = { group ->
                    groupListViewModel.addGroup(group)
                    navController.popBackStack()
                }
            )
        }

        composable("settings") {
            SettingsScreen(
                publicKeyHex = groupListViewModel.keyManager.publicKeyHex,
                relayURLs = listOf("wss://relay.damus.io", "wss://nos.lol"),
                onBack = { navController.popBackStack() }
            )
        }
    }
}
