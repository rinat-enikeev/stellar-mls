package chat.onym.android.ui.screens

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Pause
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.PlayCircle
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.BrokenImage
import androidx.compose.material.icons.filled.Chat
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.DoneAll
import androidx.compose.material.icons.filled.Error
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Image
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.FilterList
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SmallFloatingActionButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import chat.onym.android.ui.TestTags
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.material.icons.filled.HowToVote
import androidx.compose.material.icons.filled.People
import androidx.compose.material.icons.filled.PersonRemove
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.filled.Warning
import com.stellarmls.mls.SEPGroupType
import chat.onym.android.blossom.BlossomClient
import chat.onym.android.blossom.ImageCache
import chat.onym.android.blossom.VideoCache
import chat.onym.android.crypto.MediaCrypto
import chat.onym.android.model.ChatMessage
import chat.onym.android.model.MediaAttachment
import chat.onym.android.model.MessageStatus
import chat.onym.android.model.toHex
import chat.onym.android.viewmodel.ChatViewModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale
import java.util.concurrent.TimeUnit

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatScreen(
    viewModel: ChatViewModel,
    onBack: () -> Unit,
    onGroupInfo: () -> Unit = {},
    contactAliasStore: chat.onym.android.persistence.ContactAliasStore? = null,
    onUnpinEpoch: (() -> Unit)? = null,
    groupListViewModel: chat.onym.android.viewmodel.GroupListViewModel? = null,
    onForkGroup: (() -> Unit)? = null,
    pushNotificationsEnabled: Boolean = false,
    onEnablePushNotifications: (() -> Unit)? = null
) {
    val groupName = viewModel.groupName
    val listState = rememberLazyListState()

    // With reverseLayout=true, index 0 is the bottom (newest message)
    val isNearBottom = remember {
        derivedStateOf {
            listState.firstVisibleItemIndex <= 2
        }
    }

    // Auto-scroll only when user is near bottom
    LaunchedEffect(viewModel.messages.size) {
        if (viewModel.messages.isNotEmpty() && isNearBottom.value) {
            listState.animateScrollToItem(0)
        }
    }

    val context = LocalContext.current
    val view = androidx.compose.ui.platform.LocalView.current
    val scope = rememberCoroutineScope()

    val photoPickerLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.PickVisualMedia()
    ) { uri ->
        if (uri != null) {
            val mimeType = context.contentResolver.getType(uri)
            if (mimeType?.startsWith("video/") == true) {
                viewModel.selectedVideoUri = uri
            } else {
                viewModel.selectedImageUri = uri
            }
        }
    }

    Scaffold(
        modifier = Modifier.testTag(TestTags.Chat.Screen),
        topBar = {
            TopAppBar(
                title = { Text(groupName) },
                navigationIcon = {
                    IconButton(
                        onClick = onBack,
                        modifier = Modifier.testTag(TestTags.Chat.Back)
                    ) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    IconButton(
                        onClick = onGroupInfo,
                        modifier = Modifier.testTag(TestTags.Chat.GroupInfo)
                    ) {
                        Icon(Icons.Filled.Group, contentDescription = "Group Info")
                    }
                }
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .imePadding()
        ) {
            // Governance type banner — highlights the rules for this group.
            // Anarchy is surfaced as a warning (any member can kick/invite);
            // 1v1 / Democracy / Oligarchy get an informational notice.
            val governancePrefs = remember { context.getSharedPreferences("stellar_chat", Context.MODE_PRIVATE) }
            val groupIdForBanner = viewModel.group?.id
            val governanceDismissKey = remember(groupIdForBanner) { "governance_banner_dismissed_${groupIdForBanner ?: ""}" }
            var governanceBannerDismissed by remember(groupIdForBanner) {
                mutableStateOf(
                    groupIdForBanner != null &&
                    governancePrefs.getBoolean(governanceDismissKey, false)
                )
            }
            val governanceType = viewModel.group?.groupType
            if (governanceType != null && !governanceBannerDismissed && !chat.onym.android.viewmodel.GroupListViewModel.isDemoMode) {
                GovernanceBanner(
                    groupType = governanceType,
                    onDismiss = {
                        governanceBannerDismissed = true
                        if (groupIdForBanner != null) {
                            governancePrefs.edit().putBoolean(governanceDismissKey, true).apply()
                        }
                    }
                )
            }

            // Epoch pin banner
            val pinnedEpoch = viewModel.group?.pinnedEpoch
            val group = viewModel.group
            if (pinnedEpoch != null && group != null) {
                val snapshot = groupListViewModel?.epochSnapshots?.get(group.id)?.get(pinnedEpoch)
                val isPrivateBranch = snapshot != null && !snapshot.groupSecret.contentEquals(group.groupSecret)
                val bgColor = if (isPrivateBranch) Color(0xFFE8F5E9) else Color(0xFFFFF3E0)
                val fgColor = if (isPrivateBranch) Color(0xFF2E7D32) else Color(0xFFE65100)
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(bgColor)
                        .padding(horizontal = 16.dp, vertical = 8.dp)
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(
                            imageVector = if (isPrivateBranch)
                                Icons.Default.Lock else Icons.Default.FilterList,
                            contentDescription = null,
                            tint = fgColor,
                            modifier = Modifier.size(16.dp)
                        )
                        Spacer(Modifier.width(6.dp))
                        Text(
                            "Epoch $pinnedEpoch" + if (snapshot != null) " \u2022 ${snapshot.members.size} members" else "",
                            style = MaterialTheme.typography.bodySmall,
                            fontWeight = FontWeight.Medium,
                            color = fgColor,
                            modifier = Modifier.weight(1f)
                        )
                        TextButton(
                            onClick = { onUnpinEpoch?.invoke() }
                        ) {
                            Text("Unpin", color = fgColor)
                        }
                    }
                    Text(
                        if (isPrivateBranch)
                            "Private branch \u2014 only members from this epoch can read and write here"
                        else
                            "Filtered view \u2014 messages are still visible to all group members",
                        style = MaterialTheme.typography.labelSmall,
                        color = fgColor.copy(alpha = 0.7f)
                    )
                }
            }

            // Fork banner
            if (group?.forkedFromGroupID != null) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(Color(0xFFF3E5F5))
                        .padding(horizontal = 16.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = "\u2442",
                        style = MaterialTheme.typography.titleMedium,
                        color = Color(0xFF7B1FA2)
                    )
                    Spacer(Modifier.width(8.dp))
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            "Forked group",
                            style = MaterialTheme.typography.bodySmall,
                            fontWeight = FontWeight.Medium,
                            color = Color(0xFF7B1FA2)
                        )
                        group.forkedAtEpoch?.let { epoch ->
                            Text(
                                "Forked at epoch $epoch",
                                style = MaterialTheme.typography.labelSmall,
                                color = Color(0xFF7B1FA2).copy(alpha = 0.7f)
                            )
                        }
                    }
                }
            }

            // Welcome banner for first group
            val welcomePrefs = remember { context.getSharedPreferences("stellar_chat", Context.MODE_PRIVATE) }
            var hasSeenFirstGroupWelcome by remember {
                mutableStateOf(welcomePrefs.getBoolean("has_seen_first_group_welcome", false))
            }
            if (!hasSeenFirstGroupWelcome) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(MaterialTheme.colorScheme.primaryContainer)
                        .padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    Text("\uD83D\uDC4B", style = MaterialTheme.typography.bodyMedium)
                    val memberCount = viewModel.group?.members?.size ?: 0
                    Text(
                        if (memberCount <= 1) "Your group is ready. Messages are end-to-end encrypted — only members can read them."
                        else "You're in. Your identity is protected — members see a key, not your name.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                        modifier = Modifier.weight(1f)
                    )
                    IconButton(onClick = {
                        hasSeenFirstGroupWelcome = true
                        welcomePrefs.edit().putBoolean("has_seen_first_group_welcome", true).apply()
                    }, modifier = Modifier.size(24.dp)) {
                        Icon(Icons.Default.Close, contentDescription = "Dismiss", modifier = Modifier.size(16.dp))
                    }
                }
            }

            // Push notification banner — show once per session if not enabled
            var pushBannerDismissed by remember { mutableStateOf(false) }
            if (!pushNotificationsEnabled && !pushBannerDismissed && onEnablePushNotifications != null) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(MaterialTheme.colorScheme.primaryContainer)
                        .padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Icon(
                        Icons.Filled.Notifications,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onPrimaryContainer,
                        modifier = Modifier.size(18.dp)
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(
                        "Enable push notifications for this chat?",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                        modifier = Modifier.weight(1f)
                    )
                    TextButton(onClick = {
                        onEnablePushNotifications()
                        pushBannerDismissed = true
                    }) {
                        Text("Enable")
                    }
                    IconButton(
                        onClick = { pushBannerDismissed = true },
                        modifier = Modifier.size(32.dp)
                    ) {
                        Icon(
                            Icons.Default.Close,
                            contentDescription = "Dismiss",
                            modifier = Modifier.size(16.dp)
                        )
                    }
                }
            }

            // 1v1 peer-left banner: the other party has left the conversation.
            val currentGroup = viewModel.group
            if (currentGroup != null &&
                currentGroup.groupType == com.stellarmls.mls.SEPGroupType.ONE_ON_ONE &&
                currentGroup.members.size <= 1) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .padding(horizontal = 16.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Icon(
                        Icons.Filled.PersonRemove,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(16.dp)
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(
                        "Your peer left this conversation. New messages cannot be sent.",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }

            if (viewModel.messages.isEmpty()) {
                // Empty state — encryption context + invite prompt
                Column(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth()
                        .padding(32.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    Icon(
                        Icons.Filled.Lock,
                        contentDescription = null,
                        modifier = Modifier.size(36.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f)
                    )
                    Spacer(modifier = Modifier.height(16.dp))
                    Text(
                        "This conversation is encrypted",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.Medium
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        "Only group members can read messages here.\nInvite someone to start chatting.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center
                    )
                    if ((viewModel.group?.members?.size ?: 0) <= 1) {
                        Spacer(modifier = Modifier.height(16.dp))
                        OutlinedButton(onClick = { onGroupInfo() }) {
                            Icon(Icons.Default.Share, contentDescription = null, modifier = Modifier.size(16.dp))
                            Spacer(modifier = Modifier.size(4.dp))
                            Text("Share invite link")
                        }
                    }
                }
            } else {
                Box(modifier = Modifier.weight(1f)) {
                    LazyColumn(
                        modifier = Modifier
                            .fillMaxSize()
                            .padding(horizontal = 16.dp),
                        state = listState,
                        reverseLayout = true,
                        verticalArrangement = Arrangement.spacedBy(2.dp)
                    ) {
                        itemsIndexed(viewModel.messages.asReversed(), key = { _, msg -> msg.id }) { reversedIndex, message ->
                            val index = viewModel.messages.size - 1 - reversedIndex

                            // Unread message separator
                            if (message.id == viewModel.firstUnreadMessageID) {
                                UnreadSeparator()
                            }

                            if (message.isSystemMessage) {
                                Column(
                                    modifier = Modifier.fillMaxWidth(),
                                    horizontalAlignment = Alignment.CenterHorizontally
                                ) {
                                    when {
                                        message.text.startsWith("VOTE_PROPOSAL::") -> {
                                            val parts = message.text.split("::")
                                            val ballotID = parts.getOrNull(4) ?: ""
                                            val expirySeconds = parts.getOrNull(5)?.toLongOrNull()
                                            val gid = message.groupID
                                            val myHex = groupListViewModel?.keyManager?.blsPublicKey()?.toHex() ?: ""
                                            val tally = groupListViewModel?.ballotTally(gid, ballotID, expirySeconds) ?: (0 to 0)
                                            val quorum = groupListViewModel?.ballotQuorum(gid) ?: 0
                                            val finalized = groupListViewModel?.ballotFinalized(gid, ballotID) ?: false
                                            val prefixV1 = "VOTE_CAST::v1::$ballotID::"
                                            val prefixV2 = "VOTE_CAST::v2::$ballotID::"
                                            val myCastText = viewModel.messages
                                                .asSequence()
                                                .filter {
                                                    it.senderPubkey == myHex &&
                                                        (it.text.startsWith(prefixV1) || it.text.startsWith(prefixV2))
                                                }
                                                .lastOrNull()
                                                ?.text
                                            val myCast = when {
                                                myCastText == null -> null
                                                myCastText.startsWith(prefixV2) ->
                                                    myCastText.removePrefix(prefixV2).substringBefore("::")
                                                else -> myCastText.removePrefix(prefixV1)
                                            }
                                            val myChoice = when (myCast) { "yes" -> true; "no" -> false; else -> null }
                                            VoteProposalCard(
                                                rawPayload = message.text,
                                                groupID = gid,
                                                senderAlias = contactAliasStore?.displayName(message.senderPubkey),
                                                targetAliasResolver = { hex -> contactAliasStore?.displayName(hex) },
                                                myBlsHex = myHex,
                                                tally = tally,
                                                quorum = quorum,
                                                finalized = finalized,
                                                myChoice = myChoice,
                                                expirySeconds = expirySeconds,
                                                onCast = { yes -> groupListViewModel?.castVote(gid, ballotID, yes) },
                                                onFinalize = { targetHex ->
                                                    groupListViewModel?.finalizeBallot(gid, ballotID, targetHex)
                                                }
                                            )
                                        }
                                        message.text.startsWith("VOTE_CAST::") -> { /* aggregated into parent card */ }
                                        message.text.startsWith("DEMOCRACY_FINALIZED::") ->
                                            SystemMessage("Ballot finalized — majority removed the proposed member.")
                                        message.text.startsWith("ADMIN_PROMOTE::v1::") -> {
                                            val hex = message.text.removePrefix("ADMIN_PROMOTE::v1::")
                                            val alias = contactAliasStore?.displayName(hex) ?: (hex.take(10) + "\u2026")
                                            SystemMessage("$alias was promoted to admin.")
                                        }
                                        message.text.startsWith("ADMIN_DEMOTE::v1::") -> {
                                            val hex = message.text.removePrefix("ADMIN_DEMOTE::v1::")
                                            val alias = contactAliasStore?.displayName(hex) ?: (hex.take(10) + "\u2026")
                                            SystemMessage("$alias was demoted from admin.")
                                        }
                                        else -> SystemMessage(message.text)
                                    }
                                    if (message.text.contains("joined the group") &&
                                        (viewModel.group?.members?.size ?: 0) == 2) {
                                        Text(
                                            "Say hello — your messages are end-to-end encrypted",
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f)
                                        )
                                    }
                                }
                            } else {
                                val isGrouped = isGroupedWithPrevious(viewModel.messages, index)
                                val senderAlias = contactAliasStore?.displayName(message.senderPubkey)
                                val replyParent = viewModel.parentMessage(message)
                                SwipeToReply(onSwipe = { viewModel.replyingToMessage = message }) {
                                    Box(modifier = Modifier.animateItem()) {
                                        MessageBubble(
                                            message, isGrouped, senderAlias = senderAlias,
                                            replyParent = replyParent,
                                            onTapReply = { targetID ->
                                                val targetIndex = viewModel.messages.indexOfFirst { it.id == targetID }
                                                if (targetIndex >= 0) {
                                                    val reversedIdx = viewModel.messages.size - 1 - targetIndex
                                                    scope.launch { listState.animateScrollToItem(reversedIdx) }
                                                }
                                            },
                                            onRetry = { viewModel.retryMessage(message.id) }
                                        )
                                    }
                                }
                            }

                            // Date separator — rendered after message content so that
                            // reverseLayout places it visually above the message
                            if (shouldShowDateSeparator(viewModel.messages, index)) {
                                DateSeparator(message.timestamp)
                            }
                        }
                    }

                    if (!isNearBottom.value) {
                        SmallFloatingActionButton(
                            onClick = {
                                scope.launch {
                                    if (viewModel.messages.isNotEmpty()) {
                                        listState.animateScrollToItem(0)
                                    }
                                }
                            },
                            modifier = Modifier
                                .align(Alignment.BottomEnd)
                                .padding(end = 16.dp, bottom = 8.dp),
                            containerColor = MaterialTheme.colorScheme.primaryContainer,
                            contentColor = MaterialTheme.colorScheme.onPrimaryContainer
                        ) {
                            Icon(Icons.Filled.KeyboardArrowDown, contentDescription = "Scroll to latest")
                        }
                    }
                }
            }

            // Image preview bar
            viewModel.selectedImageUri?.let { uri ->
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 12.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    // Show a small preview from the URI
                    val previewBitmap = remember(uri) {
                        try {
                            val stream = context.contentResolver.openInputStream(uri)
                            val opts = BitmapFactory.Options().apply { inSampleSize = 8 }
                            BitmapFactory.decodeStream(stream, null, opts).also { stream?.close() }
                        } catch (_: Exception) { null }
                    }
                    if (previewBitmap != null) {
                        Image(
                            bitmap = previewBitmap.asImageBitmap(),
                            contentDescription = "Preview",
                            modifier = Modifier
                                .size(56.dp)
                                .clip(RoundedCornerShape(8.dp)),
                            contentScale = ContentScale.Crop
                        )
                    }
                    Spacer(modifier = Modifier.weight(1f))
                    if (viewModel.isSendingImage) {
                        CircularProgressIndicator(modifier = Modifier.size(24.dp))
                    } else {
                        TextButton(onClick = { viewModel.selectedImageUri = null }) {
                            Text("Cancel")
                        }
                        Button(onClick = {
                            scope.launch {
                                val bytes = withContext(Dispatchers.IO) {
                                    context.contentResolver.openInputStream(uri)?.readBytes()
                                }
                                if (bytes != null) {
                                    viewModel.sendImage(bytes)
                                }
                            }
                        }) {
                            Text("Send")
                        }
                    }
                }
            }

            // Video preview bar
            viewModel.selectedVideoUri?.let { uri ->
                val videoThumb = remember(uri) {
                    try {
                        val bytes = MediaCrypto.generateVideoThumbnail(context, uri)
                        bytes?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
                    } catch (_: Exception) { null }
                }
                val videoMeta = remember(uri) {
                    try {
                        MediaCrypto.videoMetadata(context, uri)
                    } catch (_: Exception) { null }
                }
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 12.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Box {
                        if (videoThumb != null) {
                            Image(
                                bitmap = videoThumb.asImageBitmap(),
                                contentDescription = "Video preview",
                                modifier = Modifier
                                    .size(56.dp)
                                    .clip(RoundedCornerShape(8.dp)),
                                contentScale = ContentScale.Crop
                            )
                        } else {
                            Box(
                                modifier = Modifier
                                    .size(56.dp)
                                    .clip(RoundedCornerShape(8.dp))
                                    .background(MaterialTheme.colorScheme.surfaceVariant),
                                contentAlignment = Alignment.Center
                            ) {
                                Icon(Icons.Filled.PlayCircle, contentDescription = null)
                            }
                        }
                        Icon(
                            Icons.Filled.PlayCircle,
                            contentDescription = "Video",
                            modifier = Modifier
                                .size(24.dp)
                                .align(Alignment.Center),
                            tint = Color.White
                        )
                        if (videoMeta != null) {
                            Text(
                                text = formatDuration(videoMeta.third),
                                style = MaterialTheme.typography.labelSmall,
                                color = Color.White,
                                modifier = Modifier
                                    .align(Alignment.BottomEnd)
                                    .padding(2.dp)
                                    .background(Color.Black.copy(alpha = 0.6f), RoundedCornerShape(4.dp))
                                    .padding(horizontal = 2.dp)
                            )
                        }
                    }
                    Spacer(modifier = Modifier.weight(1f))
                    if (viewModel.isSendingVideo) {
                        CircularProgressIndicator(modifier = Modifier.size(24.dp))
                    } else {
                        TextButton(onClick = { viewModel.selectedVideoUri = null }) {
                            Text("Cancel")
                        }
                        Button(onClick = {
                            viewModel.sendVideo(context)
                        }) {
                            Text("Send")
                        }
                    }
                }
            }

            if (!viewModel.isMember) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    Text(
                        text = "You were removed from this group",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    if (onForkGroup != null) {
                        Spacer(modifier = Modifier.height(8.dp))
                        Button(onClick = onForkGroup) {
                            Text("Fork Group")
                        }
                    }
                }
            } else {
                // Reply preview bar
                viewModel.replyingToMessage?.let { replyMessage ->
                    val replySenderAlias = contactAliasStore?.displayName(replyMessage.senderPubkey)
                    ReplyPreviewBar(
                        message = replyMessage,
                        senderAlias = replySenderAlias,
                        onDismiss = { viewModel.replyingToMessage = null }
                    )
                }

                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(8.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    IconButton(
                        onClick = {
                            photoPickerLauncher.launch(
                                PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo)
                            )
                        },
                        enabled = viewModel.hasBlossomServers
                    ) {
                        Icon(
                            Icons.Filled.Image,
                            contentDescription = "Pick image",
                            tint = if (viewModel.hasBlossomServers)
                                MaterialTheme.colorScheme.primary
                            else
                                MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f)
                        )
                    }
                    TextField(
                        value = viewModel.inputText,
                        onValueChange = { viewModel.inputText = it },
                        modifier = Modifier
                            .weight(1f)
                            .testTag(TestTags.Chat.MessageInput),
                        placeholder = { Text("Message...") },
                        singleLine = true
                    )
                    if (viewModel.inputText.isBlank()) {
                        chat.onym.android.ui.components.VoiceRecordButton(
                            hasBlossomServers = viewModel.hasBlossomServers,
                            onSend = { file -> viewModel.sendVoice(file) },
                            modifier = Modifier.padding(12.dp)
                        )
                    } else {
                        IconButton(
                            onClick = {
                                view.performHapticFeedback(android.view.HapticFeedbackConstants.CONFIRM)
                                viewModel.sendMessage()
                            },
                            modifier = Modifier.testTag(TestTags.Chat.SendButton)
                        ) {
                            Icon(
                                Icons.AutoMirrored.Filled.Send,
                                contentDescription = "Send",
                                tint = MaterialTheme.colorScheme.primary
                            )
                        }
                    }
                }
            }
        }
    }
}

