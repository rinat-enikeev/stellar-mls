package com.stellarmls.chat.ui.screens

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
import androidx.activity.compose.BackHandler
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
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Close
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.stellarmls.chat.nostr.InvitationTransport
import com.stellarmls.chat.ui.components.QRCodeImage
import com.stellarmls.chat.ui.components.QRScannerView
import com.stellarmls.chat.viewmodel.CreationPhase
import com.stellarmls.chat.viewmodel.CreateGroupViewModel
import com.stellarmls.chat.viewmodel.GroupListViewModel
import com.stellarmls.chat.viewmodel.InvitationStatus
import com.stellarmls.chat.viewmodel.OnChainPublishStatus

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CreateGroupScreen(
    viewModel: CreateGroupViewModel,
    keyManager: com.stellarmls.chat.crypto.KeyManager,
    groupListViewModel: GroupListViewModel,
    invitationTransport: InvitationTransport,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    var showScanner by remember { mutableStateOf(false) }

    val isProcessing = viewModel.phase == CreationPhase.CREATING ||
        viewModel.phase == CreationPhase.PUBLISHING ||
        viewModel.phase == CreationPhase.INVITING

    // Block system back during processing
    BackHandler(enabled = isProcessing) { /* blocked */ }

    if (showScanner) {
        androidx.compose.foundation.layout.Box(modifier = Modifier.fillMaxSize()) {
            QRScannerView { scannedCode ->
                val trimmed = scannedCode.trim()
                if (trimmed.length == 64 && trimmed.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }) {
                    viewModel.keyInput = trimmed
                }
                showScanner = false
            }
        }
        return
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Create Group") },
                navigationIcon = {
                    if (viewModel.phase == CreationPhase.INPUT || viewModel.phase == CreationPhase.DONE) {
                        IconButton(onClick = onBack) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                        }
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
            when (viewModel.phase) {
                CreationPhase.INPUT -> InputContent(
                    viewModel = viewModel,
                    keyManager = keyManager,
                    groupListViewModel = groupListViewModel,
                    invitationTransport = invitationTransport,
                    onShowScanner = { showScanner = true },
                    context = context
                )
                CreationPhase.CREATING, CreationPhase.PUBLISHING, CreationPhase.INVITING ->
                    ProgressContent(
                        viewModel = viewModel,
                        groupListViewModel = groupListViewModel,
                        invitationTransport = invitationTransport,
                        keyManager = keyManager
                    )
                CreationPhase.DONE -> DoneContent(
                    viewModel = viewModel,
                    groupListViewModel = groupListViewModel,
                    context = context
                )
            }

            viewModel.errorMessage?.let { error ->
                Spacer(modifier = Modifier.height(12.dp))
                Text(
                    error,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall
                )
            }
        }
    }
}

