package chat.onym.android.ui.screens

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
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
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Groups
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Key
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Router
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.TextButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import chat.onym.android.BuildConfig
import chat.onym.android.ui.TestTags
import chat.onym.android.ui.components.QRCodeImage
import chat.onym.android.ui.components.SettingsIconBox
import chat.onym.android.ui.components.SettingsIconColors
import chat.onym.android.viewmodel.GroupListViewModel
import com.stellarmls.mls.SEPTier

private enum class SettingsTab { Invite, Preferences }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    viewModel: GroupListViewModel,
    onBack: (() -> Unit)? = null,
    onBackupRecoveryPhrase: (() -> Unit)? = null,
    onRegenerateAndBackup: ((onError: (String) -> Unit) -> Unit)? = null,
    onOpenRelays: () -> Unit = {},
    onOpenBlossom: () -> Unit = {},
    onOpenStellarContract: () -> Unit = {},
    onOpenAdvanced: () -> Unit = {},
    onJoinGroup: () -> Unit = {},
    onScanInvite: () -> Unit = onJoinGroup,
) {
    var tab by rememberSaveable { mutableStateOf(SettingsTab.Invite) }
    var showAbout by remember { mutableStateOf(false) }
    var showRegenerateConfirm by remember { mutableStateOf(false) }
    var regenerateError by remember { mutableStateOf<String?>(null) }

    Scaffold(
        modifier = Modifier.testTag(TestTags.Settings.Screen),
        topBar = {
            TopAppBar(
                title = { Text("Settings") },
                navigationIcon = {
                    if (onBack != null) {
                        IconButton(onClick = onBack) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                        }
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
        ) {
            SingleChoiceSegmentedButtonRow(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 8.dp)
            ) {
                SegmentedButton(
                    selected = tab == SettingsTab.Invite,
                    onClick = { tab = SettingsTab.Invite },
                    shape = SegmentedButtonDefaults.itemShape(index = 0, count = 2),
                    modifier = Modifier.testTag(TestTags.Settings.InviteTab)
                ) { Text("Invite Key") }
                SegmentedButton(
                    selected = tab == SettingsTab.Preferences,
                    onClick = { tab = SettingsTab.Preferences },
                    shape = SegmentedButtonDefaults.itemShape(index = 1, count = 2),
                    modifier = Modifier.testTag(TestTags.Settings.PreferencesTab)
                ) { Text("Preferences") }
            }

            when (tab) {
                SettingsTab.Invite -> InviteKeyTab(
                    viewModel = viewModel,
                    onScan = onScanInvite,
                    onPaste = onJoinGroup
                )
                SettingsTab.Preferences -> PreferencesTab(
                    viewModel = viewModel,
                    onOpenRelays = onOpenRelays,
                    onOpenBlossom = onOpenBlossom,
                    onOpenStellarContract = onOpenStellarContract,
                    onOpenAdvanced = onOpenAdvanced,
                    onBackupRecoveryPhrase = onBackupRecoveryPhrase,
                    onRequestRegenerate = { showRegenerateConfirm = true },
                    onShowAbout = { showAbout = true }
                )
            }
        }
    }

    if (showAbout) {
        OnboardingSheet(onDismiss = { showAbout = false }, isRevisit = true)
    }

    if (showRegenerateConfirm) {
        AlertDialog(
            onDismissRequest = { showRegenerateConfirm = false },
            title = { Text("Generate new identity?") },
            text = {
                Text(
                    "This replaces your current keys with a new BIP39-backed identity. " +
                    "Existing groups will become read-only since other members won't recognize " +
                    "your new keys — they'll need to re-invite you. You'll see the new recovery " +
                    "phrase right after."
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    showRegenerateConfirm = false
                    onRegenerateAndBackup?.invoke { err -> regenerateError = err }
                }) {
                    Text("Generate", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { showRegenerateConfirm = false }) { Text("Cancel") }
            }
        )
    }

    regenerateError?.let { err ->
        AlertDialog(
            onDismissRequest = { regenerateError = null },
            title = { Text("Regeneration failed") },
            text = { Text(err) },
            confirmButton = {
                TextButton(onClick = { regenerateError = null }) { Text("OK") }
            }
        )
    }
}

// MARK: - Invite Key Tab

@Composable
private fun InviteKeyTab(
    viewModel: GroupListViewModel,
    onScan: () -> Unit,
    onPaste: () -> Unit,
) {
    val context = LocalContext.current
    val inviteKey = viewModel.keyManager.keyAgreementPublicKeyHex

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 8.dp)
    ) {
        // QR hero card
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(22.dp))
                .padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Box(
                modifier = Modifier
                    .background(MaterialTheme.colorScheme.surface, RoundedCornerShape(16.dp))
                    .padding(14.dp),
                contentAlignment = Alignment.Center
            ) {
                QRCodeImage(text = inviteKey, size = 240.dp)
            }

            Spacer(Modifier.height(14.dp))

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                Button(
                    onClick = { shareText(context, inviteKey) },
                    modifier = Modifier.weight(1f).height(46.dp),
                    shape = RoundedCornerShape(14.dp)
                ) {
                    Icon(Icons.Default.Share, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Share link", fontWeight = FontWeight.SemiBold)
                }
                Button(
                    onClick = {
                        val cb = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                        cb.setPrimaryClip(ClipData.newPlainText("Invite Key", inviteKey))
                        Toast.makeText(context, "Copied", Toast.LENGTH_SHORT).show()
                    },
                    modifier = Modifier.weight(1f).height(46.dp),
                    shape = RoundedCornerShape(14.dp),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.primary.copy(alpha = 0.12f),
                        contentColor = MaterialTheme.colorScheme.primary
                    )
                ) {
                    Icon(Icons.Default.ContentCopy, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Copy", fontWeight = FontWeight.SemiBold)
                }
            }
        }

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("FIND FRIENDS")
        GroupedCard {
            NavRow(
                icon = Icons.Default.QrCodeScanner,
                iconBg = SettingsIconColors.Green,
                title = "Scan invite QR",
                onClick = onScan
            )
            RowSeparator(start = 58.dp)
            NavRow(
                icon = Icons.Default.Key,
                iconBg = SettingsIconColors.Purple,
                title = "Paste invite key",
                onClick = onPaste
            )
        }
        FooterNote("Share this QR code so others can find you. Your invite key is your X25519 inbox key.")
        Spacer(Modifier.height(24.dp))
    }
}