private fun shouldShowDateSeparator(messages: List<ChatMessage>, index: Int): Boolean {
    if (index == 0) return true
    val prevCal = Calendar.getInstance().apply { time = messages[index - 1].timestamp }
    val currCal = Calendar.getInstance().apply { time = messages[index].timestamp }
    return prevCal.get(Calendar.YEAR) != currCal.get(Calendar.YEAR)
        || prevCal.get(Calendar.DAY_OF_YEAR) != currCal.get(Calendar.DAY_OF_YEAR)
}

private fun isGroupedWithPrevious(messages: List<ChatMessage>, index: Int): Boolean {
    if (index == 0) return false
    val prev = messages[index - 1]
    val curr = messages[index]
    return prev.senderPubkey == curr.senderPubkey
        && curr.timestamp.time - prev.timestamp.time < TimeUnit.MINUTES.toMillis(2)
}

@Composable
fun UnreadSeparator() {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        HorizontalDivider(modifier = Modifier.weight(1f), color = MaterialTheme.colorScheme.primary.copy(alpha = 0.4f))
        Text(
            text = "New Messages",
            style = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.SemiBold),
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.padding(horizontal = 8.dp)
        )
        HorizontalDivider(modifier = Modifier.weight(1f), color = MaterialTheme.colorScheme.primary.copy(alpha = 0.4f))
    }
}

