package chat.onym.android.ui.screens

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import chat.onym.android.model.ChatGroup
import chat.onym.android.model.ChatMessage
import java.util.Calendar
import java.util.Date

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun SearchScreen(
    groups: List<ChatGroup>,
    chatMessages: Map<String, List<ChatMessage>>,
    onGroupClick: (ChatGroup) -> Unit
) {
    var searchText by remember { mutableStateOf("") }

    val filteredGroups = remember(groups, searchText) {
        if (searchText.isBlank()) emptyList()
        else groups.filter { it.name.contains(searchText, ignoreCase = true) }
    }
    val messageResults = remember(chatMessages, groups, searchText) {
        if (searchText.isBlank()) emptyList()
        else {
            val groupMap = groups.associateBy { it.id }
            chatMessages.flatMap { (groupId, msgs) ->
                msgs.filter { it.text.contains(searchText, ignoreCase = true) }
                    .mapNotNull { msg -> groupMap[groupId]?.let { g -> Pair(g, msg) } }
            }
                .sortedByDescending { it.second.timestamp.time }
                .take(50)
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(title = { Text("Search") })
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
        ) {
            OutlinedTextField(
                value = searchText,
                onValueChange = { searchText = it },
                placeholder = { Text("Search chats and messages") },
                singleLine = true,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 8.dp)
            )

            if (searchText.isBlank()) {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        "Search across all your chats and messages.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            } else if (filteredGroups.isEmpty() && messageResults.isEmpty()) {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        "No results for \"$searchText\"",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            } else {
                LazyColumn(modifier = Modifier.fillMaxSize()) {
                    if (filteredGroups.isNotEmpty()) {
                        item {
                            Text(
                                "Chats",
                                style = MaterialTheme.typography.titleSmall,
                                color = MaterialTheme.colorScheme.primary,
                                modifier = Modifier.padding(start = 16.dp, top = 8.dp, bottom = 4.dp)
                            )
                        }
                        itemsIndexed(filteredGroups, key = { _, g -> g.id }) { _, group ->
                            Card(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(horizontal = 16.dp, vertical = 2.dp)
                                    .combinedClickable(onClick = { onGroupClick(group) })
                            ) {
                                Row(
                                    modifier = Modifier.padding(12.dp),
                                    verticalAlignment = Alignment.CenterVertically
                                ) {
                                    Text(
                                        group.name,
                                        style = MaterialTheme.typography.bodyLarge,
                                        modifier = Modifier.weight(1f)
                                    )
                                    Text(
                                        "${group.members.size} members",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant
                                    )
                                }
                            }
                        }
                    }

                    if (messageResults.isNotEmpty()) {
                        item {
                            Text(
                                "Messages",
                                style = MaterialTheme.typography.titleSmall,
                                color = MaterialTheme.colorScheme.primary,
                                modifier = Modifier.padding(start = 16.dp, top = 12.dp, bottom = 4.dp)
                            )
                        }
                        itemsIndexed(messageResults, key = { i, pair -> "${pair.second.id}_$i" }) { _, (group, msg) ->
                            Card(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(horizontal = 16.dp, vertical = 2.dp)
                                    .combinedClickable(onClick = { onGroupClick(group) })
                            ) {
                                Column(modifier = Modifier.padding(12.dp)) {
                                    Row {
                                        Text(
                                            group.name,
                                            style = MaterialTheme.typography.labelMedium,
                                            fontWeight = FontWeight.SemiBold,
                                            modifier = Modifier.weight(1f)
                                        )
                                        Text(
                                            searchTimestamp(msg.timestamp),
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant
                                        )
                                    }
                                    Spacer(modifier = Modifier.height(2.dp))
                                    Text(
                                        msg.text,
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        maxLines = 2,
                                        overflow = TextOverflow.Ellipsis
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

private fun searchTimestamp(date: Date): String {
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
