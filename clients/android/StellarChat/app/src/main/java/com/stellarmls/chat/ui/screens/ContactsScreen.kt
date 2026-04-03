package com.stellarmls.chat.ui.screens

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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.stellarmls.chat.model.ChatGroup
import com.stellarmls.chat.model.ChatMessage
import com.stellarmls.chat.persistence.ContactAliasStore
import com.stellarmls.chat.persistence.StellarChatDao
import kotlinx.coroutines.launch
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
    dao: StellarChatDao
) {
    val scope = rememberCoroutineScope()
    var aliasTarget by remember { mutableStateOf<String?>(null) }
    var aliasText by remember { mutableStateOf("") }

    val contacts = remember(chatMessages, groups) {
        val map = mutableMapOf<String, Pair<MutableSet<String>, Date>>()
        for ((groupID, messages) in chatMessages) {
            for (msg in messages) {
                if (msg.isMine) continue
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

    if (contacts.isEmpty()) {
        Box(
            modifier = Modifier.fillMaxSize(),
            contentAlignment = Alignment.Center
        ) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(
                    "No Contacts",
                    style = MaterialTheme.typography.titleLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Text(
                    "People you chat with will appear here.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 8.dp)
                )
            }
        }
    } else {
        LazyColumn(modifier = Modifier.fillMaxSize()) {
            items(contacts, key = { it.pubkey }) { contact ->
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