@Composable
fun SystemMessage(text: String) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp, horizontal = 12.dp),
        contentAlignment = Alignment.Center
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f)
        )
    }
}

@Composable
fun DateSeparator(date: Date) {
    val text = formatDateSeparator(date)
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 8.dp),
        contentAlignment = Alignment.Center
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f)
        )
    }
}

private fun formatDateSeparator(date: Date): String {
    val cal = Calendar.getInstance()
    val today = Calendar.getInstance()
    cal.time = date

    return when {
        cal.get(Calendar.YEAR) == today.get(Calendar.YEAR)
            && cal.get(Calendar.DAY_OF_YEAR) == today.get(Calendar.DAY_OF_YEAR) -> "Today"
        cal.get(Calendar.YEAR) == today.get(Calendar.YEAR)
            && cal.get(Calendar.DAY_OF_YEAR) == today.get(Calendar.DAY_OF_YEAR) - 1 -> "Yesterday"
        else -> SimpleDateFormat("EEE, MMM d", Locale.getDefault()).format(date)
    }
}

@Composable
fun MessageBubble(message: ChatMessage, isGrouped: Boolean = false, senderAlias: String? = null, replyParent: ChatMessage? = null, onTapReply: ((String) -> Unit)? = null, onRetry: (() -> Unit)? = null) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = if (isGrouped) 0.dp else 4.dp),
        horizontalArrangement = if (message.isMine) Arrangement.End else Arrangement.Start,
        verticalAlignment = Alignment.Bottom
    ) {
        // Avatar for received messages
        if (!message.isMine) {
            if (!isGrouped) {
                AvatarBadge(message.senderPubkey, alias = senderAlias)
            } else {
                Spacer(modifier = Modifier.size(28.dp))
            }
            Spacer(modifier = Modifier.width(6.dp))
        }

        Column(
            horizontalAlignment = if (message.isMine) Alignment.End else Alignment.Start
        ) {
            if (!message.isMine && !isGrouped) {
                Text(
                    text = senderAlias ?: (message.senderPubkey.take(8) + "..."),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 8.dp, bottom = 2.dp)
                )
            }

            val shape = RoundedCornerShape(
            topStart = 16.dp,
            topEnd = 16.dp,
            bottomStart = if (message.isMine) 16.dp else 4.dp,
            bottomEnd = if (message.isMine) 4.dp else 16.dp
        )

        if (message.mediaAttachment != null) {
            Column(
                modifier = Modifier
                    .widthIn(max = 220.dp)
                    .clip(shape)
                    .background(
                        if (message.isMine) MaterialTheme.colorScheme.primary
                        else MaterialTheme.colorScheme.surfaceVariant
                    )
            ) {
                if (replyParent != null) {
                    QuotedReplyView(parent = replyParent, isMine = message.isMine,
                        onClick = { onTapReply?.invoke(replyParent.id) })
                } else if (message.replyToID != null) {
                    MissingReplyView(isMine = message.isMine)
                }
                when {
                    message.mediaAttachment.mimeType.startsWith("audio/") ->
                        VoiceBubbleContent(media = message.mediaAttachment, isMine = message.isMine)
                    message.mediaAttachment.mimeType.startsWith("video/") ->
                        VideoBubbleContent(media = message.mediaAttachment)
                    else ->
                        ImageBubbleContent(media = message.mediaAttachment)
                }
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 8.dp, vertical = 4.dp),
                    horizontalArrangement = Arrangement.End,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = formatMessageTimestamp(message.timestamp),
                        style = MaterialTheme.typography.labelSmall,
                        color = if (message.isMine) Color.White.copy(alpha = 0.7f)
                        else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f)
                    )
                    if (message.isMine) {
                        Spacer(modifier = Modifier.width(2.dp))
                        MessageStatusIcon(message.status, message.isMine, onRetry)
                    }
                }
            }
        } else {
            Box(
                modifier = Modifier
                    .widthIn(max = 280.dp)
                    .clip(shape)
                    .background(
                        if (message.isMine) MaterialTheme.colorScheme.primary
                        else MaterialTheme.colorScheme.surfaceVariant
                    )
            ) {
                Column {
                    if (replyParent != null) {
                        QuotedReplyView(parent = replyParent, isMine = message.isMine,
                            onClick = { onTapReply?.invoke(replyParent.id) })
                    } else if (message.replyToID != null) {
                        MissingReplyView(isMine = message.isMine)
                    }
                    Text(
                        text = message.text,
                        color = if (message.isMine) Color.White
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)
                    )
                    Row(
                        modifier = Modifier.align(Alignment.End).padding(horizontal = 12.dp, vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(2.dp)
                    ) {
                        Text(
                            text = formatMessageTimestamp(message.timestamp),
                            style = MaterialTheme.typography.labelSmall,
                            color = if (message.isMine) Color.White.copy(alpha = 0.7f)
                            else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f)
                        )
                        if (message.isMine) {
                            MessageStatusIcon(message.status, message.isMine, onRetry)
                        }
                    }
                }
            }
        }
        } // Column
    } // Row
}

