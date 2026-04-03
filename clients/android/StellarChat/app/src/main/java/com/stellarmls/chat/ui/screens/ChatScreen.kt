package com.stellarmls.chat.ui.screens

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.Schedule
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.stellarmls.chat.blossom.BlossomClient
import com.stellarmls.chat.blossom.ImageCache
import com.stellarmls.chat.crypto.MediaCrypto
import com.stellarmls.chat.model.ChatMessage
import com.stellarmls.chat.model.MediaAttachment
import com.stellarmls.chat.model.MessageStatus
import com.stellarmls.chat.viewmodel.ChatViewModel
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
    onInvite: () -> Unit,
    onGroupInfo: () -> Unit = {},
    contactAliasStore: com.stellarmls.chat.persistence.ContactAliasStore? = null
) {
    val groupName = viewModel.groupName
    val listState = rememberLazyListState()

    // Track whether user is near the bottom of the list
    val isNearBottom = remember {
        derivedStateOf {
            val layoutInfo = listState.layoutInfo
            val lastVisible = layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
            lastVisible >= layoutInfo.totalItemsCount - 3
        }
    }

    // Auto-scroll only when user is near bottom
    LaunchedEffect(viewModel.messages.size) {
        if (viewModel.messages.isNotEmpty() && isNearBottom.value) {
            listState.animateScrollToItem(viewModel.messages.size - 1)
        }
    }

    val context = LocalContext.current
    val view = androidx.compose.ui.platform.LocalView.current
    val scope = rememberCoroutineScope()

    val photoPickerLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.PickVisualMedia()
    ) { uri ->
        viewModel.selectedImageUri = uri
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(groupName) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    IconButton(onClick = onGroupInfo) {
                        Icon(Icons.Filled.Group, contentDescription = "Group Info")
                    }
                    IconButton(onClick = onInvite) {
                        Icon(Icons.Filled.PersonAdd, contentDescription = "Invite Member")
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
            if (viewModel.messages.isEmpty()) {
                // Empty state
                Column(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxWidth(),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    @Suppress("DEPRECATION")
                    Icon(
                        Icons.Filled.Chat,
                        contentDescription = null,
                        modifier = Modifier.size(48.dp),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f)
                    )
                    Spacer(modifier = Modifier.height(12.dp))
                    Text(
                        "No messages yet",
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        "Send the first message to start the conversation",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f)
                    )
                }
            } else {
                Box(modifier = Modifier.weight(1f)) {
                    LazyColumn(
                        modifier = Modifier
                            .fillMaxSize()
                            .padding(horizontal = 16.dp),
                        state = listState,
                        verticalArrangement = Arrangement.spacedBy(2.dp)
                    ) {
                        itemsIndexed(viewModel.messages, key = { _, msg -> msg.id }) { index, message ->
                            // Date separator
                            if (shouldShowDateSeparator(viewModel.messages, index)) {
                                DateSeparator(message.timestamp)
                            }

                            // Unread message separator
                            if (message.id == viewModel.firstUnreadMessageID) {
                                UnreadSeparator()
                            }

                            val isGrouped = isGroupedWithPrevious(viewModel.messages, index)
                            val senderAlias = contactAliasStore?.displayName(message.senderPubkey)
                            Box(modifier = Modifier.animateItem()) {
                                MessageBubble(message, isGrouped, senderAlias = senderAlias, onRetry = {
                                    viewModel.retryMessage(message.id)
                                })
                            }
                        }
                    }

                    if (!isNearBottom.value) {
                        SmallFloatingActionButton(
                            onClick = {
                                scope.launch {
                                    if (viewModel.messages.isNotEmpty()) {
                                        listState.animateScrollToItem(viewModel.messages.size - 1)
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

            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(8.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                IconButton(
                    onClick = {
                        photoPickerLauncher.launch(
                            PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)
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
                    modifier = Modifier.weight(1f),
                    placeholder = { Text("Message...") },
                    singleLine = true
                )
                IconButton(
                    onClick = {
                        view.performHapticFeedback(android.view.HapticFeedbackConstants.CONFIRM)
                        viewModel.sendMessage()
                    },
                    enabled = viewModel.inputText.isNotBlank()
                ) {
                    Icon(
                        Icons.AutoMirrored.Filled.Send,
                        contentDescription = "Send",
                        tint = if (viewModel.inputText.isNotBlank())
                            MaterialTheme.colorScheme.primary
                        else
                            MaterialTheme.colorScheme.onSurfaceVariant
                    )
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
fun MessageBubble(message: ChatMessage, isGrouped: Boolean = false, senderAlias: String? = null, onRetry: (() -> Unit)? = null) {
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
                ImageBubbleContent(media = message.mediaAttachment)
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
                    .padding(horizontal = 12.dp, vertical = 8.dp)
            ) {
                Column {
                    Text(
                        text = message.text,
                        color = if (message.isMine) Color.White
                        else MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    Row(
                        modifier = Modifier.align(Alignment.End),
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
