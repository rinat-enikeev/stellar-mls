package chat.onym.android.ui.screens

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.Language
import androidx.compose.material.icons.filled.Public
import androidx.compose.material.icons.filled.QuestionMark
import androidx.compose.material.icons.filled.Router
import androidx.compose.material.icons.filled.SettingsEthernet
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.text.KeyboardOptions
import chat.onym.android.crypto.KeyAttestation
import chat.onym.android.model.toHex
import chat.onym.android.ui.components.SettingsIconBox
import chat.onym.android.ui.components.SettingsIconColors
import chat.onym.android.viewmodel.GroupListViewModel

// MARK: - Relays

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RelaysScreen(
    viewModel: GroupListViewModel,
    onBack: () -> Unit
) {
    var newUrl by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }

    SubScreenScaffold("Relays", onBack) {
        if (viewModel.relayURLs.isNotEmpty()) {
            SummaryCard(
                iconBg = Brush.linearGradient(
                    listOf(SettingsIconColors.Green, SettingsIconColors.Blue)
                ),
                icon = Icons.Default.Router,
                title = "${viewModel.relayURLs.size} relay${if (viewModel.relayURLs.size == 1) "" else "s"} configured",
                subtitle = "First relay is tried first",
                badge = "Healthy",
                badgeColor = SettingsIconColors.Green
            )

            Spacer(Modifier.height(16.dp))
            SectionHeaderSm("MY RELAYS")
            GroupedCard {
                viewModel.relayURLs.forEachIndexed { index, url ->
                    RelayRow(
                        url = url,
                        isPrimary = index == 0,
                        onRemove = { viewModel.removeRelay(index) },
                        separator = index < viewModel.relayURLs.size - 1
                    )
                }
            }
            Spacer(Modifier.height(16.dp))
        }

        SectionHeaderSm("ADD RELAY")
        GroupedCard {
            Row(
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                OutlinedTextField(
                    value = newUrl,
                    onValueChange = { newUrl = it; error = null },
                    placeholder = { Text("wss://relay.example.com") },
                    modifier = Modifier.weight(1f),
                    singleLine = true,
                    textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrect = false)
                )
                if (newUrl.trim().isNotEmpty()) {
                    Spacer(Modifier.width(8.dp))
                    Button(
                        onClick = {
                            if (viewModel.addRelay(newUrl.trim())) {
                                newUrl = ""; error = null
                            } else error = "Invalid URL. Must be ws:// or wss:// and not already added."
                        }
                    ) { Text("Add") }
                }
            }
        }
        error?.let { FooterNote(it, error = true) }
        FooterNote("Swipe to remove. Your primary relay is tried first.")

        if (viewModel.relayURLs.isEmpty()) {
            Spacer(Modifier.height(24.dp))
            EmptyState(
                icon = Icons.Default.Router,
                title = "No relays yet",
                subtitle = "Add a relay to start sending and receiving messages."
            )
        }
    }
}

@Composable
private fun RelayRow(url: String, isPrimary: Boolean, onRemove: () -> Unit, separator: Boolean) {
    Column {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Box(
                Modifier
                    .size(8.dp)
                    .shadow(3.dp, CircleShape)
                    .background(SettingsIconColors.Green, CircleShape)
            )
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    url,
                    style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                    maxLines = 1
                )
                if (isPrimary) {
                    Spacer(Modifier.height(2.dp))
                    Text(
                        "PRIMARY",
                        style = MaterialTheme.typography.labelSmall.copy(
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.primary
                        ),
                        modifier = Modifier
                            .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.15f), RoundedCornerShape(4.dp))
                            .padding(horizontal = 6.dp, vertical = 1.dp)
                    )
                }
            }
            IconButton(onClick = onRemove) {
                Icon(Icons.Default.Delete, contentDescription = "Remove", tint = MaterialTheme.colorScheme.error)
            }
        }
        if (separator) RowSeparator(start = 36.dp)
    }
}