@Composable
fun ImageBubbleContent(media: MediaAttachment) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val imageCache: ImageCache = remember { ImageCache.getInstance(context) }
    var bitmap by remember(media.blobHash) { mutableStateOf(imageCache.getBitmap(media.blobHash)) }
    var isLoading by remember(media.blobHash) { mutableStateOf(false) }
    var failed by remember(media.blobHash) { mutableStateOf(false) }
    var showFullScreen by remember { mutableStateOf(false) }

    // Try to decrypt thumbnail for immediate display
    val thumbnailBitmap = remember(media.blobHash) {
        media.encryptedThumbnail?.let { encThumb ->
            try {
                val plain = MediaCrypto.decryptMedia(encThumb, media.fileKey)
                BitmapFactory.decodeByteArray(plain, 0, plain.size)
            } catch (_: Exception) { null }
        }
    }

    // Load full image on appear
    LaunchedEffect(media.blobHash) {
        if (bitmap != null) return@LaunchedEffect
        isLoading = true
        try {
            val encrypted = withContext(Dispatchers.IO) {
                BlossomClient.download(media.blobHash, media.blossomServers)
            }
            val plain = withContext(Dispatchers.Default) {
                MediaCrypto.decryptMedia(encrypted, media.fileKey)
            }
            val bmp = BitmapFactory.decodeByteArray(plain, 0, plain.size)
            if (bmp != null) {
                imageCache.store(media.blobHash, bmp, plain)
                bitmap = bmp
            } else {
                failed = true
            }
        } catch (_: Exception) {
            failed = true
        }
        isLoading = false
    }

    val displayBitmap = bitmap ?: thumbnailBitmap
    val aspectRatio = if (media.width > 0 && media.height > 0)
        media.width.toFloat() / media.height.toFloat() else 1f

    if (displayBitmap != null) {
        Image(
            bitmap = displayBitmap.asImageBitmap(),
            contentDescription = "Image",
            modifier = Modifier
                .widthIn(max = 220.dp)
                .aspectRatio(aspectRatio.coerceIn(0.5f, 2f))
                .clickable {
                    if (bitmap != null) {
                        showFullScreen = true
                    } else if (!isLoading) {
                        scope.launch {
                            isLoading = true
                            try {
                                val enc = withContext(Dispatchers.IO) {
                                    BlossomClient.download(media.blobHash, media.blossomServers)
                                }
                                val plain = withContext(Dispatchers.Default) {
                                    MediaCrypto.decryptMedia(enc, media.fileKey)
                                }
                                val bmp = BitmapFactory.decodeByteArray(plain, 0, plain.size)
                                if (bmp != null) {
                                    imageCache.store(media.blobHash, bmp, plain)
                                    bitmap = bmp
                                }
                            } catch (_: Exception) { }
                            isLoading = false
                        }
                    }
                },
            contentScale = ContentScale.Crop
        )
        if (isLoading && bitmap == null) {
            Box(
                modifier = Modifier
                    .widthIn(max = 220.dp)
                    .aspectRatio(aspectRatio.coerceIn(0.5f, 2f)),
                contentAlignment = Alignment.Center
            ) {
                CircularProgressIndicator(modifier = Modifier.size(24.dp), color = Color.White)
            }
        }
    } else if (failed) {
        Box(
            modifier = Modifier
                .widthIn(max = 220.dp)
                .aspectRatio(aspectRatio.coerceIn(0.5f, 2f))
                .background(MaterialTheme.colorScheme.surfaceVariant),
            contentAlignment = Alignment.Center
        ) {
            Icon(Icons.Filled.BrokenImage, contentDescription = "Failed", tint = MaterialTheme.colorScheme.error)
        }
    } else {
        Box(
            modifier = Modifier
                .widthIn(max = 220.dp)
                .aspectRatio(aspectRatio.coerceIn(0.5f, 2f))
                .background(MaterialTheme.colorScheme.surfaceVariant),
            contentAlignment = Alignment.Center
        ) {
            if (isLoading) {
                CircularProgressIndicator(modifier = Modifier.size(24.dp))
            } else {
                Icon(Icons.Filled.Image, contentDescription = "Image", tint = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }

    if (showFullScreen && bitmap != null) {
        FullScreenImageViewer(bitmap = bitmap!!, onDismiss = { showFullScreen = false })
    }
}

@Composable
fun FullScreenImageViewer(bitmap: Bitmap, onDismiss: () -> Unit) {
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false)
    ) {
        var scale by remember { mutableFloatStateOf(1f) }
        var offset by remember { mutableStateOf(Offset.Zero) }
        var dragOffsetY by remember { mutableFloatStateOf(0f) }
        val backgroundAlpha = 1f - (kotlin.math.abs(dragOffsetY) / 900f).coerceAtMost(0.6f)

        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color.Black.copy(alpha = backgroundAlpha))
        ) {
            Image(
                bitmap = bitmap.asImageBitmap(),
                contentDescription = "Full screen image",
                modifier = Modifier
                    .fillMaxSize()
                    .graphicsLayer(
                        scaleX = scale,
                        scaleY = scale,
                        translationX = offset.x,
                        translationY = offset.y + dragOffsetY
                    )
                    .pointerInput(Unit) {
                        detectTransformGestures { _, pan, zoom, _ ->
                            val newScale = (scale * zoom).coerceIn(1f, 5f)
                            if (newScale > 1f) {
                                scale = newScale
                                offset = Offset(offset.x + pan.x, offset.y + pan.y)
                            } else {
                                scale = newScale
                                // When at 1x, vertical drag dismisses
                                dragOffsetY += pan.y
                            }
                        }
                    }
                    .pointerInput(Unit) {
                        detectTapGestures(
                            onDoubleTap = {
                                if (scale > 1f) {
                                    scale = 1f
                                    offset = Offset.Zero
                                    dragOffsetY = 0f
                                } else {
                                    scale = 2.5f
                                }
                            }
                        )
                    },
                contentScale = ContentScale.Fit
            )

            // Check dismiss after gesture ends — use a side effect on dragOffsetY
            LaunchedEffect(dragOffsetY) {
                // Only act when finger lifts (stable value)
                if (dragOffsetY != 0f && scale <= 1f) {
                    kotlinx.coroutines.delay(50)
                    if (kotlin.math.abs(dragOffsetY) > 400f) {
                        onDismiss()
                    }
                }
            }

            IconButton(
                onClick = onDismiss,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(16.dp)
            ) {
                Icon(
                    Icons.Filled.Close,
                    contentDescription = "Close",
                    tint = Color.White,
                    modifier = Modifier.size(28.dp)
                )
            }
        }
    }
}

