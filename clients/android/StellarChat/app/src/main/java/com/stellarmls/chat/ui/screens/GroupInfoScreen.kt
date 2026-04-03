package com.stellarmls.chat.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.PersonRemove
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
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
import androidx.compose.ui.unit.dp
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.toHex
import com.stellarmls.mls.SEPGroupMemberLeaf

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GroupInfoScreen(
    group: ChatGroup,
    myBlsPubkey: ByteArray,
    onRemoveMember: (ByteArray, (Result<Unit>) -> Unit) -> Unit,
    onRotateKey: () -> Unit = {},
    onRenameGroup: (String) -> Unit = {},
    onBack: () -> Unit
) {
    var memberToRemove by remember { mutableStateOf<SEPGroupMemberLeaf?>(null) }
    var removalStatus by remember { mutableStateOf<String?>(null) }
    var removalStatusIsError by remember { mutableStateOf(false) }
    var isRemovingMember by remember { mutableStateOf(false) }
    var showRenameDialog by remember { mutableStateOf(false) }
    var newGroupName by remember { mutableStateOf(group.name) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Group Info") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                }
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(16.dp)
                .verticalScroll(rememberScrollState())
        ) {
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Group", style = MaterialTheme.typography.titleMedium)
                    Spacer(modifier = Modifier.height(8.dp))
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable {
                                newGroupName = group.name
                                showRenameDialog = true
                            },
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        InfoRow("Name", group.name)
                        Icon(
                            Icons.Filled.Edit,
                            contentDescription = "Rename",
                            modifier = Modifier.size(16.dp),
                            tint = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }
                    InfoRow("Epoch", group.epoch.toString())
                    InfoRow("Members", group.members.size.toString())
                    InfoRow("Tier", group.tier.displayName)
                    if (group.isPublishedOnChain) {
                        InfoRow("On-Chain", "Verified")
                    }
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Members", style = MaterialTheme.typography.titleMedium)
                    Spacer(modifier = Modifier.height(8.dp))

                    for (member in group.members) {
                        val isMe = member.publicKeyCompressed.contentEquals(myBlsPubkey)
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = 4.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    member.publicKeyCompressed.toHex().take(16) + "...",
                                    style = MaterialTheme.typography.bodySmall
                                )
                                if (isMe) {
                                    Text(
                                        "You",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.primary
                                    )
                                }
                            }
                            if (!isMe) {
                                IconButton(
                                    onClick = { memberToRemove = member },
                                    enabled = !isRemovingMember
                                ) {
                                    Icon(
                                        Icons.Filled.PersonRemove,
                                        contentDescription = "Remove",
                                        tint = if (isRemovingMember) MaterialTheme.colorScheme.onSurfaceVariant
                                            else MaterialTheme.colorScheme.error
                                    )
                                }
                            }
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Key Rotation", style = MaterialTheme.typography.titleMedium)
                    Spacer(modifier = Modifier.height(8.dp))
                    androidx.compose.material3.Button(
                        onClick = {
                            onRotateKey()
                            removalStatus = "Key rotated."
                            removalStatusIsError = false
                        },
                        modifier = Modifier.fillMaxWidth()
                    ) {
                        Text("Rotate Group Key")
                    }
                    Text(
                        "Generate a new encryption key without changing membership. Provides forward secrecy.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 4.dp)
                    )
                }
            }

            if (isRemovingMember) {
                Spacer(modifier = Modifier.height(16.dp))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
                    Spacer(modifier = Modifier.size(8.dp))
                    Text(
                        "Removing member and updating on-chain state...",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }

            removalStatus?.let { status ->
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    status,
                    color = if (removalStatusIsError) {
                        MaterialTheme.colorScheme.error
                    } else {
                        MaterialTheme.colorScheme.primary
                    },
                    style = MaterialTheme.typography.bodySmall
                )
            }
        }
    }

    memberToRemove?.let { member ->
        AlertDialog(
            onDismissRequest = { memberToRemove = null },
            title = { Text("Remove Member") },
            text = { Text("The member will be removed and the group key will be rotated. They will not be able to decrypt future messages.") },
            confirmButton = {
                TextButton(onClick = {
                    isRemovingMember = true
                    removalStatus = null
                    onRemoveMember(member.publicKeyCompressed) { result ->
                        isRemovingMember = false
                        if (result.isSuccess) {
                            removalStatus = "Member removed. Key rotated."
                            removalStatusIsError = false
                        } else {
                            removalStatus = result.exceptionOrNull()?.message ?: "Failed to remove member."
                            removalStatusIsError = true
                        }
                    }
                    memberToRemove = null
                }) {
                    Text("Remove", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { memberToRemove = null }) {
                    Text("Cancel")
                }
            }
        )
    }

    if (showRenameDialog) {
        AlertDialog(
            onDismissRequest = { showRenameDialog = false },
            title = { Text("Rename Group") },
            text = {
                androidx.compose.material3.OutlinedTextField(
                    value = newGroupName,
                    onValueChange = { newGroupName = it },
                    label = { Text("Group name") },
                    singleLine = true
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    onRenameGroup(newGroupName)
                    removalStatus = "Group renamed."
                    removalStatusIsError = false
                    showRenameDialog = false
                }) {
                    Text("Rename")
                }
            },
            dismissButton = {
                TextButton(onClick = { showRenameDialog = false }) {
                    Text("Cancel")
                }
            }
        )
    }
}

@Composable
private fun InfoRow(label: String, value: String) {
    Row(modifier = Modifier.padding(vertical = 2.dp)) {
        Text(label, style = MaterialTheme.typography.labelMedium, modifier = Modifier.weight(1f))
        Text(value, style = MaterialTheme.typography.bodyMedium)
    }
}

private val com.stellarmls.mls.SEPTier.displayName: String
    get() = when (this) {
        com.stellarmls.mls.SEPTier.SMALL -> "Small (up to 32)"
        com.stellarmls.mls.SEPTier.MEDIUM -> "Medium (up to 256)"
        com.stellarmls.mls.SEPTier.LARGE -> "Large (up to 2,048)"
    }