// MARK: - Blossom Servers

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BlossomServersScreen(
    viewModel: GroupListViewModel,
    onBack: () -> Unit
) {
    var newUrl by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }

    SubScreenScaffold("Blossom Servers", onBack) {
        if (viewModel.blossomServerURLs.isNotEmpty()) {
            SectionHeaderSm("MY SERVERS")
            GroupedCard {
                viewModel.blossomServerURLs.forEachIndexed { index, url ->
                    BlossomRow(
                        url = url,
                        isPrimary = index == 0,
                        onRemove = { viewModel.removeBlossomServer(index) },
                        separator = index < viewModel.blossomServerURLs.size - 1
                    )
                }
            }
            Spacer(Modifier.height(16.dp))
        }

        SectionHeaderSm("ADD SERVER")
        GroupedCard {
            Row(
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                OutlinedTextField(
                    value = newUrl,
                    onValueChange = { newUrl = it; error = null },
                    placeholder = { Text("https://blossom.example.com") },
                    modifier = Modifier.weight(1f),
                    singleLine = true,
                    textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrect = false)
                )
                if (newUrl.trim().isNotEmpty()) {
                    Spacer(Modifier.width(8.dp))
                    Button(
                        onClick = {
                            if (viewModel.addBlossomServer(newUrl.trim())) {
                                newUrl = ""; error = null
                            } else error = "Invalid URL. Must be http(s):// and not already added."
                        }
                    ) { Text("Add") }
                }
            }
        }
        error?.let { FooterNote(it, error = true) }
        FooterNote("Encrypted images are stored on Blossom servers. The server never sees plaintext.")

        if (viewModel.blossomServerURLs.isEmpty()) {
            Spacer(Modifier.height(24.dp))
            EmptyState(
                icon = Icons.Default.Image,
                title = "No Blossom servers",
                subtitle = "Add a server URL to store encrypted media."
            )
        }
    }
}

@Composable
private fun BlossomRow(url: String, isPrimary: Boolean, onRemove: () -> Unit, separator: Boolean) {
    Column {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            SettingsIconBox(icon = Icons.Default.Image, background = SettingsIconColors.Purple)
            Spacer(Modifier.width(12.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    url,
                    style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                    maxLines = 1
                )
                if (isPrimary) {
                    Spacer(Modifier.height(2.dp))
                    Text(
                        "PRIMARY",
                        style = MaterialTheme.typography.labelSmall.copy(
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.primary
                        ),
                        modifier = Modifier
                            .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.15f), RoundedCornerShape(4.dp))
                            .padding(horizontal = 6.dp, vertical = 1.dp)
                    )
                }
            }
            IconButton(onClick = onRemove) {
                Icon(Icons.Default.Delete, contentDescription = "Remove", tint = MaterialTheme.colorScheme.error)
            }
        }
        if (separator) RowSeparator(start = 58.dp)
    }
}

// MARK: - TURN Servers

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TurnServersScreen(
    viewModel: GroupListViewModel,
    onBack: () -> Unit
) {
    var newUrl by remember { mutableStateOf("") }
    var usernameInput by remember { mutableStateOf(viewModel.turnUsername) }
    var passwordInput by remember { mutableStateOf(viewModel.turnPassword) }
    var error by remember { mutableStateOf<String?>(null) }

    SubScreenScaffold("TURN Servers", onBack) {
        SectionHeaderSm("BUILT-IN")
        GroupedCard {
            Row(
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                SettingsIconBox(icon = Icons.Default.Public, background = SettingsIconColors.Cyan)
                Spacer(Modifier.width(12.dp))
                Column(modifier = Modifier.weight(1f)) {
                    Text("EU TURN servers", style = MaterialTheme.typography.bodyLarge)
                    Text("Built-in", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                StatusChip("Active", SettingsIconColors.Green)
            }
        }
        FooterNote("Always included for call relay through restrictive networks.")

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("CUSTOM")
        GroupedCard {
            Row(
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("Enable custom TURN", modifier = Modifier.weight(1f))
                Switch(
                    checked = viewModel.turnEnabled,
                    onCheckedChange = { viewModel.updateTurnEnabled(it) }
                )
            }
        }

        if (viewModel.turnEnabled) {
            Spacer(Modifier.height(16.dp))
            SectionHeaderSm("SERVER URL")
            GroupedCard {
                viewModel.turnURLs.forEachIndexed { index, url ->
                    Column {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 16.dp, vertical = 10.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Icon(
                                Icons.Default.SettingsEthernet,
                                contentDescription = null,
                                tint = SettingsIconColors.Blue,
                                modifier = Modifier.size(20.dp)
                            )
                            Spacer(Modifier.width(10.dp))
                            Text(
                                url,
                                style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
                                modifier = Modifier.weight(1f),
                                maxLines = 1
                            )
                            IconButton(onClick = { viewModel.removeTurnURL(index) }) {
                                Icon(Icons.Default.Delete, contentDescription = "Remove", tint = MaterialTheme.colorScheme.error)
                            }
                        }
                        if (index < viewModel.turnURLs.size - 1) RowSeparator(start = 46.dp)
                    }
                }
                Row(
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    OutlinedTextField(
                        value = newUrl,
                        onValueChange = { newUrl = it; error = null },
                        placeholder = { Text("turn:host:443?transport=tcp") },
                        modifier = Modifier.weight(1f),
                        singleLine = true,
                        textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrect = false)
                    )
                    if (newUrl.trim().isNotEmpty()) {
                        Spacer(Modifier.width(8.dp))
                        Button(
                            onClick = {
                                if (viewModel.addTurnURL(newUrl.trim())) {
                                    newUrl = ""; error = null
                                } else error = "Invalid URL. Must start with turn: or turns: and not already added."
                            }
                        ) { Text("Add") }
                    }
                }
            }
            error?.let { FooterNote(it, error = true) }

            Spacer(Modifier.height(16.dp))
            SectionHeaderSm("CREDENTIALS")
            GroupedCard {
                Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                    OutlinedTextField(
                        value = usernameInput,
                        onValueChange = { usernameInput = it },
                        label = { Text("Username") },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrect = false)
                    )
                    Spacer(Modifier.height(8.dp))
                    OutlinedTextField(
                        value = passwordInput,
                        onValueChange = { passwordInput = it },
                        label = { Text("Password") },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                        visualTransformation = PasswordVisualTransformation()
                    )
                    Spacer(Modifier.height(10.dp))
                    Button(
                        onClick = { viewModel.saveTurnCredentials(usernameInput, passwordInput) },
                        modifier = Modifier.fillMaxWidth()
                    ) { Text("Save credentials") }
                }
            }
            FooterNote("Optional custom TURN servers for restrictive networks. Use turn: or turns: URL schemes.")
        }
    }
}