// MARK: - Preferences Tab

@Composable
private fun PreferencesTab(
    viewModel: GroupListViewModel,
    onOpenRelays: () -> Unit,
    onOpenBlossom: () -> Unit,
    onOpenStellarContract: () -> Unit,
    onOpenAdvanced: () -> Unit,
    onBackupRecoveryPhrase: (() -> Unit)?,
    onRequestRegenerate: () -> Unit,
    onShowAbout: () -> Unit,
) {
    val km = viewModel.keyManager

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 8.dp)
    ) {
        SectionHeaderSm("NETWORK")
        GroupedCard {
            NavRow(
                icon = Icons.Default.Router,
                iconBg = SettingsIconColors.Orange,
                title = "Relays",
                detail = "${viewModel.relayURLs.size}",
                onClick = onOpenRelays
            )
            RowSeparator(start = 58.dp)
            NavRow(
                icon = Icons.Default.Image,
                iconBg = SettingsIconColors.Purple,
                title = "Blossom Servers",
                detail = "${viewModel.blossomServerURLs.size}",
                onClick = onOpenBlossom
            )
        }
        FooterNote("Relays carry encrypted messages. Blossom servers store encrypted media.")

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("PROTOCOL")
        GroupedCard {
            StellarContractRow(isConfigured = viewModel.isContractConfigured, onClick = onOpenStellarContract)
            RowSeparator(start = 58.dp)
            DefaultGroupTierRow(
                current = viewModel.defaultGroupTier,
                onSelect = { viewModel.saveDefaultGroupTier(it) }
            )
        }
        FooterNote("New groups will use this tier. Larger tiers support more members but use bigger ZK circuits.")

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("SECURITY")
        GroupedCard {
            NavRow(
                icon = Icons.Default.Key,
                iconBg = SettingsIconColors.Orange,
                title = "Backup Recovery Phrase",
                detail = if (km.isBip39Backed) null else "Upgrade required",
                onClick = {
                    if (km.isBip39Backed) {
                        onBackupRecoveryPhrase?.invoke()
                    } else {
                        onRequestRegenerate()
                    }
                }
            )
        }
        FooterNote(
            if (km.isBip39Backed)
                "View your 12-word recovery phrase. You will need it to restore your identity on a new device."
            else
                "Your identity predates backup support. Generate a new BIP39-backed identity to enable backup. Your existing groups will become read-only since other members won't recognize your new keys."
        )

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("ADVANCED")
        GroupedCard {
            NavRow(
                icon = Icons.Default.Settings,
                iconBg = SettingsIconColors.Gray,
                title = "Advanced",
                onClick = onOpenAdvanced
            )
        }

        Spacer(Modifier.height(16.dp))
        SectionHeaderSm("ABOUT")
        GroupedCard {
            DetailRow(
                icon = Icons.Default.Info,
                iconBg = SettingsIconColors.Gray,
                title = "Version",
                detail = BuildConfig.VERSION_NAME
            )
            RowSeparator(start = 58.dp)
            DetailRow(
                icon = Icons.Default.Lock,
                iconBg = SettingsIconColors.Blue,
                title = "End-to-end encryption",
                detail = null
            )
            RowSeparator(start = 58.dp)
            NavRow(
                icon = Icons.Default.AutoAwesome,
                iconBg = SettingsIconColors.Pink,
                title = "About Stellar Chat",
                titleColor = MaterialTheme.colorScheme.primary,
                onClick = onShowAbout
            )
        }
        FooterNote("Messages are encrypted end-to-end. Relays see only opaque ciphertext and hidden topic tags. Group IDs never appear in cleartext on the wire.")
        Spacer(Modifier.height(32.dp))
    }
}

