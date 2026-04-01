package com.stellarmls.chat.ui.screens

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.viewmodel.CreateGroupViewModel
import com.stellarmls.chat.viewmodel.GroupListViewModel

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CreateGroupScreen(
    viewModel: CreateGroupViewModel,
    keyManager: com.stellarmls.chat.crypto.KeyManager,
    groupListViewModel: GroupListViewModel? = null,
    onBack: () -> Unit,
    onGroupCreated: (ChatGroup) -> Unit
) {
    val context = LocalContext.current
    var onChainStatus by remember { mutableStateOf<OnChainPublishStatus>(OnChainPublishStatus.Idle) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Create Group") },
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
            OutlinedTextField(
                value = viewModel.groupName,
                onValueChange = { viewModel.groupName = it },
                label = { Text("Group Name") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )

            Spacer(modifier = Modifier.height(16.dp))

            Button(
                onClick = {
                    // M-15: Sanitize group name before creation
                    val sanitized = sanitizeGroupName(viewModel.groupName) ?: return@Button
                    viewModel.groupName = sanitized
                    viewModel.createGroup(keyManager)
                    viewModel.createdGroup?.let { group ->
                        onGroupCreated(group)
                        // Auto-publish on-chain if configured
                        if (groupListViewModel?.isContractConfigured == true) {
                            onChainStatus = OnChainPublishStatus.Publishing
                            groupListViewModel.publishGroupOnChain(group) { result ->
                                onChainStatus = result.fold(
                                    onSuccess = { OnChainPublishStatus.Published },
                                    onFailure = { OnChainPublishStatus.Failed(it.message ?: "Unknown error") }
                                )
                            }
                        }
                    }
                },
                modifier = Modifier.fillMaxWidth(),
                enabled = viewModel.groupName.isNotBlank()
            ) {
                Text("Create Group")
            }

            viewModel.inviteCode?.let { code ->
                Spacer(modifier = Modifier.height(24.dp))

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

                // On-chain status
                if (groupListViewModel?.isContractConfigured == true) {
                    Spacer(modifier = Modifier.height(12.dp))
                    Card(modifier = Modifier.fillMaxWidth()) {
                        Column(modifier = Modifier.padding(16.dp)) {
                            Text("On-Chain Status", style = MaterialTheme.typography.titleMedium)
                            Spacer(modifier = Modifier.height(8.dp))
                            when (val status = onChainStatus) {
                                is OnChainPublishStatus.Idle -> {}
                                is OnChainPublishStatus.Publishing -> {
                                    Row(verticalAlignment = Alignment.CenterVertically) {
                                        CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                                        Spacer(modifier = Modifier.width(8.dp))
                                        Text(
                                            "Publishing to Stellar testnet...",
                                            style = MaterialTheme.typography.bodySmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant
                                        )
                                    }
                                }
                                is OnChainPublishStatus.Published -> {
                                    Text(
                                        "\u2713 Published on-chain",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = Color(0xFF4CAF50)
                                    )
                                }
                                is OnChainPublishStatus.Failed -> {
                                    Text(
                                        "Publication failed: ${status.reason}",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.error
                                    )
                                    Spacer(modifier = Modifier.height(4.dp))
                                    OutlinedButton(onClick = {
                                        viewModel.createdGroup?.let { group ->
                                            onChainStatus = OnChainPublishStatus.Publishing
                                            groupListViewModel.publishGroupOnChain(group) { result ->
                                                onChainStatus = result.fold(
                                                    onSuccess = { OnChainPublishStatus.Published },
                                                    onFailure = { OnChainPublishStatus.Failed(it.message ?: "Unknown error") }
                                                )
                                            }
                                        }
                                    }) {
                                        Text("Retry")
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

private sealed class OnChainPublishStatus {
    object Idle : OnChainPublishStatus()
    object Publishing : OnChainPublishStatus()
    object Published : OnChainPublishStatus()
    data class Failed(val reason: String) : OnChainPublishStatus()
}

/** M-15: Sanitize group name — strip control characters and enforce length limit. */
internal fun sanitizeGroupName(name: String): String? {
    val trimmed = name.trim()
    // Remove Unicode control characters and zero-width characters
    val sanitized = trimmed.filter { ch ->
        !ch.isISOControl() && ch.category != CharCategory.FORMAT
    }
    return if (sanitized.isNotEmpty() && sanitized.length <= 100) sanitized else null
}