// MARK: - Stellar Contract

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun StellarContractScreen(
    viewModel: GroupListViewModel,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    var endpoint by remember(viewModel.contractEndpoint) { mutableStateOf(viewModel.contractEndpoint) }
    var contractId by remember(viewModel.contractID) { mutableStateOf(viewModel.contractID) }
    var relayerUrl by remember(viewModel.relayerURL) { mutableStateOf(viewModel.relayerURL) }
    var relayerToken by remember(viewModel.relayerAuthToken) { mutableStateOf(viewModel.relayerAuthToken) }
    var saveStatus by remember { mutableStateOf<String?>(null) }

    SubScreenScaffold("Stellar Contract", onBack) {
        // Status banner
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(14.dp))
                .padding(horizontal = 16.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Box(
                Modifier
                    .size(38.dp)
                    .background(
                        if (viewModel.isContractConfigured) SettingsIconColors.Green else SettingsIconColors.Gray,
                        CircleShape
                    ),
                contentAlignment = Alignment.Center
            ) {
                Icon(
                    if (viewModel.isContractConfigured) Icons.Default.Check else Icons.Default.QuestionMark,
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier.size(18.dp)
                )
            }
            Spacer(Modifier.width(12.dp))
            Column {
                Text(
                    if (viewModel.isContractConfigured) "Connected" else "Not configured",
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = FontWeight.SemiBold
                )
                Text(
                    if (viewModel.relayerURL.isBlank()) "Direct RPC" else "Via relayer",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
        }

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("ENDPOINT")
        GroupedCard {
            Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                LabeledField(label = "ENDPOINT URL", value = endpoint, onChange = { endpoint = it }, placeholder = "https://soroban-testnet.stellar.org")
                Spacer(Modifier.height(8.dp))
                LabeledField(label = "CONTRACT ID", value = contractId, onChange = { contractId = it }, placeholder = "C…")
            }
        }

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("RELAYER (OPTIONAL)")
        GroupedCard {
            Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) {
                LabeledField(label = "RELAYER URL", value = relayerUrl, onChange = { relayerUrl = it }, placeholder = "https://relay.example.com")
                Spacer(Modifier.height(8.dp))
                LabeledField(label = "RELAYER AUTH TOKEN", value = relayerToken, onChange = { relayerToken = it }, placeholder = "Optional token", secure = true)
            }
        }
        FooterNote("If a relayer URL is set, on-chain requests are sent through the relayer instead of directly to Soroban RPC.")

        Spacer(Modifier.height(16.dp))
        Button(
            onClick = {
                viewModel.saveContractConfig(endpoint, contractId)
                viewModel.saveRelayerConfig(relayerUrl, relayerToken)
                saveStatus = if (viewModel.isContractConfigured) "Configured successfully"
                else "Error: Invalid endpoint URL or contract ID"
                Toast.makeText(context, saveStatus, Toast.LENGTH_SHORT).show()
            },
            modifier = Modifier.fillMaxWidth().height(52.dp),
            enabled = endpoint.trim().isNotEmpty() && contractId.trim().isNotEmpty(),
            shape = RoundedCornerShape(14.dp)
        ) {
            Text("Save contract configuration", fontWeight = FontWeight.SemiBold)
        }
        saveStatus?.let {
            Spacer(Modifier.height(6.dp))
            Text(
                it,
                style = MaterialTheme.typography.labelSmall,
                color = if (it.startsWith("Error")) MaterialTheme.colorScheme.error else SettingsIconColors.Green,
                modifier = Modifier.padding(horizontal = 16.dp)
            )
        }

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("DIAGNOSTICS")
        GroupedCard {
            Column {
                KeyValueRow(
                    label = "Status",
                    value = if (viewModel.isContractConfigured) "Connected" else "Not configured",
                    valueColor = if (viewModel.isContractConfigured) SettingsIconColors.Green else MaterialTheme.colorScheme.onSurfaceVariant
                )
                RowSeparator(start = 16.dp)
                KeyValueRow(label = "Network", value = "Testnet")
            }
        }
    }
}

