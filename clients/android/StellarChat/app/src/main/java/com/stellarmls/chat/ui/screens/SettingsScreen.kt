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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.stellarmls.chat.crypto.KeyAttestation
import com.stellarmls.chat.crypto.KeyManager
import com.stellarmls.chat.model.toHex
import com.stellarmls.chat.viewmodel.GroupListViewModel

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    viewModel: GroupListViewModel,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    val km = viewModel.keyManager
    var newRelayUrl by remember { mutableStateOf("") }
    var contractEndpointInput by remember { mutableStateOf(viewModel.contractEndpoint) }
    var contractIDInput by remember { mutableStateOf(viewModel.contractID) }
    var contractSaveStatus by remember { mutableStateOf<String?>(null) }
    var attestationStatus by remember { mutableStateOf<String?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Settings") },
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
            // Nostr Identity
            SettingsCard("Nostr Identity (secp256k1)") {
                CopyableField("Public Key", km.publicKeyHex, context)
            }

            Spacer(modifier = Modifier.height(12.dp))

            // Inbox Key
            SettingsCard("Inbox Key (X25519)") {
                CopyableField("Inbox Key", km.keyAgreementPublicKeyHex, context)
                Text(
                    "Share this key so others can send you invitations",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 4.dp)
                )
            }

            Spacer(modifier = Modifier.height(12.dp))

            // Stellar Identity
            SettingsCard("Stellar Identity (Ed25519)") {
                CopyableField("Account ID", km.stellarAccountID, context)
                Text(
                    "Derived from Nostr key via HKDF",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 4.dp)
                )
            }

            Spacer(modifier = Modifier.height(12.dp))

            // BLS Membership
            SettingsCard("Group Membership (BLS12-381)") {
                Text("BLS Public Key", style = MaterialTheme.typography.labelMedium)
                Text(
                    km.blsPublicKey().toHex(),
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 2
                )
                Spacer(modifier = Modifier.height(8.dp))
                OutlinedButton(onClick = {
                    try {
                        val attestation = km.createAttestation()
                        val valid = KeyAttestation.verify(attestation)
                        attestationStatus = if (valid) "Attestation created and verified" else "Verification failed"
                    } catch (e: Exception) {
                        attestationStatus = "Error: ${e.message}"
                    }
                }) {
                    Text("Create Key Attestation")
                }
                attestationStatus?.let {
                    Text(it, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(top = 4.dp))
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            // Relay Management
            SettingsCard("Relays") {
                viewModel.relayURLs.forEachIndexed { index, url ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Text(url, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
                        IconButton(onClick = { viewModel.removeRelay(index) }) {
                            Icon(Icons.Default.Delete, contentDescription = "Remove", tint = MaterialTheme.colorScheme.error)
                        }
                    }
                }
                Spacer(modifier = Modifier.height(4.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    OutlinedTextField(
                        value = newRelayUrl,
                        onValueChange = { newRelayUrl = it },
                        label = { Text("wss://...") },
                        modifier = Modifier.weight(1f),
                        singleLine = true
                    )
                    Spacer(modifier = Modifier.width(8.dp))
                    Button(
                        onClick = {
                            if (viewModel.addRelay(newRelayUrl)) {
                                newRelayUrl = ""
                            }
                        },
                        enabled = newRelayUrl.isNotBlank()
                    ) {
                        Text("Add")
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            // Stellar Contract
            SettingsCard("Stellar Contract") {
                Text(
                    if (viewModel.isContractConfigured) "Connected" else "Not configured",
                    style = MaterialTheme.typography.labelMedium,
                    color = if (viewModel.isContractConfigured) MaterialTheme.colorScheme.primary
                    else MaterialTheme.colorScheme.onSurfaceVariant
                )
                Spacer(modifier = Modifier.height(8.dp))
                OutlinedTextField(
                    value = contractEndpointInput,
                    onValueChange = { contractEndpointInput = it },
                    label = { Text("Endpoint URL") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true
                )
                Spacer(modifier = Modifier.height(4.dp))
                OutlinedTextField(
                    value = contractIDInput,
                    onValueChange = { contractIDInput = it },
                    label = { Text("Contract ID") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true
                )
                Spacer(modifier = Modifier.height(8.dp))
                Button(
                    onClick = {
                        viewModel.saveContractConfig(contractEndpointInput, contractIDInput)
                        contractSaveStatus = "Saved"
                    },
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("Save")
                }
                contractSaveStatus?.let {
                    Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            // Protocol Info
            SettingsCard("Protocol") {
                SettingsRow("Transport", "Nostr (NIP-01)")
                SettingsRow("Invitation Kind", "24113")
                SettingsRow("Message Kind", "24114")
                SettingsRow("Encryption", "AES-256-GCM")
                SettingsRow("Invitation Encryption", "X25519 ECDH + AES-256-GCM")
                SettingsRow("Key Derivation", "HKDF-SHA256")
                SettingsRow("Signing", "secp256k1 Schnorr")
                SettingsRow("Stellar Signing", "Ed25519")
                SettingsRow("ZK Backend", "Groth16 / BLS12-381")
                SettingsRow("Commitment", "Poseidon Merkle + SHA256")
            }

            Spacer(modifier = Modifier.height(16.dp))
        }
    }
}

@Composable
private fun SettingsCard(title: String, content: @Composable () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium)
            Spacer(modifier = Modifier.height(8.dp))
            content()
        }
    }
}

@Composable
private fun CopyableField(label: String, value: String, context: Context) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Column(modifier = Modifier.weight(1f)) {
            Text(label, style = MaterialTheme.typography.labelMedium)
            Text(value, style = MaterialTheme.typography.bodySmall, maxLines = 2)
        }
        IconButton(onClick = {
            val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            clipboard.setPrimaryClip(ClipData.newPlainText(label, value))
            Toast.makeText(context, "Copied", Toast.LENGTH_SHORT).show()
        }) {
            Icon(Icons.Default.ContentCopy, contentDescription = "Copy")
        }
    }
}

@Composable
private fun SettingsRow(label: String, value: String) {
    Column(modifier = Modifier.padding(vertical = 2.dp)) {
        Text(label, style = MaterialTheme.typography.labelMedium)
        Text(value, style = MaterialTheme.typography.bodyMedium)
    }
}
