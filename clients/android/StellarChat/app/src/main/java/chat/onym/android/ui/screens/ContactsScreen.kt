package chat.onym.android.ui.screens

import android.Manifest
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
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
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import chat.onym.android.ui.TestTags
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import chat.onym.android.contacts.ContactsPermission
import chat.onym.android.contacts.PhoneContact
import chat.onym.android.contacts.SystemContactsImporter
import chat.onym.android.model.ChatGroup
import chat.onym.android.model.ChatMessage
import chat.onym.android.model.InviteCode
import chat.onym.android.persistence.ContactAliasStore
import chat.onym.android.persistence.InvitedContact
import chat.onym.android.persistence.InvitedContactKind
import chat.onym.android.persistence.InvitedContactStatus
import chat.onym.android.persistence.InvitedContactStore
import chat.onym.android.persistence.StellarChatDao
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.Calendar
import java.util.Date

data class Contact(
    val pubkey: String,
    val groupNames: String,
    val lastSeen: Date
)

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ContactsScreen(
    groups: List<ChatGroup>,
    chatMessages: Map<String, List<ChatMessage>>,
    contactAliasStore: ContactAliasStore,
    invitedContactStore: InvitedContactStore,
    myKeyAgreementPublicKeyHex: String,
    dao: StellarChatDao
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var aliasTarget by remember { mutableStateOf<String?>(null) }
    var aliasText by remember { mutableStateOf("") }
    var pickerTarget by remember { mutableStateOf<PhoneContact?>(null) }
    var phonebook by remember { mutableStateOf<List<PhoneContact>>(emptyList()) }
    var importing by remember { mutableStateOf(false) }
    var importError by remember { mutableStateOf<String?>(null) }
    var filter by remember { mutableStateOf("") }
    var permission by remember { mutableStateOf(SystemContactsImporter.permission(context)) }

    val loadPhonebook: () -> Unit = {
        importing = true
        scope.launch {
            try {
                val loaded = withContext(Dispatchers.IO) {
                    SystemContactsImporter.fetchAll(context)
                }
                phonebook = loaded
                importError = null
            } catch (e: Exception) {
                importError = "Couldn't load contacts: ${e.message ?: ""}"
            } finally {
                importing = false
            }
        }
    }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted ->
        permission = if (granted) ContactsPermission.AUTHORIZED else ContactsPermission.DENIED
        if (granted) loadPhonebook()
    }

    // Re-check permission on entry: the user may have toggled it in system
    // settings while the app was backgrounded.
    LaunchedEffect(Unit) {
        permission = SystemContactsImporter.permission(context)
        if (permission == ContactsPermission.AUTHORIZED && phonebook.isEmpty() && !importing) {
            loadPhonebook()
        }
    }

    val activeContacts = remember(chatMessages, groups) {
        val map = mutableMapOf<String, Pair<MutableSet<String>, Date>>()
        for ((groupID, messages) in chatMessages) {
            for (msg in messages) {
                if (msg.isMine || msg.isSystemMessage) continue
                val entry = map.getOrPut(msg.senderPubkey) { Pair(mutableSetOf(), Date(0)) }
                entry.first.add(groupID)
                if (msg.timestamp.after(entry.second)) {
                    map[msg.senderPubkey] = Pair(entry.first, msg.timestamp)
                }
            }
        }
        val groupMap = groups.associateBy { it.id }
        map.entries
            .map { (pubkey, info) ->
                val names = info.first.mapNotNull { groupMap[it]?.name }.joinToString(", ")
                Contact(pubkey, names, info.second)
            }
            .sortedByDescending { it.lastSeen.time }
    }

    val invitedPending = invitedContactStore.contacts.filter { it.status != InvitedContactStatus.JOINED }

    val invitedPhones = invitedContactStore.contacts.map { it.phone }.toSet()
    val filteredPhonebook = remember(phonebook, invitedPhones, filter) {
        val base = phonebook.filter { it.phone !in invitedPhones }
        if (filter.isBlank()) base else {
            val needle = filter.lowercase()
            base.filter {
                it.displayName.lowercase().contains(needle) || it.phone.contains(filter)
            }
        }
    }

    val isEmpty = activeContacts.isEmpty() && invitedPending.isEmpty() && phonebook.isEmpty()

    LazyColumn(modifier = Modifier
        .fillMaxSize()
        .testTag(TestTags.Contacts.Screen)) {
        if (isEmpty && permission != ContactsPermission.AUTHORIZED) {
            item {
                Box(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 48.dp),
                    contentAlignment = Alignment.Center
                ) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(
                            "No Contacts",
                            style = MaterialTheme.typography.titleLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                        Text(
                            "People you chat with will appear here. You can also invite friends from your phone contacts.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(top = 8.dp, start = 32.dp, end = 32.dp),
                        )
                    }
                }
            }
        }

        if (activeContacts.isNotEmpty()) {
            item { SectionHeader("Active") }
            items(activeContacts, key = { "active-" + it.pubkey }) { contact ->
                val alias = contactAliasStore.displayName(contact.pubkey)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp, vertical = 12.dp)
                        .combinedClickable(
                            onClick = {},
                            onLongClick = {
                                aliasText = alias ?: ""
                                aliasTarget = contact.pubkey
                            }
                        ),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    ContactAvatar(pubkey = contact.pubkey, alias = alias)
                    Spacer(modifier = Modifier.width(12.dp))
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            alias ?: (contact.pubkey.take(12) + "..."),
                            style = MaterialTheme.typography.bodyLarge,
                            fontWeight = FontWeight.Medium
                        )
                        Text(
                            contact.groupNames,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1
                        )
                    }
                    Text(
                        contactTimestamp(contact.lastSeen),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }
        }

        if (invitedPending.isNotEmpty()) {
            item { SectionHeader("Invited") }
            items(invitedPending, key = { "invited-" + it.phone }) { contact ->
                InvitedRow(
                    contact = contact,
                    onResend = {
                        // Rebuild the URL from current state: identity may have
                        // rotated (BIP39 restore/regenerate) or group state may
                        // have advanced since the original invite was stored.
                        // Using the cached URL would point to a stale inbox or
                        // stale group snapshot that no longer reconciles.
                        val freshURL = rebuildInviteURL(
                            contact,
                            groups,
                            myKeyAgreementPublicKeyHex
                        ) ?: contact.inviteURL
                        scope.launch {
                            if (freshURL != contact.inviteURL) {
                                invitedContactStore.upsert(
                                    contact.copy(inviteURL = freshURL),
                                    dao
                                )
                            }
                            val launched = launchTelegram(context, freshURL)
                            if (launched) {
                                invitedContactStore.setStatus(
                                    contact.phone,
                                    InvitedContactStatus.SENT,
                                    dao
                                )
                            }
                        }
                    },
                    onRemove = {
                        scope.launch { invitedContactStore.remove(contact.phone, dao) }
                    }
                )
            }
        }

        item { SectionHeader("From Phonebook") }

        when (permission) {
            ContactsPermission.NOT_DETERMINED -> item {
                Column(modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
                    Text(
                        "Sync your phone contacts to invite friends to Onym via Telegram. Contacts never leave this device.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    Spacer(modifier = Modifier.size(8.dp))
                    Button(
                        onClick = {
                            permissionLauncher.launch(Manifest.permission.READ_CONTACTS)
                        }
                    ) { Text("Sync Contacts") }
                }
            }

            ContactsPermission.DENIED -> item {
                Column(modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
                    Text(
                        "Contacts access is off. Enable it in Settings → Onym to sync your phonebook.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    Spacer(modifier = Modifier.size(8.dp))
                    OutlinedButton(onClick = {
                        val intent = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS).apply {
                            data = Uri.fromParts("package", context.packageName, null)
                            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                        }
                        try { context.startActivity(intent) } catch (_: Exception) {}
                    }) { Text("Open Settings") }
                }
            }

            ContactsPermission.AUTHORIZED -> {
                if (importing) {
                    item {
                        Text(
                            "Loading contacts…",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp)
                        )
                    }
                } else if (phonebook.isEmpty()) {
                    item {
                        OutlinedButton(
                            onClick = loadPhonebook,
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)
                        ) { Text("Load Contacts") }
                    }
                }
                importError?.let {
                    item {
                        Text(
                            it,
                            style = MaterialTheme.typography.bodySmall,
                            color = Color.Red,
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp)
                        )
                    }
                }
                if (phonebook.isNotEmpty()) {
                    item {
                        OutlinedTextField(
                            value = filter,
                            onValueChange = { filter = it },
                            placeholder = { Text("Search") },
                            singleLine = true,
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Text),
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 16.dp, vertical = 4.dp)
                        )
                    }
                    items(filteredPhonebook, key = { "phone-" + it.id }) { phoneContact ->
                        PhonebookRow(
                            contact = phoneContact,
                            onInvite = { pickerTarget = phoneContact }
                        )
                    }
                }
            }
        }
    }

    aliasTarget?.let { pubkey ->
        AlertDialog(
            onDismissRequest = { aliasTarget = null },
            title = { Text("Set Name") },
            text = {
                Column {
                    Text(
                        pubkey.take(16) + "...",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    OutlinedTextField(
                        value = aliasText,
                        onValueChange = { aliasText = it },
                        placeholder = { Text("Display name") },
                        singleLine = true,
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(top = 8.dp)
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    scope.launch {
                        contactAliasStore.setAlias(pubkey, aliasText, dao)
                    }
                    aliasTarget = null
                }) {
                    Text("Save")
                }
            },
            dismissButton = {
                Row {
                    if (contactAliasStore.displayName(pubkey) != null) {
                        TextButton(onClick = {
                            scope.launch {
                                contactAliasStore.removeAlias(pubkey, dao)
                            }
                            aliasTarget = null
                        }) {
                            Text("Remove", color = Color.Red)
                        }
                    }
                    TextButton(onClick = { aliasTarget = null }) {
                        Text("Cancel")
                    }
                }
            }
        )
    }

    pickerTarget?.let { target ->
        TelegramInvitePickerSheet(
            contact = target,
            groups = groups,
            myKeyAgreementPublicKeyHex = myKeyAgreementPublicKeyHex,
            invitedContactStore = invitedContactStore,
            dao = dao,
            onDismiss = { pickerTarget = null }
        )
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        title.uppercase(),
        style = MaterialTheme.typography.labelSmall,
        fontWeight = FontWeight.Bold,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)
    )
}