// MARK: - Nav rows

@Composable
private fun NavRow(
    icon: ImageVector,
    iconBg: Color,
    title: String,
    detail: String? = null,
    titleColor: Color = MaterialTheme.colorScheme.onSurface,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        SettingsIconBox(icon = icon, background = iconBg)
        Spacer(Modifier.width(12.dp))
        Text(title, style = MaterialTheme.typography.bodyLarge, color = titleColor, modifier = Modifier.weight(1f))
        if (detail != null) {
            Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.width(4.dp))
        }
        Icon(
            Icons.AutoMirrored.Filled.KeyboardArrowRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
            modifier = Modifier.size(18.dp)
        )
    }
}

@Composable
private fun DetailRow(
    icon: ImageVector,
    iconBg: Color,
    title: String,
    detail: String?,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        SettingsIconBox(icon = icon, background = iconBg)
        Spacer(Modifier.width(12.dp))
        Text(title, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
        if (detail != null) {
            Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun StellarContractRow(isConfigured: Boolean, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        SettingsIconBox(icon = Icons.Default.Star, background = SettingsIconColors.Yellow)
        Spacer(Modifier.width(12.dp))
        Text("Stellar Contract", style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
        Box(
            modifier = Modifier
                .size(6.dp)
                .background(if (isConfigured) SettingsIconColors.Green else SettingsIconColors.Gray, CircleShape)
        )
        Spacer(Modifier.width(5.dp))
        Text(
            if (isConfigured) "Connected" else "Not configured",
            style = MaterialTheme.typography.bodyMedium,
            color = if (isConfigured) SettingsIconColors.Green else MaterialTheme.colorScheme.onSurfaceVariant
        )
        Spacer(Modifier.width(4.dp))
        Icon(
            Icons.AutoMirrored.Filled.KeyboardArrowRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
            modifier = Modifier.size(18.dp)
        )
    }
}

@Composable
private fun DefaultGroupTierRow(current: SEPTier, onSelect: (SEPTier) -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    val label = when (current) {
        SEPTier.SMALL -> "32 members"
        SEPTier.MEDIUM -> "256 members"
        SEPTier.LARGE -> "2,048 members"
    }

    Box {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable { expanded = true }
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            SettingsIconBox(icon = Icons.Default.Groups, background = SettingsIconColors.Green)
            Spacer(Modifier.width(12.dp))
            Text("Default Group Capacity", style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.width(4.dp))
            Icon(
                Icons.AutoMirrored.Filled.KeyboardArrowRight,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                modifier = Modifier.size(18.dp)
            )
        }
        DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
            listOf(
                SEPTier.SMALL to "32 members",
                SEPTier.MEDIUM to "256 members",
                SEPTier.LARGE to "2,048 members"
            ).forEach { (tier, text) ->
                DropdownMenuItem(
                    text = { Text(text, fontWeight = if (tier == current) FontWeight.SemiBold else FontWeight.Normal) },
                    onClick = {
                        onSelect(tier)
                        expanded = false
                    }
                )
            }
        }
    }
}

private fun shareText(context: Context, text: String) {
    val intent = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_TEXT, text)
    }
    context.startActivity(Intent.createChooser(intent, "Share invite key"))
}
