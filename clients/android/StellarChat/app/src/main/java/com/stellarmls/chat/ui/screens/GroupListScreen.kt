package com.stellarmls.chat.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Email
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.stellarmls.chat.model.ChatGroup

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GroupListScreen(
    groups: List<ChatGroup>,
    pendingInvitationCount: Int = 0,
    onGroupClick: (ChatGroup) -> Unit,
    onInviteMember: (ChatGroup) -> Unit = {},
    onCreateGroup: () -> Unit,
    onJoinGroup: () -> Unit,
    onSettings: () -> Unit,
    onInvitations: () -> Unit = {},
    onDeleteGroup: (String) -> Unit
) {
    var showAddDialog by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Stellar Chat") },
                actions = {
                    // Invitations button with badge
                    IconButton(onClick = onInvitations) {
                        BadgedBox(
                            badge = {
                                if (pendingInvitationCount > 0) {
                                    Badge { Text("$pendingInvitationCount") }
                                }
                            }
                        ) {
                            Icon(Icons.Default.Email, contentDescription = "Invitations")
                        }
                    }
                    IconButton(onClick = onSettings) {
                        Icon(Icons.Default.Settings, contentDescription = "Settings")
                    }
                }
            )
        },
        floatingActionButton = {
            FloatingActionButton(onClick = { showAddDialog = true }) {
                Icon(Icons.Default.Add, contentDescription = "Add Group")
            }
        }
    ) { padding ->
        if (groups.isEmpty()) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding),
                contentAlignment = Alignment.Center
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        "No groups yet",
                        style = MaterialTheme.typography.titleLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    Text(
                        "Tap + to create or join a group",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 8.dp)
                    )
                }
            }
        } else {
            LazyColumn(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding)
            ) {
                itemsIndexed(groups, key = { _, g -> g.id }) { _, group ->
                    Card(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = 16.dp, vertical = 4.dp)
                            .clickable { onGroupClick(group) }
                    ) {
                        Row(
                            modifier = Modifier.padding(16.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            // On-chain status indicator
                            if (group.isPublishedOnChain) {
                                Text(
                                    text = "\u2713",
                                    color = Color(0xFF4CAF50),
                                    style = MaterialTheme.typography.titleMedium
                                )
                            } else {
                                Text(
                                    text = "\u25CB",
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    style = MaterialTheme.typography.titleMedium
                                )
                            }
                            Spacer(modifier = Modifier.width(12.dp))
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    group.name,
                                    style = MaterialTheme.typography.titleMedium
                                )
                                Text(
                                    "${group.members.size} members | Epoch ${group.epoch}",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    if (showAddDialog) {
        AlertDialog(
            onDismissRequest = { showAddDialog = false },
            title = { Text("Add Group") },
            text = { Text("Create a new group or join an existing one?") },
            confirmButton = {
                TextButton(onClick = {
                    showAddDialog = false
                    onCreateGroup()
                }) {
                    Text("Create")
                }
            },
            dismissButton = {
                TextButton(onClick = {
                    showAddDialog = false
                    onJoinGroup()
                }) {
                    Text("Join")
                }
            }
        )
    }
}
