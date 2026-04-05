package com.stellarmls.chat.ui.screens

import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.background
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Circle
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.PersonRemove
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.Switch
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.EpochSnapshot
import com.stellarmls.chat.model.toHex
import com.stellarmls.chat.ui.components.QRScannerView
import com.stellarmls.mls.SEPGroupMemberLeaf
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GroupInfoScreen(
    group: ChatGroup,
    myBlsPubkey: ByteArray,
    onRemoveMember: (ByteArray, (Result<Unit>) -> Unit) -> Unit,
    onRotateKey: () -> Unit = {},
    onRenameGroup: (String) -> Unit = {},
    epochSnapshots: Map<Long, EpochSnapshot> = emptyMap(),
    onPinEpoch: (Long) -> Unit = {},
    onUnpinEpoch: () -> Unit = {},
    canForkGroup: Boolean = false,
    onForkGroup: () -> Unit = {},
    onSendInvitation: ((recipientKeyHex: String, onResult: (Result<Unit>) -> Unit) -> Unit)? = null,
    inviteLink: String? = null,
    onTogglePushNotifications: ((Boolean) -> Unit)? = null,
    onBack: () -> Unit
) {
    val isMember = group.members.any { it.publicKeyCompressed.contentEquals(myBlsPubkey) }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var memberToRemove by remember { mutableStateOf<SEPGroupMemberLeaf?>(null) }
    var removalStatus by remember { mutableStateOf<String?>(null) }
    var removalStatusIsError by remember { mutableStateOf(false) }
    var isRemovingMember by remember { mutableStateOf(false) }
    var showRenameDialog by remember { mutableStateOf(false) }
    var newGroupName by remember { mutableStateOf(group.name) }
    // Invite member state
    var recipientKey by remember { mutableStateOf("") }
    var inviteStatus by remember { mutableStateOf<String?>(null) }
    var inviteLinkCopied by remember { mutableStateOf(false) }
    var showScanner by remember { mutableStateOf(false) }

    if (showScanner) {
        Box(modifier = Modifier.fillMaxSize()) {
            QRScannerView { scannedCode ->
                val trimmed = scannedCode.trim()
                if (trimmed.length == 64 && trimmed.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }) {
                    recipientKey = trimmed
                }
                showScanner = false
            }
        }
        return
    }

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
                        val removed = group.members.none { it.publicKeyCompressed.contentEquals(myBlsPubkey) }
                        if (removed) {
                            InfoRow("On-Chain", "Diverged \u26A0\uFE0F")
                        } else {
                            InfoRow("On-Chain", "Verified")
                        }
                    }
                    if (onTogglePushNotifications != null) {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = 2.dp),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text("Push Notifications", style = MaterialTheme.typography.labelMedium)
                            Switch(
                                checked = group.pushNotificationsEnabled,
                                onCheckedChange = { onTogglePushNotifications(it) }
                            )
                        }
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
                            if (!isMe && isMember) {
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

            // Invite Member section
            if (isMember && onSendInvitation != null) {
                Spacer(modifier = Modifier.height(16.dp))
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text("Invite Member", style = MaterialTheme.typography.titleMedium)
                        Spacer(modifier = Modifier.height(8.dp))
                        OutlinedTextField(
                            value = recipientKey,
                            onValueChange = { recipientKey = it; inviteStatus = null },
                            label = { Text("X25519 inbox key (64 hex chars)") },
                            modifier = Modifier.fillMaxWidth(),
                            singleLine = true,
                            textStyle = MaterialTheme.typography.bodySmall
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            OutlinedButton(
                                onClick = {
                                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                                    val clip = clipboard.primaryClip
                                    if (clip != null && clip.itemCount > 0) {
                                        recipientKey = clip.getItemAt(0).text?.toString()?.trim() ?: ""
                                    }
                                },
                                modifier = Modifier.weight(1f)
                            ) {
                                Text("Paste")
                            }
                            OutlinedButton(
                                onClick = { showScanner = true },
                                modifier = Modifier.weight(1f)
                            ) {
                                Text("Scan QR")
                            }
                        }
                        Spacer(modifier = Modifier.height(8.dp))
                        Button(
                            onClick = {
                                inviteStatus = null
                                onSendInvitation(recipientKey.trim()) { result ->
                                    result.onSuccess { inviteStatus = "Invitation sent!" }
                                    result.onFailure { inviteStatus = "Error: ${it.message}" }
                                }
                            },
                            modifier = Modifier.fillMaxWidth(),
                            enabled = recipientKey.trim().length == 64
                        ) {
                            Text("Send Invitation")
                        }
                        inviteStatus?.let { status ->
                            Spacer(modifier = Modifier.height(4.dp))
                            Text(
                                status,
                                style = MaterialTheme.typography.bodySmall,
                                color = if (status.startsWith("Error")) MaterialTheme.colorScheme.error
                                    else MaterialTheme.colorScheme.primary
                            )
                        }
                        Text(
                            "Enter the recipient's inbox key. They can find it in Settings.",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(top = 4.dp)
                        )
                    }
                }
            }

            // Copy Invite Link section
            if (isMember && inviteLink != null) {
                Spacer(modifier = Modifier.height(16.dp))
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text("Invite Link", style = MaterialTheme.typography.titleMedium)
                        Spacer(modifier = Modifier.height(8.dp))
                        Button(
                            onClick = {
                                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
                                clipboard.setPrimaryClip(android.content.ClipData.newPlainText("Invite Link", inviteLink))
                                inviteLinkCopied = true
                                scope.launch {
                                    delay(2000)
                                    inviteLinkCopied = false
                                }
                            },
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Text(if (inviteLinkCopied) "Link Copied!" else "Copy Invite Link")
                        }
                        Text(
                            "Share this link with someone who has the app installed.",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(top = 4.dp)
                        )
                    }
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            if (isMember) {
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
            }

            // Fork section for removed members
            if (!isMember && canForkGroup) {
                Spacer(modifier = Modifier.height(16.dp))
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text("Fork", style = MaterialTheme.typography.titleMedium)
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            "Create a new group from a past epoch, excluding members you choose.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        Button(
                            onClick = onForkGroup,
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Text("Fork This Group")
                        }
                    }
                }
            }

            // Epoch History section
            if (epochSnapshots.isNotEmpty()) {
                Spacer(modifier = Modifier.height(16.dp))

                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text("Epoch History", style = MaterialTheme.typography.titleMedium)
                        Spacer(modifier = Modifier.height(8.dp))

                        if (group.pinnedEpoch != null) {
                            Button(
                                onClick = { onUnpinEpoch() },
                                modifier = Modifier.fillMaxWidth()
                            ) {
                                Text("Follow Latest Epoch")
                            }
                            Spacer(modifier = Modifier.height(8.dp))
                        }

                        val sortedSnapshots = epochSnapshots.entries.sortedByDescending { it.key }
                        for ((epoch, snapshot) in sortedSnapshots) {
                            val isPinned = group.pinnedEpoch == epoch
                            val isRekeyBoundary = !snapshot.groupSecret.contentEquals(group.groupSecret)
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(8.dp))
                                    .background(
                                        if (isPinned) MaterialTheme.colorScheme.primaryContainer
                                        else Color.Transparent
                                    )
                                    .clickable {
                                        if (isPinned) onUnpinEpoch() else onPinEpoch(epoch)
                                    }
                                    .padding(horizontal = 12.dp, vertical = 8.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Icon(
                                    if (isRekeyBoundary) Icons.Filled.Lock else Icons.Filled.Circle,
                                    contentDescription = null,
                                    tint = if (isRekeyBoundary) Color(0xFF2E7D32) else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f),
                                    modifier = Modifier.size(16.dp)
                                )
                                Spacer(modifier = Modifier.width(8.dp))
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(
                                        "Epoch $epoch",
                                        style = MaterialTheme.typography.bodyMedium
                                    )
                                    Text(
                                        snapshot.changeDescription,
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant
                                    )
                                    Text(
                                        "${snapshot.members.size} members" +
                                            if (isRekeyBoundary) " \u2022 private branch" else "",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = if (isRekeyBoundary) Color(0xFF2E7D32) else MaterialTheme.colorScheme.onSurfaceVariant
                                    )
                                }
                                if (isPinned) {
                                    Icon(
                                        Icons.Filled.Check,
                                        contentDescription = "Pinned",
                                        tint = MaterialTheme.colorScheme.primary,
                                        modifier = Modifier.size(20.dp)
                                    )
                                }
                            }
                        }

                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            "Switch to a previous epoch to communicate with members who were present at that point. " +
                                "Epochs marked with \uD83D\uDD12 use a different encryption key \u2014 only members from that epoch can read messages there.",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }
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
