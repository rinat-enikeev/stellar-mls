package com.stellarmls.chat.ui.screens

import android.content.ClipboardManager
import android.content.Context
import android.util.Log
import android.widget.Toast
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.Card
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
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.stellarmls.chat.BuildConfig
import com.stellarmls.chat.model.BootstrapPayload
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.hexToBytes
import com.stellarmls.chat.nostr.InvitationTransport
import com.stellarmls.chat.ui.components.QRScannerView
import com.stellarmls.mls.SEPCommitmentBuilder
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun InviteMemberScreen(
    group: ChatGroup,
    invitationTransport: InvitationTransport,
    keyManager: com.stellarmls.chat.crypto.KeyManager,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var recipientKey by remember { mutableStateOf("") }
    var status by remember { mutableStateOf<String?>(null) }
    var isSending by remember { mutableStateOf(false) }
    var showScanner by remember { mutableStateOf(false) }

    if (showScanner) {
        androidx.compose.foundation.layout.Box(modifier = Modifier.fillMaxSize()) {
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
                title = { Text("Invite Member") },
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
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Group: ${group.name}", style = MaterialTheme.typography.titleMedium)
                    Text("Members: ${group.members.size}", style = MaterialTheme.typography.bodySmall)
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            OutlinedTextField(
                value = recipientKey,
                onValueChange = { recipientKey = it; status = null },
                label = { Text("Recipient X25519 Inbox Key (64 hex chars)") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true
            )

            Spacer(modifier = Modifier.height(8.dp))

            OutlinedButton(
                onClick = {
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    val clip = clipboard.primaryClip
                    if (clip != null && clip.itemCount > 0) {
                        recipientKey = clip.getItemAt(0).text?.toString() ?: ""
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
                Text("Scan Inbox Key QR")
            }

            Spacer(modifier = Modifier.height(16.dp))

            Button(
                onClick = {
                    isSending = true
                    status = null
                    scope.launch {
                        try {
                            val pubkeyBytes = recipientKey.trim().hexToBytes()
                            require(pubkeyBytes.size == 32) { "Key must be 32 bytes (64 hex chars)" }
                            val payload = BootstrapPayload.from(group, keyManager.publicKeyHex)
                            if (BuildConfig.DEBUG) {
                                val root = SEPCommitmentBuilder.computeMerkleRoot(group.members, group.tier)
                                val poseidon = SEPCommitmentBuilder.computePoseidonCommitment(root, group.epoch, group.salt)
                                val firstMember = group.members.firstOrNull()
                                Log.d(
                                    "InviteMember",
                                    "sendInvitation group=${group.groupIDData.debugHexPrefix(12)} " +
                                        "epoch=${group.epoch} tier=${group.tier.id} members=${group.members.size} " +
                                        "salt=${group.salt.debugHexPrefix(12)} " +
                                        "payloadCommitment=${payload.commitment?.debugHexPrefix(12) ?: "none"} " +
                                        "poseidon=${poseidon.debugHexPrefix(12)} " +
                                        "firstPk=${firstMember?.publicKeyCompressed?.debugHexPrefix(12) ?: "none"} " +
                                        "firstLeaf=${firstMember?.leafHash?.debugHexPrefix(12) ?: "none"}"
                                )
                            }
                            withContext(Dispatchers.IO) {
                                if (BuildConfig.DEBUG) {
                                    val recipientInboxTag = com.stellarmls.chat.crypto.GroupCrypto.hiddenInboxTag(pubkeyBytes)
                                    Log.d(
                                        "InviteMember",
                                        "recipientPubkey=${pubkeyBytes.debugHexPrefix(12)} recipientInboxTag=$recipientInboxTag"
                                    )
                                }
                                invitationTransport.sendInvitation(payload, pubkeyBytes, keyManager)
                            }
                            status = "Invitation sent!"
                            Toast.makeText(context, "Invitation sent", Toast.LENGTH_SHORT).show()
                        } catch (e: Exception) {
                            status = "Error: ${e.message}"
                        } finally {
                            isSending = false
                        }
                    }
                },
                modifier = Modifier.fillMaxWidth(),
                enabled = recipientKey.length == 64 && !isSending
            ) {
                Text(if (isSending) "Sending..." else "Send Invitation")
            }

            status?.let { s ->
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    s,
                    color = if (s.startsWith("Error")) MaterialTheme.colorScheme.error
                    else MaterialTheme.colorScheme.primary,
                    style = MaterialTheme.typography.bodySmall
                )
            }
        }
    }
}

private fun ByteArray.debugHexPrefix(bytes: Int = 8): String {
    val count = minOf(bytes, size)
    return take(count).joinToString("") { "%02x".format(it) }
}