@Composable
private fun InvitedRow(
    contact: InvitedContact,
    onResend: () -> Unit,
    onRemove: () -> Unit
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(
            modifier = Modifier
                .size(40.dp)
                .background(MaterialTheme.colorScheme.primary, CircleShape),
            contentAlignment = Alignment.Center
        ) {
            Icon(
                Icons.AutoMirrored.Filled.Send,
                contentDescription = null,
                tint = Color.White,
                modifier = Modifier.size(20.dp)
            )
        }
        Spacer(modifier = Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                contact.displayName,
                style = MaterialTheme.typography.bodyLarge,
                fontWeight = FontWeight.Medium
            )
            Text(
                contact.phone,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
        StatusChip(status = contact.status)
        IconButton(onClick = onResend) {
            Icon(
                Icons.Default.Refresh,
                contentDescription = "Resend",
                tint = MaterialTheme.colorScheme.primary
            )
        }
        IconButton(onClick = onRemove) {
            Icon(
                Icons.Default.Delete,
                contentDescription = "Remove",
                tint = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

@Composable
private fun StatusChip(status: InvitedContactStatus) {
    val (label, color) = when (status) {
        InvitedContactStatus.PENDING -> "Pending" to Color(0xFFFB8C00)
        InvitedContactStatus.SENT -> "Sent" to Color(0xFF1E88E5)
        InvitedContactStatus.JOINED -> "Joined" to Color(0xFF43A047)
    }
    Box(
        modifier = Modifier
            .background(color.copy(alpha = 0.15f), RoundedCornerShape(12.dp))
            .padding(horizontal = 8.dp, vertical = 3.dp)
    ) {
        Text(
            label,
            style = MaterialTheme.typography.labelSmall,
            fontWeight = FontWeight.SemiBold,
            color = color
        )
    }
}

@Composable
private fun PhonebookRow(contact: PhoneContact, onInvite: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(
            modifier = Modifier
                .size(40.dp)
                .background(
                    MaterialTheme.colorScheme.surfaceVariant,
                    CircleShape
                ),
            contentAlignment = Alignment.Center
        ) {
            Icon(
                Icons.Default.Person,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(22.dp)
            )
        }
        Spacer(modifier = Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                contact.displayName,
                style = MaterialTheme.typography.bodyLarge,
                fontWeight = FontWeight.Medium
            )
            Text(
                contact.phone,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
        OutlinedButton(
            onClick = onInvite,
            contentPadding = ButtonDefaults.ContentPadding
        ) { Text("Invite") }
    }
}

@Composable
fun ContactAvatar(pubkey: String, alias: String? = null) {
    val palette = listOf(
        Color(0xFFE53935), Color(0xFFFF9800), Color(0xFFFDD835),
        Color(0xFF43A047), Color(0xFF00897B), Color(0xFF1E88E5),
        Color(0xFF3949AB), Color(0xFF8E24AA), Color(0xFFD81B60),
        Color(0xFF6D4C41)
    )
    val colorIndex = pubkey.take(2).toIntOrNull(16)?.rem(palette.size) ?: 0
    val initials = if (alias != null && alias.isNotEmpty()) {
        alias.first().uppercase()
    } else {
        pubkey.take(2).uppercase()
    }

    Box(
        modifier = Modifier
            .size(40.dp)
            .background(palette[colorIndex], CircleShape),
        contentAlignment = Alignment.Center
    ) {
        Text(
            initials,
            style = MaterialTheme.typography.titleSmall,
            fontWeight = FontWeight.Bold,
            color = Color.White
        )
    }
}

private fun rebuildInviteURL(
    contact: InvitedContact,
    groups: List<ChatGroup>,
    myKeyAgreementPublicKeyHex: String
): String? = when (contact.kind) {
    InvitedContactKind.ONBOARD -> {
        contact.handshakeNonce?.let { nonce ->
            "https://onym.chat/onboard?inviter=$myKeyAgreementPublicKeyHex&nonce=$nonce"
        }
    }
    InvitedContactKind.GROUP -> {
        val group = groups.firstOrNull { it.id == contact.groupID }
        group?.let {
            val code = InviteCode(
                groupID = it.groupIDData,
                groupSecret = it.groupSecret,
                name = it.name,
                relayHints = it.relayHints,
                members = it.members.toList(),
                epoch = it.epoch,
                salt = it.salt,
                commitment = it.commitment,
                tierRawValue = it.tier.id,
                groupTypeRawValue = it.groupType.id,
                adminPubkeys = it.adminPubkeys.toList()
            ).encode()
            "https://onym.chat/join?code=$code"
        }
    }
}

private fun contactTimestamp(date: Date): String {
    val seconds = (System.currentTimeMillis() - date.time) / 1000
    if (seconds < 60) return "Just now"
    if (seconds < 3600) return "${seconds / 60}m"
    if (seconds < 86400) return "${seconds / 3600}h"
    val cal = Calendar.getInstance()
    val today = Calendar.getInstance()
    cal.time = date
    if (cal.get(Calendar.YEAR) == today.get(Calendar.YEAR)
        && cal.get(Calendar.DAY_OF_YEAR) == today.get(Calendar.DAY_OF_YEAR) - 1
    ) return "Yesterday"
    val fmt = java.text.SimpleDateFormat("MMM d", java.util.Locale.getDefault())
    return fmt.format(date)
}