// MARK: - Advanced

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AdvancedSettingsScreen(
    viewModel: GroupListViewModel,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    val km = viewModel.keyManager
    var attestationStatus by remember { mutableStateOf<String?>(null) }

    SubScreenScaffold("Advanced", onBack) {
        SectionHeaderSm("NOSTR IDENTITY")
        GroupedCard {
            Column {
                KeyValueRow(label = "Public key (secp256k1)", value = truncate(km.publicKeyHex), monoValue = true)
                RowSeparator(start = 16.dp)
                CopyRow(context = context, label = "Copy Nostr public key", value = km.publicKeyHex)
            }
        }
        FooterNote("Your Nostr identity key used to sign events on relays.")

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("STELLAR IDENTITY")
        GroupedCard {
            Column {
                KeyValueRow(label = "Account ID (Ed25519)", value = truncate(km.stellarAccountID), monoValue = true)
                RowSeparator(start = 16.dp)
                CopyRow(context = context, label = "Copy Stellar Account ID", value = km.stellarAccountID)
            }
        }
        FooterNote("Derived from Nostr key via HKDF-SHA256. StrKey encoded (G…).")

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("BLS / ATTESTATION")
        GroupedCard {
            Column {
                KeyValueRow(label = "BLS public key (BLS12-381)", value = truncate(km.blsPublicKey().toHex()), monoValue = true)
                RowSeparator(start = 16.dp)
                TextActionRow(label = "Create key attestation") {
                    try {
                        val attestation = km.createAttestation()
                        val valid = KeyAttestation.verify(attestation)
                        val blsHex = attestation.blsPubkey.take(8).joinToString("") { "%02x".format(it) }
                        val edHex = attestation.ed25519Pubkey.take(8).joinToString("") { "%02x".format(it) }
                        attestationStatus = "Bound BLS $blsHex... to Stellar $edHex... (${if (valid) "verified" else "INVALID"})"
                    } catch (e: Exception) {
                        attestationStatus = "Error: ${e.message}"
                    }
                }
                attestationStatus?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.labelSmall,
                        color = if (it.startsWith("Error")) MaterialTheme.colorScheme.error else SettingsIconColors.Green,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp)
                    )
                }
            }
        }
        FooterNote("Ed25519 signature binding BLS group key to Stellar identity per SEP-XXXX §1.1.")

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("PROTOCOL")
        GroupedCard {
            Column {
                ProtocolKv(label = "Invitation kind", value = "34113", separator = true)
                ProtocolKv(label = "Message kind", value = "24114", separator = true)
                ProtocolKv(label = "Encryption", value = "AES-256-GCM", separator = true)
                ProtocolKv(label = "Invitation encryption", value = "X25519 ECDH + AES-GCM", separator = true)
                ProtocolKv(label = "Key derivation", value = "HKDF-SHA256", separator = true)
                ProtocolKv(label = "Topic derivation", value = "SHA256(secret)", separator = true)
                ProtocolKv(label = "Nostr signing", value = "secp256k1 Schnorr", separator = true)
                ProtocolKv(label = "Stellar signing", value = "Ed25519", separator = true)
                ProtocolKv(label = "ZK backend", value = "Groth16 / BLS12-381", separator = true)
                ProtocolKv(label = "Commitment", value = "Poseidon-only", separator = false)
            }
        }
    }
}