@Composable
fun VideoBubbleContent(media: MediaAttachment) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var isLoading by remember(media.blobHash) { mutableStateOf(false) }
    var showFullScreen by remember { mutableStateOf(false) }
    var videoFile by remember(media.blobHash) { mutableStateOf<java.io.File?>(
        VideoCache.getInstance(context).getCachedFile(media.blobHash)
    ) }

    // Decrypt thumbnail for display
    val thumbnailBitmap = remember(media.blobHash) {
        media.encryptedThumbnail?.let { encThumb ->
            try {
                val plain = MediaCrypto.decryptMedia(encThumb, media.fileKey)
                BitmapFactory.decodeByteArray(plain, 0, plain.size)
            } catch (_: Exception) { null }
        }
    }

    val aspectRatio = if (media.width > 0 && media.height > 0)
        media.width.toFloat() / media.height.toFloat() else 1f

    Box(
        modifier = Modifier
            .widthIn(max = 220.dp)
            .aspectRatio(aspectRatio.coerceIn(0.5f, 2f))
            .clickable {
                if (videoFile != null) {
                    showFullScreen = true
                } else if (!isLoading) {
                    scope.launch {
                        isLoading = true
                        try {
                            val encrypted = withContext(Dispatchers.IO) {
                                BlossomClient.download(media.blobHash, media.blossomServers)
                            }
                            val plain = withContext(Dispatchers.Default) {
                                MediaCrypto.decryptMedia(encrypted, media.fileKey)
                            }
                            val file = VideoCache.getInstance(context).store(media.blobHash, plain)
                            videoFile = file
                            showFullScreen = true
                        } catch (_: Exception) { }
                        isLoading = false
                    }
                }
            }
    ) {
        if (thumbnailBitmap != null) {
            Image(
                bitmap = thumbnailBitmap.asImageBitmap(),
                contentDescription = "Video",
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop
            )
        } else {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(MaterialTheme.colorScheme.surfaceVariant)
            )
        }

        // Play icon overlay
        if (!isLoading) {
            Icon(
                Icons.Filled.PlayCircle,
                contentDescription = "Play video",
                modifier = Modifier
                    .size(48.dp)
                    .align(Alignment.Center),
                tint = Color.White.copy(alpha = 0.9f)
            )
        } else {
            CircularProgressIndicator(
                modifier = Modifier
                    .size(32.dp)
                    .align(Alignment.Center),
                color = Color.White
            )
        }

        // Duration label
        media.duration?.let { dur ->
            Text(
                text = formatDuration(dur),
                style = MaterialTheme.typography.labelSmall,
                color = Color.White,
                modifier = Modifier
                    .align(Alignment.BottomEnd)
                    .padding(4.dp)
                    .background(Color.Black.copy(alpha = 0.6f), RoundedCornerShape(4.dp))
                    .padding(horizontal = 4.dp, vertical = 2.dp)
            )
        }
    }

    if (showFullScreen && videoFile != null) {
        FullScreenVideoPlayer(videoFile = videoFile!!, onDismiss = { showFullScreen = false })
    }
}

