package chat.onym.android.ui.screens

import android.content.ClipboardManager
import android.content.Context
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
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
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import chat.onym.android.model.ChatGroup
import chat.onym.android.onchain.OnChainVerificationResult
import chat.onym.android.ui.components.QRScannerView
import chat.onym.android.ui.components.VerificationBadge
import chat.onym.android.viewmodel.GroupListViewModel
import chat.onym.android.viewmodel.JoinGroupViewModel

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun JoinGroupScreen(
    viewModel: JoinGroupViewModel,
    groupListViewModel: GroupListViewModel? = null,
    onBack: () -> Unit,
    onGroupJoined: (ChatGroup) -> Unit
) {
    val context = LocalContext.current
    var showScanner by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        if (viewModel.inviteText.isBlank()) {
            val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            val clip = clipboard.primaryClip
            if (clip != null && clip.itemCount > 0) {
                val text = clip.getItemAt(0).text?.toString()?.trim() ?: ""
                if (text.length > 100) {
                    try {
                        android.util.Base64.decode(text, android.util.Base64.NO_WRAP)
                        viewModel.inviteText = text
                    } catch (_: Exception) { }
                }
            }
        }
    }

    // Trigger on-chain verification when a group is decoded
    LaunchedEffect(viewModel.decodedGroup) {
        val group = viewModel.decodedGroup ?: return@LaunchedEffect
        if (groupListViewModel?.isContractConfigured != true) return@LaunchedEffect
        viewModel.isVerifying = true
        groupListViewModel.verifyGroupOnChain(group) { result ->
            viewModel.verificationResult = result
            viewModel.isVerifying = false
        }
    }

    if (showScanner) {
        Box(modifier = Modifier.fillMaxSize()) {
            QRScannerView { scannedCode ->
                viewModel.inviteText = scannedCode
                showScanner = false
            }
        }
        return
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Join Group") },
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
        ) {
            Text(
                "Paste or scan a group invitation code shared by an existing group member.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )

            Spacer(modifier = Modifier.height(12.dp))

            OutlinedTextField(
                value = viewModel.inviteText,
                onValueChange = {
                    viewModel.inviteText = it
                },
                label = { Text("Invite Code") },
                modifier = Modifier
                    .fillMaxWidth()
                    .height(120.dp),
                maxLines = 5
            )

            Spacer(modifier = Modifier.height(8.dp))

            OutlinedButton(
                onClick = {
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    val clip = clipboard.primaryClip
                    if (clip != null && clip.itemCount > 0) {
                        viewModel.inviteText = clip.getItemAt(0).text?.toString() ?: ""
                    }
                },
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Paste from Clipboard")
            }

            Spacer(modifier = Modifier.height(8.dp))

            OutlinedButton(
                onClick = { showScanner = true },
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Scan QR Code")
            }

            Spacer(modifier = Modifier.height(16.dp))

            // Group preview card
            viewModel.decodedGroup?.let { group ->
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text(group.name, style = MaterialTheme.typography.titleMedium)
                        Spacer(modifier = Modifier.height(4.dp))
                        Text(
                            "Members: ${group.members.size} | Epoch: ${group.epoch}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )

                        val result = viewModel.verificationResult
                        if (result != null) {
                            Spacer(modifier = Modifier.height(6.dp))
                            VerificationBadge(result)
                            if (result is OnChainVerificationResult.Inactive) {
                                Text(
                                    "This group has been deactivated on-chain and cannot be joined.",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.error,
                                    modifier = Modifier.padding(top = 4.dp)
                                )
                            }
                        } else if (viewModel.isVerifying) {
                            Spacer(modifier = Modifier.height(6.dp))
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                CircularProgressIndicator(modifier = Modifier.size(12.dp), strokeWidth = 1.5.dp)
                                Spacer(modifier = Modifier.width(6.dp))
                                Text(
                                    "Verifying on-chain...",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant
                                )
                            }
                        }
                    }
                }

                Spacer(modifier = Modifier.height(16.dp))
            }

            if (viewModel.decodedGroup != null) {
                Button(
                    onClick = {
                        viewModel.confirmJoin()
                        viewModel.joinedGroup?.let { onGroupJoined(it) }
                    },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = viewModel.verificationResult !is OnChainVerificationResult.Inactive
                ) {
                    Text("Join Group")
                }
            } else {
                Button(
                    onClick = { viewModel.decodeInvite() },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = viewModel.inviteText.isNotBlank()
                ) {
                    Text("Preview")
                }
            }

            viewModel.error?.let { err ->
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    err,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall
                )
            }
        }
    }
}
