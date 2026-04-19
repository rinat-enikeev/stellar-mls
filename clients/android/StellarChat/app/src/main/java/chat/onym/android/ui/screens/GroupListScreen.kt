package chat.onym.android.ui.screens

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.background
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Email
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.WifiOff
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import chat.onym.android.model.ChatGroup
import chat.onym.android.model.ChatMessage
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.util.Calendar
import java.util.Date

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun GroupListScreen(
    groups: List<ChatGroup>,
    pendingInvitationCount: Int = 0,
    chatMessages: Map<String, List<ChatMessage>> = emptyMap(),
    unreadCounts: Map<String, Int> = emptyMap(),
    isRelayConnected: Boolean = true,
    onGroupClick: (ChatGroup) -> Unit,
    onInviteMember: (ChatGroup) -> Unit = {},
    onCreateGroup: () -> Unit,
    onJoinGroup: () -> Unit,
    onInvitations: () -> Unit = {},
    onDeleteGroup: (String) -> Unit,
    onTogglePin: (String) -> Unit = {},
    onRefresh: () -> Unit = {},
    onRestore: (() -> Unit)? = null
) {
    var showAddDialog by remember { mutableStateOf(false) }
    var groupToDelete by remember { mutableStateOf<ChatGroup?>(null) }
    var groupForContextMenu by remember { mutableStateOf<ChatGroup?>(null) }
    var isRefreshing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    val context = LocalContext.current
    val prefs = remember { context.getSharedPreferences("stellar_chat", android.content.Context.MODE_PRIVATE) }
    var showOnboarding by remember { mutableStateOf(!prefs.getBoolean("has_seen_onboarding", false)) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Chats") },
                actions = {
                    IconButton(onClick = onInvitations) {
                        BadgedBox(
                            badge = {
                                if (pendingInvitationCount > 0) {
                                    Badge { Text("$pendingInvitationCount") }
                                }
                            }
                        ) {
                            Icon(Icons.Default.Email, contentDescription = "Invitations")
                        }
                    }
                }
            )
        },
        floatingActionButton = {
            FloatingActionButton(onClick = { showAddDialog = true }) {
                Icon(Icons.Default.Add, contentDescription = "Add Group")
            }
        }
    ) { padding ->
        Column(modifier = Modifier.padding(padding)) {
            if (!isRelayConnected) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(MaterialTheme.colorScheme.error)
                        .padding(vertical = 4.dp),
                    horizontalArrangement = Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Icon(
                        Icons.Default.WifiOff,
                        contentDescription = null,
                        modifier = Modifier.size(14.dp),
                        tint = MaterialTheme.colorScheme.onError
                    )
                    Spacer(modifier = Modifier.width(6.dp))
                    Text(
                        "No relay connection",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onError
                    )
                }
            }
            PullToRefreshBox(
                isRefreshing = isRefreshing,
                onRefresh = {
                    isRefreshing = true
                    onRefresh()
                    scope.launch {
                        delay(1000)
                        isRefreshing = false
                    }
                },
                modifier = Modifier.fillMaxSize()
            ) {
                if (groups.isEmpty()) {
                    Column(
                        modifier = Modifier.fillMaxSize().padding(horizontal = 32.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center
                    ) {
                        Icon(
                            Icons.Default.Email,
                            contentDescription = null,
                            modifier = Modifier.size(48.dp),
                            tint = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                        Spacer(modifier = Modifier.height(16.dp))
                        Text(
                            "Start a conversation",
                            style = MaterialTheme.typography.titleLarge,
                            fontWeight = FontWeight.Bold
                        )
                        Spacer(modifier = Modifier.height(24.dp))
                        Card(
                            onClick = onCreateGroup,
                            modifier = Modifier.fillMaxWidth(),
                            colors = androidx.compose.material3.CardDefaults.cardColors(
                                containerColor = MaterialTheme.colorScheme.primaryContainer
                            )
                        ) {
                            Row(
                                modifier = Modifier.padding(16.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Column(modifier = Modifier.weight(1f)) {
                                    Text("Create a group", style = MaterialTheme.typography.titleMedium)
                                    Text(
                                        "Start a private space and invite people you trust",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onPrimaryContainer
                                    )
                                }
                                Icon(
                                    Icons.Default.Add,
                                    contentDescription = null,
                                    tint = MaterialTheme.colorScheme.onPrimaryContainer
                                )
                            }
                        }
                        Spacer(modifier = Modifier.height(12.dp))
                        Card(
                            onClick = onJoinGroup,
                            modifier = Modifier.fillMaxWidth(),
                            colors = androidx.compose.material3.CardDefaults.cardColors(
                                containerColor = MaterialTheme.colorScheme.surfaceVariant
                            )
                        ) {
                            Row(
                                modifier = Modifier.padding(16.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Column(modifier = Modifier.weight(1f)) {
                                    Text("Join a group", style = MaterialTheme.typography.titleMedium)
                                    Text(
                                        "Have an invite link or code? Join here",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant
                                    )
                                }
                                Icon(
                                    Icons.Default.Person,
                                    contentDescription = null,
                                    tint = MaterialTheme.colorScheme.onSurfaceVariant
                                )
                            }
                        }
                    }
                } else {
                    LazyColumn(
                        modifier = Modifier.fillMaxSize()
                    ) {
                        itemsIndexed(groups, key = { _, g -> g.id }) { index, group ->
                            val lastMessage = chatMessages[group.id]?.lastOrNull()
                            val unread = unreadCounts[group.id] ?: 0

                            GroupListItem(
                                group = group,
                                lastMessage = lastMessage,
                                unreadCount = unread,
                                onClick = { onGroupClick(group) },
                                onLongClick = { groupForContextMenu = group }
                            )
                            if (index < groups.lastIndex) {
                                HorizontalDivider(
                                    modifier = Modifier.padding(start = 76.dp),
                                    color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f)
                                )
                            }
                        }
                    }
                }
            }
        }

        groupForContextMenu?.let { group ->
            AlertDialog(
                onDismissRequest = { groupForContextMenu = null },
                title = { Text(group.name) },
                text = {
                    Column {
                        TextButton(onClick = {
                            onTogglePin(group.id)
                            groupForContextMenu = null
                        }) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                Text(if (group.isPinned) "\u274C" else "\uD83D\uDCCC")
                                Text(if (group.isPinned) "Unpin" else "Pin")
                            }
                        }
                        TextButton(onClick = {
                            groupForContextMenu = null
                            groupToDelete = group
                        }) {
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                Text("\uD83D\uDDD1\uFE0F")
                                Text("Delete", color = Color.Red)
                            }
                        }
                    }
                },
                confirmButton = {},
                dismissButton = {
                    TextButton(onClick = { groupForContextMenu = null }) {
                        Text("Cancel")
                    }
                }
            )
        }

        groupToDelete?.let { group ->
            AlertDialog(
                onDismissRequest = { groupToDelete = null },
                title = { Text("Delete Group") },
                text = { Text("Delete \"${group.name}\"? This cannot be undone.") },
                confirmButton = {
                    TextButton(onClick = {
                        onDeleteGroup(group.id)
                        groupToDelete = null
                    }) {
                        Text("Delete", color = Color.Red)
                    }
                },
                dismissButton = {
                    TextButton(onClick = { groupToDelete = null }) {
                        Text("Cancel")
                    }
                }
            )
        }

        if (showAddDialog) {
            AlertDialog(
                onDismissRequest = { showAddDialog = false },
                title = { Text("Add Group") },
                text = { Text("Create a new group or join an existing one?") },
                confirmButton = {
                    TextButton(onClick = {
                        showAddDialog = false
                        onCreateGroup()
                    }) {
                        Text("Create")
                    }
                },
                dismissButton = {
                    TextButton(onClick = {
                        showAddDialog = false
                        onJoinGroup()
                    }) {
                        Text("Join")
                    }
                }
            )
        }

        if (showOnboarding) {
            OnboardingSheet(
                onDismiss = {
                    prefs.edit().putBoolean("has_seen_onboarding", true).apply()
                    showOnboarding = false
                },
                onRestore = onRestore
            )
        }
    }
}