@Composable
fun FullScreenVideoPlayer(videoFile: java.io.File, onDismiss: () -> Unit) {
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false)
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color.Black)
        ) {
            androidx.compose.ui.viewinterop.AndroidView(
                factory = { ctx ->
                    android.widget.VideoView(ctx).apply {
                        setVideoURI(android.net.Uri.fromFile(videoFile))
                        val mc = android.widget.MediaController(ctx)
                        mc.setAnchorView(this)
                        setMediaController(mc)
                        setOnPreparedListener { start() }
                    }
                },
                modifier = Modifier
                    .fillMaxSize()
                    .align(Alignment.Center)
            )

            IconButton(
                onClick = onDismiss,
                modifier = Modifier
                    .align(Alignment.TopEnd)
                    .padding(16.dp)
            ) {
                Icon(
                    Icons.Filled.Close,
                    contentDescription = "Close",
                    tint = Color.White,
                    modifier = Modifier.size(28.dp)
                )
            }
        }
    }
}

private object ActiveVoicePlayback {
    private var stopCurrent: (() -> Unit)? = null

    fun register(stop: () -> Unit) {
        stopCurrent?.invoke()
        stopCurrent = stop
    }

    fun unregister(stop: () -> Unit) {
        if (stopCurrent === stop) stopCurrent = null
    }
}