@Composable
private fun InputContent(
    viewModel: CreateGroupViewModel,
    keyManager: com.stellarmls.chat.crypto.KeyManager,
    groupListViewModel: GroupListViewModel,
    invitationTransport: InvitationTransport,
    onShowScanner: () -> Unit,
    context: Context
) {
    OutlinedTextField(
        value = viewModel.groupName,
        onValueChange = { viewModel.groupName = it },
        label = { Text("Group Name") },
        modifier = Modifier.fillMaxWidth(),
        singleLine = true
    )

    Spacer(modifier = Modifier.height(16.dp))

    // Participants section
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(
                "Participants (${viewModel.participantKeys.size})",
                style = MaterialTheme.typography.titleMedium
            )

            Spacer(modifier = Modifier.height(8.dp))

            // Added participants list
            for (key in viewModel.participantKeys) {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        key.take(8) + "..." + key.takeLast(8),
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.weight(1f)
                    )
                    IconButton(
                        onClick = { viewModel.removeParticipant(key) },
                        modifier = Modifier.size(24.dp)
                    ) {
                        Icon(
                            Icons.Default.Close,
                            contentDescription = "Remove",
                            modifier = Modifier.size(16.dp)
                        )
                    }
                }
            }

            Spacer(modifier = Modifier.height(8.dp))

            OutlinedTextField(
                value = viewModel.keyInput,
                onValueChange = { viewModel.keyInput = it; viewModel.keyValidationError = null },
                label = { Text("Inbox key (64 hex characters)") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                textStyle = MaterialTheme.typography.bodySmall
            )

            Spacer(modifier = Modifier.height(8.dp))

            Row(modifier = Modifier.fillMaxWidth()) {
                OutlinedButton(
                    onClick = {
                        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                        val clip = clipboard.primaryClip
                        if (clip != null && clip.itemCount > 0) {
                            viewModel.keyInput = clip.getItemAt(0).text?.toString()?.trim() ?: ""
                        }
                    },
                    modifier = Modifier.weight(1f)
                ) {
                    Text("Paste", style = MaterialTheme.typography.bodySmall)
                }

                Spacer(modifier = Modifier.width(8.dp))

                OutlinedButton(
                    onClick = onShowScanner,
                    modifier = Modifier.weight(1f)
                ) {
                    Text("Scan QR", style = MaterialTheme.typography.bodySmall)
                }

                Spacer(modifier = Modifier.width(8.dp))

                Button(
                    onClick = { viewModel.addParticipant(keyManager.keyAgreementPublicKeyHex) },
                    enabled = viewModel.keyInput.trim().length == 64
                ) {
                    Text("Add", style = MaterialTheme.typography.bodySmall)
                }
            }

            viewModel.keyValidationError?.let { error ->
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    error,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall
                )
            }

            Spacer(modifier = Modifier.height(4.dp))
            Text(
                "Enter each participant's inbox key (found in their Settings \u2192 Advanced). Participants are optional.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }

    Spacer(modifier = Modifier.height(16.dp))

    Button(
        onClick = {
            viewModel.startCreation(
                keyManager,
                groupListViewModel.defaultGroupTier,
                groupListViewModel,
                invitationTransport
            )
        },
        modifier = Modifier.fillMaxWidth(),
        enabled = viewModel.groupName.isNotBlank()
    ) {
        Text("Create Group")
    }
}

@Composable
private fun ProgressContent(
    viewModel: CreateGroupViewModel,
    groupListViewModel: GroupListViewModel,
    invitationTransport: InvitationTransport,
    keyManager: com.stellarmls.chat.crypto.KeyManager
) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text("Creating Group...", style = MaterialTheme.typography.titleMedium)
            Spacer(modifier = Modifier.height(12.dp))

            // Step 1: Creating group
            StatusRow(
                label = "Creating group...",
                isDone = viewModel.phase != CreationPhase.CREATING,
                isActive = viewModel.phase == CreationPhase.CREATING
            )

            // Step 2: On-chain (if configured)
            if (groupListViewModel.isContractConfigured) {
                Spacer(modifier = Modifier.height(8.dp))
                when (val status = viewModel.onChainStatus) {
                    is OnChainPublishStatus.Idle -> {}
                    is OnChainPublishStatus.Publishing ->
                        StatusRow(label = "Publishing to Stellar blockchain...", isDone = false, isActive = true)
                    is OnChainPublishStatus.Published ->
                        StatusRow(label = "Published on-chain", isDone = true, isActive = false)
                    is OnChainPublishStatus.Failed -> {
                        StatusRow(label = "On-chain publication failed", isDone = false, isActive = false, isFailed = true)
                        Text(
                            status.reason,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(start = 28.dp)
                        )
                        Row(modifier = Modifier.padding(start = 28.dp, top = 4.dp)) {
                            TextButton(onClick = {
                                viewModel.retryOnChainPublish(groupListViewModel, invitationTransport, keyManager)
                            }) {
                                Text("Retry")
                            }
                            Spacer(modifier = Modifier.width(8.dp))
                            TextButton(onClick = {
                                viewModel.skipOnChainAndContinue(invitationTransport, keyManager)
                            }) {
                                Text("Skip", color = Color(0xFFFF9800))
                            }
                        }
                    }
                }
            }

            // Step 3: Invitations
            if (viewModel.participantKeys.isNotEmpty()) {
                Spacer(modifier = Modifier.height(8.dp))
                for (key in viewModel.participantKeys) {
                    val displayKey = key.take(8) + "..." + key.takeLast(8)
                    Spacer(modifier = Modifier.height(4.dp))
                    when (val status = viewModel.invitationStatuses[key]) {
                        is InvitationStatus.Sending ->
                            StatusRow(label = "Inviting $displayKey", isDone = false, isActive = true)
                        is InvitationStatus.Sent ->
                            StatusRow(label = "Invited $displayKey", isDone = true, isActive = false)
                        is InvitationStatus.Failed ->
                            StatusRow(label = "Failed: $displayKey", isDone = false, isActive = false, isFailed = true)
                        null ->
                            StatusRow(label = "Pending: $displayKey", isDone = false, isActive = false)
                    }
                }
            }
        }
    }
}