// MARK: - Shared atoms

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SubScreenScaffold(
    title: String,
    onBack: () -> Unit,
    content: @Composable () -> Unit
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                }
            )
        },
        containerColor = MaterialTheme.colorScheme.surfaceContainerLowest
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 8.dp)
        ) {
            content()
            Spacer(Modifier.height(32.dp))
        }
    }
}

@Composable
internal fun GroupedCard(content: @Composable () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(10.dp))
    ) { content() }
}

@Composable
internal fun SectionHeaderSm(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp)
    )
}

@Composable
internal fun FooterNote(text: String, error: Boolean = false) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp)
    )
}

@Composable
internal fun RowSeparator(start: androidx.compose.ui.unit.Dp) {
    Box(
        Modifier
            .padding(start = start)
            .fillMaxWidth()
            .height(0.5.dp)
            .background(MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f))
    )
}

@Composable
private fun SummaryCard(
    iconBg: Brush,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    subtitle: String,
    badge: String? = null,
    badgeColor: Color = SettingsIconColors.Green
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(14.dp))
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(
            modifier = Modifier.size(38.dp).background(iconBg, CircleShape),
            contentAlignment = Alignment.Center
        ) {
            Icon(icon, contentDescription = null, tint = Color.White, modifier = Modifier.size(18.dp))
        }
        Spacer(Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold)
            Text(subtitle, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        badge?.let { StatusChip(it, badgeColor) }
    }
}

@Composable
private fun StatusChip(text: String, color: Color) {
    Text(
        text,
        style = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.SemiBold),
        color = color,
        modifier = Modifier
            .background(color.copy(alpha = 0.15f), RoundedCornerShape(50))
            .padding(horizontal = 10.dp, vertical = 4.dp)
    )
}

@Composable
private fun EmptyState(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    subtitle: String
) {
    Column(
        modifier = Modifier.fillMaxWidth().padding(vertical = 20.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        Box(
            modifier = Modifier
                .size(64.dp)
                .background(MaterialTheme.colorScheme.onSurface.copy(alpha = 0.06f), CircleShape),
            contentAlignment = Alignment.Center
        ) {
            Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(28.dp))
        }
        Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        Text(
            subtitle,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 32.dp)
        )
    }
}

@Composable
private fun LabeledField(
    label: String,
    value: String,
    onChange: (String) -> Unit,
    placeholder: String,
    secure: Boolean = false
) {
    Column(modifier = Modifier.padding(vertical = 4.dp)) {
        Text(
            label,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
        Spacer(Modifier.height(4.dp))
        OutlinedTextField(
            value = value,
            onValueChange = onChange,
            placeholder = { Text(placeholder) },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            visualTransformation = if (secure) PasswordVisualTransformation() else androidx.compose.ui.text.input.VisualTransformation.None,
            textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrect = false)
        )
    }
}

@Composable
private fun KeyValueRow(
    label: String,
    value: String,
    monoValue: Boolean = false,
    valueColor: Color = MaterialTheme.colorScheme.onSurfaceVariant
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
        Text(
            value,
            style = if (monoValue) MaterialTheme.typography.labelMedium.copy(fontFamily = FontFamily.Monospace)
            else MaterialTheme.typography.bodyMedium,
            color = valueColor
        )
    }
}

@Composable
private fun ProtocolKv(label: String, value: String, separator: Boolean) {
    Column {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
            Text(
                value,
                style = MaterialTheme.typography.labelMedium.copy(fontFamily = FontFamily.Monospace),
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
        if (separator) RowSeparator(start = 16.dp)
    }
}

@Composable
private fun CopyRow(context: Context, label: String, value: String) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable {
                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                clipboard.setPrimaryClip(ClipData.newPlainText(label, value))
                Toast.makeText(context, "Copied", Toast.LENGTH_SHORT).show()
            }
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary, modifier = Modifier.weight(1f))
        Icon(Icons.Default.ContentCopy, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
    }
}

@Composable
private fun TextActionRow(label: String, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable { onClick() }
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary)
    }
}

private fun truncate(s: String, keep: Int = 14): String =
    if (s.length > keep) s.take(keep) + "…" else s