@Composable
fun VoiceBubbleContent(media: MediaAttachment, isMine: Boolean) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var isLoading by remember(media.blobHash) { mutableStateOf(false) }
    var isPlaying by remember(media.blobHash) { mutableStateOf(false) }
    var progress by remember(media.blobHash) { mutableFloatStateOf(0f) }
    var audioData by remember(media.blobHash) { mutableStateOf<ByteArray?>(
        chat.onym.android.blossom.AudioCache.get(media.blobHash)
    ) }
    val mediaPlayer = remember { mutableStateOf<android.media.MediaPlayer?>(null) }

    val stopPlayback: () -> Unit = remember(media.blobHash) {
        {
            mediaPlayer.value?.let {
                if (it.isPlaying) it.stop()
                it.release()
            }
            mediaPlayer.value = null
            isPlaying = false
            progress = 0f
        }
    }

    androidx.compose.runtime.DisposableEffect(media.blobHash) {
        onDispose {
            ActiveVoicePlayback.unregister(stopPlayback)
            mediaPlayer.value?.release()
            mediaPlayer.value = null
        }
    }

    Row(
        modifier = Modifier
            .width(200.dp)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        IconButton(
            onClick = {
                if (isPlaying) {
                    mediaPlayer.value?.pause()
                    isPlaying = false
                } else if (audioData != null) {
                    ActiveVoicePlayback.register(stopPlayback)
                    playAudio(mediaPlayer, audioData!!, context,
                        onProgress = { progress = it },
                        onComplete = { isPlaying = false; progress = 0f })
                    isPlaying = true
                } else {
                    scope.launch {
                        isLoading = true
                        try {
                            val encrypted = withContext(Dispatchers.IO) {
                                BlossomClient.download(media.blobHash, media.blossomServers)
                            }
                            val plain = withContext(Dispatchers.Default) {
                                MediaCrypto.decryptMedia(encrypted, media.fileKey)
                            }
                            chat.onym.android.blossom.AudioCache.put(media.blobHash, plain)
                            audioData = plain
                            isLoading = false
                            ActiveVoicePlayback.register(stopPlayback)
                            playAudio(mediaPlayer, plain, context,
                                onProgress = { progress = it },
                                onComplete = { isPlaying = false; progress = 0f })
                            isPlaying = true
                        } catch (_: Exception) {
                            isLoading = false
                        }
                    }
                }
            },
            modifier = Modifier.size(36.dp)
        ) {
            if (isLoading) {
                CircularProgressIndicator(modifier = Modifier.size(24.dp))
            } else {
                Icon(
                    if (isPlaying) Icons.Filled.Pause else Icons.Filled.PlayArrow,
                    contentDescription = if (isPlaying) "Pause" else "Play",
                    tint = if (isMine) Color.White else MaterialTheme.colorScheme.primary
                )
            }
        }

        Column(modifier = Modifier.weight(1f)) {
            LinearProgressIndicator(
                progress = { progress },
                modifier = Modifier
                    .fillMaxWidth()
                    .height(4.dp)
                    .clip(RoundedCornerShape(2.dp)),
                color = if (isMine) Color.White else MaterialTheme.colorScheme.primary,
                trackColor = if (isMine) Color.White.copy(alpha = 0.3f)
                    else MaterialTheme.colorScheme.primary.copy(alpha = 0.2f)
            )
            Spacer(modifier = Modifier.height(4.dp))
            Text(
                text = formatDuration(media.duration ?: 0),
                style = MaterialTheme.typography.labelSmall,
                color = if (isMine) Color.White.copy(alpha = 0.7f)
                    else MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

private fun playAudio(
    playerState: androidx.compose.runtime.MutableState<android.media.MediaPlayer?>,
    data: ByteArray,
    context: android.content.Context,
    onProgress: (Float) -> Unit,
    onComplete: () -> Unit
) {
    playerState.value?.release()
    val tempFile = java.io.File(context.cacheDir, "voice_playback_${System.currentTimeMillis()}.m4a")
    tempFile.writeBytes(data)
    val player = android.media.MediaPlayer().apply {
        setDataSource(tempFile.absolutePath)
        prepare()
        start()
        setOnCompletionListener {
            onComplete()
            tempFile.delete()
        }
    }
    playerState.value = player
    kotlinx.coroutines.CoroutineScope(Dispatchers.Main).launch {
        while (player.isPlaying) {
            onProgress(player.currentPosition.toFloat() / player.duration)
            kotlinx.coroutines.delay(100)
        }
    }
}

private fun formatDuration(seconds: Int): String {
    val mins = seconds / 60
    val secs = seconds % 60
    return String.format("%d:%02d", mins, secs)
}

@Composable
fun AvatarBadge(pubkey: String, alias: String? = null) {
    val palette = listOf(
        Color(0xFFE53935), Color(0xFFFF9800), Color(0xFFFDD835),
        Color(0xFF43A047), Color(0xFF00897B), Color(0xFF1E88E5),
        Color(0xFF3949AB), Color(0xFF8E24AA), Color(0xFFD81B60),
        Color(0xFF6D4C41)
    )
    val index = pubkey.take(2).toIntOrNull(16) ?: 0
    val color = palette[index % palette.size]
    val initials = if (alias != null && alias.isNotEmpty()) {
        alias.first().uppercase()
    } else {
        pubkey.take(2).uppercase()
    }

    Surface(
        shape = CircleShape,
        color = color,
        modifier = Modifier.size(28.dp)
    ) {
        Box(contentAlignment = Alignment.Center) {
            Text(
                text = initials,
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.Bold,
                color = Color.White
            )
        }
    }
}

@Composable
private fun MessageStatusIcon(status: MessageStatus, isMine: Boolean, onRetry: (() -> Unit)? = null) {
    val tint = if (isMine) Color.White.copy(alpha = 0.7f)
    else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f)
    when (status) {
        MessageStatus.SENDING -> Icon(
            Icons.Filled.Schedule,
            contentDescription = "Sending",
            modifier = Modifier.size(12.dp),
            tint = tint
        )
        MessageStatus.SENT -> Icon(
            Icons.Filled.Check,
            contentDescription = "Sent",
            modifier = Modifier.size(12.dp),
            tint = tint
        )
        MessageStatus.DELIVERED -> Icon(
            Icons.Filled.DoneAll,
            contentDescription = "Delivered",
            modifier = Modifier.size(12.dp),
            tint = if (isMine) Color.White else MaterialTheme.colorScheme.primary
        )
        MessageStatus.FAILED -> Icon(
            Icons.Filled.Error,
            contentDescription = "Tap to retry",
            modifier = Modifier
                .size(12.dp)
                .then(if (onRetry != null) Modifier.clickable { onRetry() } else Modifier),
            tint = Color.Red
        )
    }
}

private fun formatMessageTimestamp(date: Date): String {
    val cal = Calendar.getInstance()
    val today = Calendar.getInstance()
    cal.time = date
    return if (cal.get(Calendar.YEAR) == today.get(Calendar.YEAR)
        && cal.get(Calendar.DAY_OF_YEAR) == today.get(Calendar.DAY_OF_YEAR)
    ) {
        SimpleDateFormat("h:mm a", Locale.getDefault()).format(date)
    } else {
        SimpleDateFormat("MMM d, h:mm a", Locale.getDefault()).format(date)
    }
}

// MARK: - Swipe to Reply

@Composable
fun SwipeToReply(onSwipe: () -> Unit, content: @Composable () -> Unit) {
    val view = LocalView.current
    var offsetX by remember { mutableFloatStateOf(0f) }
    val threshold = 60.dp
    val thresholdPx = with(androidx.compose.ui.platform.LocalDensity.current) { threshold.toPx() }

    Box(
        modifier = Modifier
            .offset(x = with(androidx.compose.ui.platform.LocalDensity.current) { offsetX.toDp() })
            .pointerInput(Unit) {
                detectHorizontalDragGestures(
                    onDragEnd = {
                        if (offsetX >= thresholdPx) {
                            view.performHapticFeedback(android.view.HapticFeedbackConstants.CONTEXT_CLICK)
                            onSwipe()
                        }
                        offsetX = 0f
                    },
                    onDragCancel = { offsetX = 0f },
                    onHorizontalDrag = { _, dragAmount ->
                        offsetX = (offsetX + dragAmount).coerceIn(0f, thresholdPx + 20.dp.toPx())
                    }
                )
            }
    ) {
        content()
    }
}

// MARK: - Reply Preview Bar

@Composable
fun ReplyPreviewBar(message: ChatMessage, senderAlias: String?, onDismiss: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f))
            .padding(horizontal = 16.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(
            modifier = Modifier
                .width(3.dp)
                .height(32.dp)
                .clip(RoundedCornerShape(2.dp))
                .background(MaterialTheme.colorScheme.primary)
        )
        Spacer(modifier = Modifier.width(8.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = senderAlias ?: (message.senderPubkey.take(8) + "..."),
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.primary
            )
            val previewText = when {
                message.mediaAttachment?.mimeType?.startsWith("audio/") == true -> "Voice message"
                message.mediaAttachment?.mimeType?.startsWith("video/") == true -> "Video"
                message.mediaAttachment != null -> "Photo"
                else -> message.text
            }
            Text(
                text = previewText,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
        }
        IconButton(onClick = onDismiss, modifier = Modifier.size(24.dp)) {
            Icon(
                Icons.Filled.Close,
                contentDescription = "Cancel reply",
                modifier = Modifier.size(16.dp),
                tint = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

// MARK: - Quoted Reply

@Composable
fun QuotedReplyView(parent: ChatMessage, isMine: Boolean, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable { onClick() }
            .padding(horizontal = 10.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(
            modifier = Modifier
                .width(2.5.dp)
                .height(24.dp)
                .clip(RoundedCornerShape(1.5.dp))
                .background(
                    if (isMine) Color.White.copy(alpha = 0.5f)
                    else MaterialTheme.colorScheme.primary.copy(alpha = 0.6f)
                )
        )
        Spacer(modifier = Modifier.width(4.dp))
        Column {
            Text(
                text = parent.senderPubkey.take(8) + "...",
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.SemiBold,
                color = if (isMine) Color.White.copy(alpha = 0.8f)
                else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.8f)
            )
            val previewText = when {
                parent.mediaAttachment?.mimeType?.startsWith("audio/") == true -> "Voice message"
                parent.mediaAttachment?.mimeType?.startsWith("video/") == true -> "Video"
                parent.mediaAttachment != null -> "Photo"
                else -> parent.text
            }
            Text(
                text = previewText,
                style = MaterialTheme.typography.labelSmall,
                color = if (isMine) Color.White.copy(alpha = 0.7f)
                else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
        }
    }
}

@Composable
fun GovernanceBanner(groupType: SEPGroupType, onDismiss: () -> Unit = {}) {
    val (title, subtitle, icon, tint) = when (groupType) {
        SEPGroupType.ANARCHY -> GovernanceBannerSpec(
            "Anarchy group",
            "Any member can kick or invite anyone, unilaterally. Choose members carefully.",
            Icons.Filled.Warning,
            Color(0xFFE65100)
        )
        SEPGroupType.ONE_ON_ONE -> GovernanceBannerSpec(
            "1-on-1 chat",
            "Membership is frozen \u2014 no one else can be added. Leaving ends the chat.",
            Icons.Filled.People,
            Color(0xFF1565C0)
        )
        SEPGroupType.DEMOCRACY -> GovernanceBannerSpec(
            "Democracy group",
            "Kicks and invites require a majority vote from current members.",
            Icons.Filled.HowToVote,
            Color(0xFF6A1B9A)
        )
        SEPGroupType.OLIGARCHY -> GovernanceBannerSpec(
            "Oligarchy group",
            "Only admins can kick members or approve new invites.",
            Icons.Filled.Star,
            Color(0xFFF9A825)
        )
    }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(tint.copy(alpha = 0.12f))
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(20.dp))
        Spacer(Modifier.width(8.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodySmall, fontWeight = FontWeight.SemiBold)
            Text(subtitle, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        IconButton(onClick = onDismiss, modifier = Modifier.size(28.dp)) {
            Icon(Icons.Default.Close, contentDescription = "Dismiss", modifier = Modifier.size(14.dp))
        }
    }
}

private data class GovernanceBannerSpec(
    val title: String,
    val subtitle: String,
    val icon: androidx.compose.ui.graphics.vector.ImageVector,
    val tint: Color
)

/** Inline ballot card for a `VOTE_PROPOSAL::v1::...` system message. Yes/No
 *  buttons broadcast `VOTE_CAST::v1::<ballotID>::<yes|no>` to the whole group;
 *  the card aggregates responses to show a live tally and surfaces a
 *  Finalize action once Yes reaches quorum. On-chain proof enforcement
 *  arrives with the Democracy circuit ceremony. */
@Composable
fun VoteProposalCard(
    rawPayload: String,
    groupID: String,
    senderAlias: String?,
    targetAliasResolver: (String) -> String?,
    myBlsHex: String,
    tally: Pair<Int, Int>,
    quorum: Int,
    finalized: Boolean,
    myChoice: Boolean?,
    expirySeconds: Long?,
    onCast: (Boolean) -> Unit,
    onFinalize: (String) -> Unit
) {
    val parts = rawPayload.split("::")
    val valid = parts.size >= 5 && parts[0] == "VOTE_PROPOSAL" && parts[1] == "v1" && parts[2] == "remove"
    if (!valid) {
        SystemMessage("Ballot (unrecognized)")
        return
    }
    val targetHex = parts[3]
    val ballotID = parts[4]
    val targetLabel = targetAliasResolver(targetHex)
        ?: (targetHex.take(12) + "\u2026" + targetHex.takeLast(6))
    val (yesCount, noCount) = tally
    val passed = yesCount >= quorum
    val expired = expirySeconds != null && System.currentTimeMillis() / 1000L > expirySeconds
    val tint = Color(0xFF6A1B9A)
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 4.dp)
            .clip(RoundedCornerShape(10.dp))
            .background(tint.copy(alpha = 0.08f))
            .padding(10.dp)
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(
                Icons.Filled.HowToVote,
                contentDescription = null,
                tint = tint,
                modifier = Modifier.size(16.dp)
            )
            Spacer(Modifier.width(6.dp))
            Text("Ballot", style = MaterialTheme.typography.bodySmall, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.weight(1f))
            Text("#$ballotID", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Spacer(Modifier.height(4.dp))
        Text(
            "${senderAlias ?: "A member"} proposed to remove:",
            style = MaterialTheme.typography.labelSmall
        )
        Text(targetLabel, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Spacer(Modifier.height(6.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedButton(
                onClick = { onCast(true) },
                enabled = !finalized && !expired,
                modifier = Modifier.weight(1f)
            ) {
                Text(
                    if (myChoice == true) "\u2713 Yes $yesCount" else "Yes $yesCount",
                    style = MaterialTheme.typography.labelSmall
                )
            }
            OutlinedButton(
                onClick = { onCast(false) },
                enabled = !finalized && !expired,
                modifier = Modifier.weight(1f)
            ) {
                Text(
                    if (myChoice == false) "\u2715 No $noCount" else "No $noCount",
                    style = MaterialTheme.typography.labelSmall
                )
            }
            Text(
                "quorum $quorum",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
        if (expirySeconds != null) {
            Spacer(Modifier.height(4.dp))
            val nowSec = System.currentTimeMillis() / 1000L
            val label = if (expired) {
                "Expired " + formatRelativeSeconds(nowSec - expirySeconds, past = true)
            } else {
                "Expires " + formatRelativeSeconds(expirySeconds - nowSec, past = false)
            }
            Text(
                label,
                style = MaterialTheme.typography.labelSmall,
                color = if (expired) Color(0xFFB00020) else MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
        Spacer(Modifier.height(4.dp))
        when {
            finalized -> Text(
                "Ballot finalized \u2014 target removed.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            expired -> Text(
                "Ballot expired without reaching quorum.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            passed -> Button(
                onClick = { onFinalize(targetHex) },
                colors = ButtonDefaults.buttonColors(containerColor = tint)
            ) {
                Text("Finalize removal", style = MaterialTheme.typography.labelSmall)
            }
            else -> Text(
                "On-chain voting activates once the Democracy circuit is deployed.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

private fun formatRelativeSeconds(seconds: Long, past: Boolean): String {
    val abs = kotlin.math.abs(seconds)
    val suffix = if (past) " ago" else ""
    return when {
        abs < 60 -> if (past) "just now" else "in <1 min"
        abs < 3600 -> "${abs / 60} min$suffix"
        abs < 86400 -> "${abs / 3600} h$suffix"
        else -> "${abs / 86400} d$suffix"
    }
}

@Composable
fun MissingReplyView(isMine: Boolean) {
    Row(
        modifier = Modifier.padding(horizontal = 10.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Box(
            modifier = Modifier
                .width(2.5.dp)
                .height(24.dp)
                .clip(RoundedCornerShape(1.5.dp))
                .background(
                    if (isMine) Color.White.copy(alpha = 0.3f)
                    else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.3f)
                )
        )
        Spacer(modifier = Modifier.width(4.dp))
        Text(
            text = "Original message",
            style = MaterialTheme.typography.labelSmall,
            color = if (isMine) Color.White.copy(alpha = 0.6f)
            else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f)
        )
    }
}