private val avatarColors = listOf(
    Color(0xFF1A73E8), // blue
    Color(0xFF7B1FA2), // purple
    Color(0xFFE65100), // orange
    Color(0xFFC2185B), // pink
    Color(0xFF00897B), // teal
    Color(0xFF3949AB), // indigo
    Color(0xFF26A69A), // mint
    Color(0xFF0097A7), // cyan
)

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun GroupListItem(
    group: ChatGroup,
    lastMessage: ChatMessage?,
    unreadCount: Int,
    onClick: () -> Unit,
    onLongClick: () -> Unit
) {
    val avatarColor = remember(group.id) {
        avatarColors[(group.id.hashCode() and 0x7fffffff) % avatarColors.size]
    }
    val initial = remember(group.name) {
        group.name.take(1).uppercase()
    }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .combinedClickable(
                onClick = onClick,
                onLongClick = onLongClick
            )
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        // Avatar with on-chain badge overlay
        Box(modifier = Modifier.size(48.dp)) {
            Surface(
                shape = CircleShape,
                color = avatarColor,
                modifier = Modifier.size(48.dp)
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Text(
                        text = initial,
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                        color = Color.White
                    )
                }
            }
            if (group.isPublishedOnChain) {
                Surface(
                    shape = CircleShape,
                    color = MaterialTheme.colorScheme.surface,
                    modifier = Modifier
                        .size(18.dp)
                        .align(Alignment.BottomEnd)
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        Text(
                            text = "\u2713",
                            color = Color(0xFF4CAF50),
                            style = MaterialTheme.typography.labelSmall,
                            fontWeight = FontWeight.Bold
                        )
                    }
                }
            }
        }
        Spacer(modifier = Modifier.width(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            // Top line: name + fork badge + timestamp
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    group.name,
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = if (unreadCount > 0) FontWeight.SemiBold else FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false)
                )
                if (group.isPinned) {
                    Spacer(modifier = Modifier.width(4.dp))
                    Text(
                        text = "\uD83D\uDCCC",
                        style = MaterialTheme.typography.labelSmall
                    )
                }
                if (group.forkedFromGroupID != null) {
                    Spacer(modifier = Modifier.width(4.dp))
                    Text(
                        text = "\u2442",
                        color = Color(0xFF7B1FA2),
                        style = MaterialTheme.typography.labelSmall
                    )
                }
                Spacer(modifier = Modifier.weight(1f))
                if (lastMessage != null) {
                    Text(
                        text = relativeTimestamp(lastMessage.timestamp),
                        style = MaterialTheme.typography.labelSmall,
                        color = if (unreadCount > 0) MaterialTheme.colorScheme.primary
                                else MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }
            Spacer(modifier = Modifier.height(3.dp))
            // Bottom line: message preview + unread badge
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (lastMessage != null) {
                    val previewText = if (lastMessage.mediaAttachment != null) {
                        when {
                            lastMessage.mediaAttachment.mimeType.startsWith("video/") -> "\uD83C\uDFA5 Video"
                            lastMessage.mediaAttachment.mimeType.startsWith("audio/") -> "\uD83C\uDF99 Voice message"
                            lastMessage.mediaAttachment.mimeType.startsWith("image/") -> "\uD83D\uDCF7 Photo"
                            else -> "\uD83D\uDCC4 File"
                        }
                    } else {
                        lastMessage.text
                    }
                    Text(
                        text = previewText,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f)
                    )
                } else {
                    Text(
                        "${group.members.size} members",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f)
                    )
                    Spacer(modifier = Modifier.weight(1f))
                }
                if (unreadCount > 0) {
                    Spacer(modifier = Modifier.width(8.dp))
                    Surface(
                        shape = CircleShape,
                        color = MaterialTheme.colorScheme.primary
                    ) {
                        Text(
                            text = "$unreadCount",
                            style = MaterialTheme.typography.labelSmall,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onPrimary,
                            modifier = Modifier.padding(
                                horizontal = 6.dp,
                                vertical = 2.dp
                            )
                        )
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun OnboardingSheet(onDismiss: () -> Unit, isRevisit: Boolean = false, onRestore: (() -> Unit)? = null) {
    val pages = remember {
        listOf(
            Triple(Icons.Default.Lock, "Now your messages and\nmetadata are encrypted", "Most messengers encrypt your messages but still collect who you talk to, when, and how often. That metadata tells a complete story about you."),
            Triple(Icons.Default.Person, "Private by design.\nAnonymous by default.", "No phone numbers. No accounts. No social graph. Even other group members won't know anything about you beyond what you choose to share."),
            Triple(Icons.Default.Refresh, "Truly shared ownership.\nNo super-admin.", "Your group's legacy doesn't depend on a single super-admin. From the start, set transparent rules for adding and removing members — like via voting.")
        )
    }
    val totalPages = pages.size + 1 // +1 for differentiator page
    val pagerState = rememberPagerState(pageCount = { totalPages })
    val scope = rememberCoroutineScope()

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(bottom = 32.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            HorizontalPager(
                state = pagerState,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(300.dp)
            ) { page ->
                if (page < pages.size) {
                    Column(
                        modifier = Modifier.fillMaxSize(),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center
                    ) {
                        Icon(
                            pages[page].first,
                            contentDescription = null,
                            modifier = Modifier.size(64.dp),
                            tint = MaterialTheme.colorScheme.primary
                        )
                        Spacer(modifier = Modifier.height(20.dp))
                        Text(
                            pages[page].second,
                            style = MaterialTheme.typography.titleLarge,
                            fontWeight = FontWeight.Bold,
                            textAlign = TextAlign.Center
                        )
                        Spacer(modifier = Modifier.height(12.dp))
                        Text(
                            pages[page].third,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.padding(horizontal = 32.dp)
                        )
                    }
                } else {
                    // Differentiator page
                    Column(
                        modifier = Modifier.fillMaxSize().padding(horizontal = 32.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center
                    ) {
                        Text(
                            "What makes this different",
                            style = MaterialTheme.typography.titleLarge,
                            fontWeight = FontWeight.Bold
                        )
                        Spacer(modifier = Modifier.height(24.dp))
                        DiffRow("\u2705", Color(0xFF4CAF50), "Your content is encrypted", "like other apps")
                        Spacer(modifier = Modifier.height(16.dp))
                        DiffRow("\u2705", MaterialTheme.colorScheme.primary, "Your identity is protected", "unlike other apps")
                        Spacer(modifier = Modifier.height(16.dp))
                        DiffRow("\u2705", MaterialTheme.colorScheme.primary, "Your metadata can't be harvested", "unlike other apps")
                        Spacer(modifier = Modifier.height(16.dp))
                        DiffRow("\u2705", MaterialTheme.colorScheme.primary, "No single admin holds the keys", "unlike other apps")
                    }
                }
            }
            Spacer(modifier = Modifier.height(16.dp))
            // Page indicator dots
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                repeat(totalPages) { index ->
                    Surface(
                        shape = CircleShape,
                        color = if (index == pagerState.currentPage)
                            MaterialTheme.colorScheme.primary
                        else
                            MaterialTheme.colorScheme.outlineVariant,
                        modifier = Modifier.size(8.dp)
                    ) {}
                }
            }
            Spacer(modifier = Modifier.height(24.dp))
            Button(
                onClick = {
                    if (pagerState.currentPage < totalPages - 1) {
                        scope.launch { pagerState.animateScrollToPage(pagerState.currentPage + 1) }
                    } else {
                        onDismiss()
                    }
                },
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 32.dp)
            ) {
                val lastPageText = if (isRevisit) "Done" else "Get Started"
                Text(
                    if (pagerState.currentPage < totalPages - 1) "Next" else lastPageText,
                    fontWeight = FontWeight.SemiBold
                )
            }
            if (!isRevisit && onRestore != null) {
                Spacer(modifier = Modifier.height(8.dp))
                androidx.compose.material3.TextButton(
                    onClick = {
                        onRestore()
                        onDismiss()
                    },
                    modifier = Modifier.padding(horizontal = 32.dp)
                ) {
                    Text("Restore from Recovery Phrase", style = MaterialTheme.typography.labelMedium)
                }
            }
        }
    }
}

@Composable
private fun DiffRow(icon: String, iconColor: Color, text: String, detail: String) {
    Row(
        verticalAlignment = Alignment.Top,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier.fillMaxWidth()
    ) {
        Text(icon, color = iconColor, style = MaterialTheme.typography.titleMedium)
        Column {
            Text(text, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
            Text(detail, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

private fun relativeTimestamp(date: Date): String {
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