@Composable
private fun DoneContent(
    viewModel: CreateGroupViewModel,
    groupListViewModel: GroupListViewModel,
    context: Context
) {
    viewModel.inviteCode?.let { code ->
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(
                modifier = Modifier.padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text("QR Code", style = MaterialTheme.typography.titleMedium)
                Spacer(modifier = Modifier.height(8.dp))
                QRCodeImage(text = code, size = 200.dp)
            }
        }

        Spacer(modifier = Modifier.height(12.dp))

        Card(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text("Invite Code", style = MaterialTheme.typography.titleMedium)
                Spacer(modifier = Modifier.height(8.dp))
                Text(code, style = MaterialTheme.typography.bodySmall, maxLines = 4)
                Spacer(modifier = Modifier.height(8.dp))
                OutlinedButton(
                    onClick = {
                        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                        clipboard.setPrimaryClip(ClipData.newPlainText("Invite Code", code))
                        Toast.makeText(context, "Copied to clipboard", Toast.LENGTH_SHORT).show()
                    },
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("Copy to Clipboard")
                }
            }
        }
    }

    // Invitation results
    if (viewModel.invitationStatuses.isNotEmpty()) {
        Spacer(modifier = Modifier.height(12.dp))
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text("Invitation Results", style = MaterialTheme.typography.titleMedium)
                Spacer(modifier = Modifier.height(8.dp))

                val sentCount = viewModel.invitationStatuses.values.count { it is InvitationStatus.Sent }
                val failedCount = viewModel.invitationStatuses.values.count { it is InvitationStatus.Failed }

                if (sentCount > 0) {
                    Text(
                        "\u2713 $sentCount invitation${if (sentCount == 1) "" else "s"} sent",
                        style = MaterialTheme.typography.bodySmall,
                        color = Color(0xFF4CAF50)
                    )
                }
                if (failedCount > 0) {
                    Text(
                        "\u26A0 $failedCount invitation${if (failedCount == 1) "" else "s"} failed \u2014 invite from group settings",
                        style = MaterialTheme.typography.bodySmall,
                        color = Color(0xFFFF9800)
                    )
                }
            }
        }
    }

    // On-chain status
    if (groupListViewModel.isContractConfigured) {
        Spacer(modifier = Modifier.height(12.dp))
        Card(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text("On-Chain Status", style = MaterialTheme.typography.titleMedium)
                Spacer(modifier = Modifier.height(8.dp))
                when (viewModel.onChainStatus) {
                    is OnChainPublishStatus.Published -> {
                        Text(
                            "\u2713 Published on-chain",
                            style = MaterialTheme.typography.bodySmall,
                            color = Color(0xFF4CAF50)
                        )
                    }
                    is OnChainPublishStatus.Failed -> {
                        Text(
                            "\u26A0 Not published on-chain",
                            style = MaterialTheme.typography.bodySmall,
                            color = Color(0xFFFF9800)
                        )
                    }
                    else -> {}
                }
            }
        }
    }
}

@Composable
private fun StatusRow(
    label: String,
    isDone: Boolean,
    isActive: Boolean,
    isFailed: Boolean = false
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        if (isDone) {
            Text("\u2713", color = Color(0xFF4CAF50), style = MaterialTheme.typography.bodyMedium)
            Spacer(modifier = Modifier.width(8.dp))
        } else if (isActive) {
            CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
            Spacer(modifier = Modifier.width(8.dp))
        } else if (isFailed) {
            Text("\u2717", color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
            Spacer(modifier = Modifier.width(8.dp))
        } else {
            Text("\u25CB", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            Spacer(modifier = Modifier.width(8.dp))
        }
        Text(
            label,
            style = MaterialTheme.typography.bodySmall,
            color = when {
                isFailed -> MaterialTheme.colorScheme.error
                isDone -> MaterialTheme.colorScheme.onSurface
                else -> MaterialTheme.colorScheme.onSurfaceVariant
            }
        )
    }
}
