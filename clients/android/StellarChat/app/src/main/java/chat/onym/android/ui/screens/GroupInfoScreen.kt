package chat.onym.android.ui.screens

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
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
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CallSplit
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.HowToVote
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.NotificationsOff
import androidx.compose.material.icons.filled.People
import androidx.compose.material.icons.filled.PersonRemove
import androidx.compose.material.icons.filled.PushPin
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.VerifiedUser
import androidx.compose.material.icons.filled.VpnKey
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import chat.onym.android.model.ChatGroup
import chat.onym.android.model.EpochSnapshot
import chat.onym.android.model.toHex
import chat.onym.android.persistence.ContactAliasStore
import chat.onym.android.persistence.StellarChatDao
import chat.onym.android.ui.components.GroupAccent
import chat.onym.android.ui.components.QRScannerView
import chat.onym.android.ui.components.SettingsIconBox
import com.stellarmls.mls.SEPGroupMemberLeaf
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun GroupInfoScreen(
    group: ChatGroup,
    myBlsPubkey: ByteArray,
    onRemoveMember: (ByteArray, (Result<Unit>) -> Unit) -> Unit,
    onRotateKey: () -> Unit = {},
    onRenameGroup: (String) -> Unit = {},
    onSetGroupAvatar: (ByteArray?) -> Unit = {},
    epochSnapshots: Map<Long, EpochSnapshot> = emptyMap(),
    onPinEpoch: (Long) -> Unit = {},
    onUnpinEpoch: () -> Unit = {},
    canForkGroup: Boolean = false,
    onForkGroup: () -> Unit = {},
    onSendInvitation: ((recipientKeyHex: String, onResult: (Result<Unit>) -> Unit) -> Unit)? = null,
    inviteLink: String? = null,
    onTogglePushNotifications: ((Boolean) -> Unit)? = null,
    onProposeRemovalVote: ((targetPubkeyHex: String) -> Unit)? = null,
    onPromoteToAdmin: ((targetPubkeyHex: String) -> Unit)? = null,
    onDemoteFromAdmin: ((targetPubkeyHex: String) -> Unit)? = null,
    onSearchMessages: () -> Unit = {},
    contactAliasStore: ContactAliasStore? = null,
    dao: StellarChatDao? = null,
    onBack: () -> Unit
) {
    val isMember = group.members.any { it.publicKeyCompressed.contentEquals(myBlsPubkey) }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var memberToRemove by remember { mutableStateOf<SEPGroupMemberLeaf?>(null) }
    var removalStatus by remember { mutableStateOf<String?>(null) }
    var removalStatusIsError by remember { mutableStateOf(false) }
    var isRemovingMember by remember { mutableStateOf(false) }
    var showRenameDialog by remember { mutableStateOf(false) }
    var showRotateKeyDialog by remember { mutableStateOf(false) }
    var showLeaveDialog by remember { mutableStateOf(false) }
    var isLeaving by remember { mutableStateOf(false) }
    var newGroupName by remember { mutableStateOf(group.name) }
    var recipientKey by remember { mutableStateOf("") }
    var inviteStatus by remember { mutableStateOf<String?>(null) }
    var showScanner by remember { mutableStateOf(false) }
    var aliasTarget by remember { mutableStateOf<String?>(null) }
    var aliasText by remember { mutableStateOf("") }
    var showInviteSheet by remember { mutableStateOf(false) }
    var showEpochHistory by remember { mutableStateOf(false) }
    var epochToPin by remember { mutableStateOf<Long?>(null) }

    val accent = remember(group.id) { GroupAccent.fromSeed(group.id) }

    if (showScanner) {
        Box(modifier = Modifier.fillMaxSize()) {
            QRScannerView { scannedCode ->
                val trimmed = scannedCode.trim()
                if (trimmed.length == 64 && trimmed.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }) {
                    recipientKey = trimmed
                }
                showScanner = false
                showInviteSheet = true
            }
        }
        return
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    if (isMember) {
                        TextButton(onClick = {
                            newGroupName = group.name
                            showRenameDialog = true
                        }) {
                            Text("Edit", fontWeight = FontWeight.SemiBold)
                        }
                    }
                }
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
        ) {
            Hero(
                group = group,
                accent = accent,
                isMember = isMember,
                onPickAvatar = { uri ->
                    if (isMember) {
                        val bytes = decodeAndDownscaleAvatar(context, uri)
                        if (bytes != null) onSetGroupAvatar(bytes)
                    }
                }
            )

            if (isMember) {
                QuickActionsRow(
                    pushEnabled = group.pushNotificationsEnabled,
                    canShare = group.groupType != com.stellarmls.mls.SEPGroupType.ONE_ON_ONE,
                    onShare = { showInviteSheet = true },
                    onToggleMute = {
                        onTogglePushNotifications?.invoke(!group.pushNotificationsEnabled)
                    },
                    onSearch = onSearchMessages,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp)
                )
            } else {
                DivergedBanner(
                    epoch = group.epoch,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp)
                )
            }

            SectionCard(
                title = "Members",
                trailing = "${group.members.size} of ${group.tier.memberCap}",
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)
            ) {
                val myHex = myBlsPubkey.toHex()
                val iAmOligarchyAdmin = group.groupType == com.stellarmls.mls.SEPGroupType.OLIGARCHY &&
                    group.adminPubkeys.contains(myHex)
                // Oligarchy: only admins can invite. Anarchy/Democracy: anyone can.
                val canInvite = isMember &&
                    group.groupType != com.stellarmls.mls.SEPGroupType.ONE_ON_ONE &&
                    (group.groupType != com.stellarmls.mls.SEPGroupType.OLIGARCHY || iAmOligarchyAdmin)
                if (canInvite) {
                    AddPeopleRow(onClick = { showInviteSheet = true })
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f))
                }
                group.members.forEachIndexed { idx, member ->
                    val hex = member.publicKeyCompressed.toHex()
                    val isMe = member.publicKeyCompressed.contentEquals(myBlsPubkey)
                    val isAdmin = group.groupType == com.stellarmls.mls.SEPGroupType.OLIGARCHY &&
                        group.adminPubkeys.contains(hex)
                    // Oligarchy kicks require the current user to be an admin.
                    val canKick = isMember && !isMe &&
                        group.groupType != com.stellarmls.mls.SEPGroupType.ONE_ON_ONE &&
                        group.groupType != com.stellarmls.mls.SEPGroupType.DEMOCRACY &&
                        (group.groupType != com.stellarmls.mls.SEPGroupType.OLIGARCHY || iAmOligarchyAdmin)
                    val canVote = isMember && !isMe &&
                        group.groupType == com.stellarmls.mls.SEPGroupType.DEMOCRACY &&
                        onProposeRemovalVote != null
                    // Oligarchy: only admins see promote/demote. Demote disallowed on the
                    // last remaining admin so the group always retains at least one.
                    val canPromote = isMember && !isMe &&
                        group.groupType == com.stellarmls.mls.SEPGroupType.OLIGARCHY &&
                        iAmOligarchyAdmin && !isAdmin && onPromoteToAdmin != null
                    val canDemote = isMember && !isMe &&
                        group.groupType == com.stellarmls.mls.SEPGroupType.OLIGARCHY &&
                        iAmOligarchyAdmin && isAdmin && group.adminPubkeys.size > 1 &&
                        onDemoteFromAdmin != null
                    val alias = contactAliasStore?.displayName(hex)
                    MemberRow(
                        hex = hex,
                        alias = alias,
                        isMe = isMe,
                        isAdmin = isAdmin,
                        canRemove = canKick,
                        canVote = canVote,
                        canPromote = canPromote,
                        canDemote = canDemote,
                        removing = isRemovingMember,
                        onTap = {
                            if (contactAliasStore != null && dao != null) {
                                aliasText = alias ?: ""
                                aliasTarget = hex
                            }
                        },
                        onRemove = { memberToRemove = member },
                        onVote = { onProposeRemovalVote?.invoke(hex) },
                        onPromote = { onPromoteToAdmin?.invoke(hex) },
                        onDemote = { onDemoteFromAdmin?.invoke(hex) }
                    )
                    if (idx < group.members.lastIndex) {
                        HorizontalDivider(
                            modifier = Modifier.padding(start = 64.dp),
                            color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f)
                        )
                    }
                }
            }
            SectionFooter(
                text = if (isMember) "Tap a member to set a nickname. Use the remove button to kick someone."
                       else "Members at epoch ${group.epoch}. You won't see members added after you were removed."
            )

            if (isMember && onTogglePushNotifications != null) {
                SectionCard(
                    title = "Notifications",
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)
                ) {
                    IconRow(
                        icon = {
                            SettingsIconBox(
                                icon = if (group.pushNotificationsEnabled) Icons.Filled.Notifications else Icons.Filled.NotificationsOff,
                                background = Color(0xFFFF9500)
                            )
                        },
                        title = "Push notifications",
                        subtitle = "New messages and key events",
                        trailing = {
                            Switch(
                                checked = group.pushNotificationsEnabled,
                                onCheckedChange = { onTogglePushNotifications(it) }
                            )
                        }
                    )
                }
            }

            if (isMember) {
                SectionCard(
                    title = "Security",
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)
                ) {
                    if (group.isPublishedOnChain) {
                        IconRow(
                            icon = {
                                SettingsIconBox(
                                    icon = Icons.Filled.VerifiedUser,
                                    background = Color(0xFF34C759)
                                )
                            },
                            title = "Anchored on Stellar",
                            subtitle = "Verified · epoch ${group.epoch}"
                        )
                        HorizontalDivider(
                            modifier = Modifier.padding(start = 64.dp),
                            color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f)
                        )
                    }
                    IconRow(
                        icon = {
                            SettingsIconBox(
                                icon = Icons.Filled.VpnKey,
                                background = Color(0xFF0A84FF)
                            )
                        },
                        title = "Rotate encryption key",
                        subtitle = "Forward secrecy · keeps all members",
                        onClick = { showRotateKeyDialog = true },
                        trailing = {
                            Icon(
                                Icons.AutoMirrored.Filled.KeyboardArrowRight,
                                contentDescription = null,
                                tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f)
                            )
                        }
                    )
                    if (epochSnapshots.isNotEmpty()) {
                        HorizontalDivider(
                            modifier = Modifier.padding(start = 64.dp),
                            color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f)
                        )
                        EpochHistoryDisclosure(
                            group = group,
                            snapshots = epochSnapshots,
                            expanded = showEpochHistory,
                            onToggle = { showEpochHistory = !showEpochHistory },
                            onPinRequested = { epoch -> epochToPin = epoch },
                            onUnpin = onUnpinEpoch
                        )
                    }
                }
                SectionFooter(
                    text = "Each key change creates a new epoch. You can time-travel to read messages from a past epoch."
                )
            }

            if (isMember) {
                SectionCard(
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)
                ) {
                    DangerRow(
                        icon = Icons.AutoMirrored.Filled.Logout,
                        title = "Leave Group",
                        busy = isLeaving,
                        onClick = { showLeaveDialog = true }
                    )
                }
            }

            if (!isMember && canForkGroup) {
                SectionCard(
                    title = "Options",
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)
                ) {
                    IconRow(
                        icon = {
                            SettingsIconBox(
                                icon = Icons.Filled.CallSplit,
                                background = Color(0xFF5E5CE6)
                            )
                        },
                        title = "Fork this group",
                        subtitle = "Start fresh with members you choose",
                        onClick = onForkGroup,
                        trailing = {
                            Icon(
                                Icons.AutoMirrored.Filled.KeyboardArrowRight,
                                contentDescription = null,
                                tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f)
                            )
                        }
                    )
                    if (epochSnapshots.isNotEmpty()) {
                        HorizontalDivider(
                            modifier = Modifier.padding(start = 64.dp),
                            color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.3f)
                        )
                        EpochHistoryDisclosure(
                            group = group,
                            snapshots = epochSnapshots,
                            expanded = showEpochHistory,
                            onToggle = { showEpochHistory = !showEpochHistory },
                            onPinRequested = { epoch -> epochToPin = epoch },
                            onUnpin = onUnpinEpoch
                        )
                    }
                }
                SectionFooter(text = "Create a new group from a past epoch, excluding members you choose.")
            }

            if (isRemovingMember) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(16.dp),
                    horizontalArrangement = Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
                    Spacer(modifier = Modifier.width(8.dp))
                    Text(
                        "Removing member…",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }

            removalStatus?.let { status ->
                Text(
                    status,
                    color = if (removalStatusIsError) MaterialTheme.colorScheme.error else Color(0xFF2E7D32),
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp)
                )
            }

            Spacer(modifier = Modifier.height(32.dp))
        }
    }

    memberToRemove?.let { member ->
        AlertDialog(
            onDismissRequest = { memberToRemove = null },
            title = { Text("Remove Member") },
            text = { Text("The member will be removed and the group key will be rotated. They will not be able to decrypt future messages.") },
            confirmButton = {
                TextButton(onClick = {
                    isRemovingMember = true
                    removalStatus = null
                    onRemoveMember(member.publicKeyCompressed) { result ->
                        isRemovingMember = false
                        if (result.isSuccess) {
                            removalStatus = "Member removed. Key rotated."
                            removalStatusIsError = false
                        } else {
                            removalStatus = result.exceptionOrNull()?.message ?: "Failed to remove member."
                            removalStatusIsError = true
                        }
                    }
                    memberToRemove = null
                }) {
                    Text("Remove", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { memberToRemove = null }) { Text("Cancel") }
            }
        )
    }

    if (showRenameDialog) {
        AlertDialog(
            onDismissRequest = { showRenameDialog = false },
            title = { Text("Rename Group") },
            text = {
                OutlinedTextField(
                    value = newGroupName,
                    onValueChange = { newGroupName = it },
                    label = { Text("Group name") },
                    singleLine = true
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    onRenameGroup(newGroupName)
                    showRenameDialog = false
                }) { Text("Rename") }
            },
            dismissButton = {
                TextButton(onClick = { showRenameDialog = false }) { Text("Cancel") }
            }
        )
    }

    if (showRotateKeyDialog) {
        AlertDialog(
            onDismissRequest = { showRotateKeyDialog = false },
            title = { Text("Rotate Group Key?") },
            text = { Text("A new encryption key will be generated for all members. Previous messages remain readable.") },
            confirmButton = {
                TextButton(onClick = {
                    onRotateKey()
                    showRotateKeyDialog = false
                }) { Text("Rotate Key") }
            },
            dismissButton = {
                TextButton(onClick = { showRotateKeyDialog = false }) { Text("Cancel") }
            }
        )
    }

    if (showLeaveDialog) {
        AlertDialog(
            onDismissRequest = { showLeaveDialog = false },
            title = { Text("Leave Group?") },
            text = { Text("You'll no longer receive messages. The group key will be rotated so future messages stay private from you.") },
            confirmButton = {
                TextButton(onClick = {
                    showLeaveDialog = false
                    isLeaving = true
                    onRemoveMember(myBlsPubkey) { result ->
                        isLeaving = false
                        if (result.isSuccess) {
                            onBack()
                        } else {
                            removalStatus = result.exceptionOrNull()?.message ?: "Failed to leave group."
                            removalStatusIsError = true
                        }
                    }
                }) {
                    Text("Leave Group", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { showLeaveDialog = false }) { Text("Cancel") }
            }
        )
    }

    epochToPin?.let { epoch ->
        AlertDialog(
            onDismissRequest = { epochToPin = null },
            title = { Text("Pin to Epoch $epoch?") },
            text = { Text("You will switch to viewing this epoch. Other members at the current epoch won't see your messages here.") },
            confirmButton = {
                TextButton(onClick = {
                    onPinEpoch(epoch)
                    epochToPin = null
                    onBack()
                }) { Text("Pin Epoch") }
            },
            dismissButton = {
                TextButton(onClick = { epochToPin = null }) { Text("Cancel") }
            }
        )
    }

    if (aliasTarget != null && contactAliasStore != null && dao != null) {
        AlertDialog(
            onDismissRequest = { aliasTarget = null },
            title = { Text("Set Name") },
            text = {
                Column {
                    Text(
                        (aliasTarget?.take(16) ?: "") + "...",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    OutlinedTextField(
                        value = aliasText,
                        onValueChange = { aliasText = it },
                        label = { Text("Display name") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth()
                    )
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    val pubkey = aliasTarget ?: return@TextButton
                    scope.launch { contactAliasStore.setAlias(pubkey, aliasText, dao) }
                    aliasTarget = null
                }) { Text("Save") }
            },
            dismissButton = {
                Row {
                    val current = aliasTarget?.let { contactAliasStore.displayName(it) }
                    if (!current.isNullOrEmpty()) {
                        TextButton(onClick = {
                            val pubkey = aliasTarget ?: return@TextButton
                            scope.launch { contactAliasStore.removeAlias(pubkey, dao) }
                            aliasTarget = null
                        }) {
                            Text("Remove", color = MaterialTheme.colorScheme.error)
                        }
                    }
                    TextButton(onClick = { aliasTarget = null }) { Text("Cancel") }
                }
            }
        )
    }

    if (showInviteSheet) {
        val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
        ModalBottomSheet(
            onDismissRequest = { showInviteSheet = false },
            sheetState = sheetState
        ) {
            InviteSheetContent(
                groupName = group.name,
                inviteLink = inviteLink,
                recipientKey = recipientKey,
                onRecipientKeyChange = { recipientKey = it; inviteStatus = null },
                inviteStatus = inviteStatus,
                onCopyLink = {
                    inviteLink?.let {
                        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                        clipboard.setPrimaryClip(ClipData.newPlainText("Invite Link", it))
                    }
                },
                onShareLink = {
                    inviteLink?.let {
                        val sendIntent = Intent().apply {
                            action = Intent.ACTION_SEND
                            putExtra(Intent.EXTRA_TEXT, it)
                            type = "text/plain"
                        }
                        context.startActivity(Intent.createChooser(sendIntent, null))
                    }
                },
                onPasteKey = {
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    val clip = clipboard.primaryClip
                    if (clip != null && clip.itemCount > 0) {
                        recipientKey = clip.getItemAt(0).text?.toString()?.trim() ?: ""
                    }
                },
                onScanQR = {
                    showInviteSheet = false
                    showScanner = true
                },
                onSendInvite = {
                    onSendInvitation?.invoke(recipientKey.trim()) { result ->
                        result.onSuccess { inviteStatus = "Invitation sent!" }
                        result.onFailure { inviteStatus = "Error: ${it.message}" }
                    }
                }
            )
        }
    }
}

// MARK: - Hero

@Composable
private fun Hero(
    group: ChatGroup,
    accent: GroupAccent,
    isMember: Boolean,
    onPickAvatar: (Uri) -> Unit
) {
    val photoPickerLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.PickVisualMedia()
    ) { uri: Uri? ->
        if (uri != null) onPickAvatar(uri)
    }
    val launchPicker: () -> Unit = {
        photoPickerLauncher.launch(
            PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)
        )
    }
    val avatarBitmap: ImageBitmap? = remember(group.avatarData) {
        group.avatarData?.let { bytes ->
            try {
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
            } catch (_: Exception) { null }
        }
    }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 8.dp, bottom = 12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Box(contentAlignment = Alignment.Center) {
            Box(
                modifier = Modifier
                    .size(220.dp)
                    .blur(48.dp)
                    .background(accent.primary.copy(alpha = 0.18f), CircleShape)
            )
            Box(
                modifier = Modifier
                    .size(96.dp)
                    .clip(CircleShape)
                    .background(accent.brush())
                    .clickable(enabled = isMember, onClick = launchPicker),
                contentAlignment = Alignment.Center
            ) {
                if (avatarBitmap != null) {
                    Image(
                        bitmap = avatarBitmap,
                        contentDescription = null,
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.size(96.dp).clip(CircleShape)
                    )
                } else {
                    Text(
                        initialsForGroup(group.name),
                        color = Color.White,
                        fontSize = 36.sp,
                        fontWeight = FontWeight.Bold
                    )
                }
            }
            if (isMember) {
                Box(
                    modifier = Modifier
                        .offset(x = 34.dp, y = 34.dp)
                        .size(32.dp)
                        .clip(CircleShape)
                        .background(MaterialTheme.colorScheme.surface)
                        .border(3.dp, MaterialTheme.colorScheme.background, CircleShape)
                        .clickable(onClick = launchPicker),
                    contentAlignment = Alignment.Center
                ) {
                    Icon(
                        Icons.Filled.Edit,
                        contentDescription = "Change group photo",
                        tint = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.size(16.dp)
                    )
                }
            }
        }

        Text(
            group.name,
            fontSize = 24.sp,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(horizontal = 24.dp)
        )

        Row(
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            modifier = Modifier.padding(horizontal = 16.dp)
        ) {
            Chip(
                icon = Icons.Filled.People,
                text = "${group.members.size} members",
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                bgAlpha = 0.08f
            )
            Chip(
                icon = Icons.Filled.Lock,
                text = "End-to-end encrypted",
                tint = Color(0xFF2E7D32),
                bgAlpha = 0.12f
            )
            if (group.isPublishedOnChain && isMember) {
                Chip(
                    icon = Icons.Filled.VerifiedUser,
                    text = "Anchored",
                    tint = Color(0xFF2E7D32),
                    bgAlpha = 0.12f
                )
            }
        }
    }
}

@Composable
private fun Chip(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    text: String,
    tint: Color,
    bgAlpha: Float
) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        modifier = Modifier
            .clip(RoundedCornerShape(50))
            .background(tint.copy(alpha = bgAlpha))
            .padding(horizontal = 10.dp, vertical = 5.dp)
    ) {
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(11.dp))
        Text(
            text,
            color = tint,
            fontSize = 11.sp,
            fontWeight = FontWeight.Medium
        )
    }
}

// MARK: - Quick actions

@Composable
private fun QuickActionsRow(
    pushEnabled: Boolean,
    canShare: Boolean,
    onShare: () -> Unit,
    onToggleMute: () -> Unit,
    onSearch: () -> Unit,
    modifier: Modifier = Modifier
) {
    Row(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        if (canShare) {
            QuickActionTile(
                icon = Icons.Filled.Share,
                label = "Share",
                subtitle = "Invite link",
                tint = MaterialTheme.colorScheme.primary,
                onClick = onShare,
                modifier = Modifier.weight(1f)
            )
        } else {
            QuickActionTile(
                icon = Icons.Filled.Lock,
                label = "Sealed",
                subtitle = "1v1",
                tint = Color.Gray,
                onClick = {},
                modifier = Modifier.weight(1f)
            )
        }
        QuickActionTile(
            icon = if (pushEnabled) Icons.Filled.Notifications else Icons.Filled.NotificationsOff,
            label = if (pushEnabled) "Muted" else "Unmute",
            subtitle = if (pushEnabled) "On" else "Off",
            tint = if (pushEnabled) Color(0xFFFF9500) else Color.Gray,
            onClick = onToggleMute,
            modifier = Modifier.weight(1f)
        )
        QuickActionTile(
            icon = Icons.Filled.Search,
            label = "Search",
            subtitle = "Messages",
            tint = Color.Gray,
            onClick = onSearch,
            modifier = Modifier.weight(1f)
        )
    }
}

@Composable
private fun QuickActionTile(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    label: String,
    subtitle: String,
    tint: Color,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    Column(
        modifier = modifier
            .clip(RoundedCornerShape(14.dp))
            .background(MaterialTheme.colorScheme.surfaceContainer)
            .clickable(onClick = onClick)
            .padding(vertical = 12.dp, horizontal = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(6.dp)
    ) {
        Box(
            modifier = Modifier
                .size(36.dp)
                .clip(RoundedCornerShape(10.dp))
                .background(tint),
            contentAlignment = Alignment.Center
        ) {
            Icon(icon, contentDescription = null, tint = Color.White, modifier = Modifier.size(18.dp))
        }
        Text(label, fontSize = 13.sp, fontWeight = FontWeight.Medium)
        Text(
            subtitle,
            fontSize = 11.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
    }
}

// MARK: - Diverged banner

@Composable
private fun DivergedBanner(epoch: Long, modifier: Modifier = Modifier) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(Color(0xFFFF9500).copy(alpha = 0.12f))
            .padding(14.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Icon(
            Icons.Filled.Warning,
            contentDescription = null,
            tint = Color(0xFFFF9500),
            modifier = Modifier.size(22.dp)
        )
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(
                "You were removed from this group",
                fontWeight = FontWeight.SemiBold,
                color = Color(0xFFC77700),
                fontSize = 14.sp
            )
            Text(
                "You can still read messages from epoch $epoch, but won't see anything posted after that. Fork this epoch to continue with the members you choose.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

// MARK: - Section primitives

@Composable
private fun SectionCard(
    title: String? = null,
    trailing: String? = null,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit
) {
    Column(modifier = modifier) {
        if (title != null) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(start = 16.dp, end = 16.dp, bottom = 6.dp, top = 4.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    title.uppercase(),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    fontWeight = FontWeight.SemiBold
                )
                Spacer(modifier = Modifier.weight(1f))
                if (trailing != null) {
                    Text(
                        trailing,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }
        }
        Card(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(14.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)
        ) {
            Column { content() }
        }
    }
}

@Composable
private fun SectionFooter(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(start = 20.dp, end = 20.dp, bottom = 4.dp, top = 2.dp)
    )
}

@Composable
private fun IconRow(
    icon: @Composable () -> Unit,
    title: String,
    subtitle: String? = null,
    onClick: (() -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        icon()
        Column(modifier = Modifier.weight(1f)) {
            Text(title, fontSize = 15.sp)
            if (subtitle != null) {
                Text(
                    subtitle,
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
        }
        if (trailing != null) trailing()
    }
}

@Composable
private fun DangerRow(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    busy: Boolean,
    onClick: () -> Unit
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = !busy, onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.error, modifier = Modifier.size(22.dp))
        Text(title, color = MaterialTheme.colorScheme.error, fontSize = 15.sp, modifier = Modifier.weight(1f))
        if (busy) CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
    }
}

// MARK: - Members

@Composable
private fun AddPeopleRow(onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Box(
            modifier = Modifier
                .size(40.dp)
                .clip(CircleShape)
                .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.14f)),
            contentAlignment = Alignment.Center
        ) {
            Icon(
                Icons.Filled.Add,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier.size(18.dp)
            )
        }
        Text(
            "Add people",
            color = MaterialTheme.colorScheme.primary,
            fontWeight = FontWeight.Medium,
            fontSize = 15.sp,
            modifier = Modifier.weight(1f)
        )
        Icon(
            Icons.AutoMirrored.Filled.KeyboardArrowRight,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f)
        )
    }
}

@Composable
private fun MemberRow(
    hex: String,
    alias: String?,
    isMe: Boolean,
    isAdmin: Boolean,
    canRemove: Boolean,
    canVote: Boolean = false,
    canPromote: Boolean = false,
    canDemote: Boolean = false,
    removing: Boolean,
    onTap: () -> Unit,
    onRemove: () -> Unit,
    onVote: () -> Unit = {},
    onPromote: () -> Unit = {},
    onDemote: () -> Unit = {}
) {
    val displayName = when {
        !alias.isNullOrEmpty() -> alias
        isMe -> "You"
        else -> hex.take(8) + "…" + hex.takeLast(4)
    }
    val subtitle = hex.take(10) + "…" + if (isMe) " · You" else ""
    val accent = remember(hex) { GroupAccent.fromSeed(hex) }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onTap)
            .padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp)
    ) {
        Box(
            modifier = Modifier
                .size(40.dp)
                .clip(CircleShape)
                .background(accent.brush()),
            contentAlignment = Alignment.Center
        ) {
            Text(
                initialsForGroup(displayName),
                color = Color.White,
                fontSize = 15.sp,
                fontWeight = FontWeight.SemiBold
            )
        }
        Column(modifier = Modifier.weight(1f)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp)
            ) {
                Text(
                    displayName,
                    fontWeight = FontWeight.SemiBold,
                    fontSize = 15.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis
                )
                if (isAdmin) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(3.dp),
                        modifier = Modifier
                            .clip(RoundedCornerShape(4.dp))
                            .background(Color(0xFFFF9500).copy(alpha = 0.14f))
                            .padding(horizontal = 6.dp, vertical = 1.dp)
                    ) {
                        Icon(
                            Icons.Filled.Shield,
                            contentDescription = null,
                            tint = Color(0xFFFF9500),
                            modifier = Modifier.size(9.dp)
                        )
                        Text(
                            "ADMIN",
                            fontSize = 10.sp,
                            fontWeight = FontWeight.SemiBold,
                            color = Color(0xFFFF9500)
                        )
                    }
                }
            }
            Text(
                subtitle,
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
        }
        if (canVote) {
            IconButton(onClick = onVote) {
                Icon(
                    Icons.Filled.HowToVote,
                    contentDescription = "Propose removal vote",
                    tint = Color(0xFF6A1B9A)
                )
            }
        }
        if (canPromote) {
            IconButton(onClick = onPromote) {
                Icon(
                    Icons.Filled.Shield,
                    contentDescription = "Promote to admin",
                    tint = Color(0xFF007AFF)
                )
            }
        }
        if (canDemote) {
            IconButton(onClick = onDemote) {
                Icon(
                    Icons.Filled.Shield,
                    contentDescription = "Demote admin",
                    tint = Color(0xFFFF9500)
                )
            }
        }
        if (canRemove) {
            IconButton(
                onClick = onRemove,
                enabled = !removing
            ) {
                Icon(
                    Icons.Filled.PersonRemove,
                    contentDescription = "Remove",
                    tint = if (removing) MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f)
                           else MaterialTheme.colorScheme.error
                )
            }
        }
    }
}

// MARK: - Epoch history

@Composable
private fun EpochHistoryDisclosure(
    group: ChatGroup,
    snapshots: Map<Long, EpochSnapshot>,
    expanded: Boolean,
    onToggle: () -> Unit,
    onPinRequested: (Long) -> Unit,
    onUnpin: () -> Unit
) {
    val sorted = remember(snapshots) { snapshots.entries.sortedByDescending { it.key } }
    Column {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onToggle)
                .padding(horizontal = 14.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            SettingsIconBox(
                icon = Icons.Filled.History,
                background = Color(0xFF5E5CE6)
            )
            Column(modifier = Modifier.weight(1f)) {
                Text("Epoch history", fontSize = 15.sp)
                Text(
                    "${sorted.size} ${if (sorted.size == 1) "epoch" else "epochs"} · current is epoch ${group.epoch}",
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            Icon(
                if (expanded) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
        AnimatedVisibility(visible = expanded) {
            Column(modifier = Modifier.padding(start = 20.dp, end = 14.dp, bottom = 8.dp)) {
                if (group.pinnedEpoch != null) {
                    TextButton(onClick = onUnpin) {
                        Text("Follow Latest Epoch")
                    }
                }
                sorted.forEachIndexed { idx, (epoch, snapshot) ->
                    val isCurrent = epoch == group.epoch
                    val isPinned = group.pinnedEpoch == epoch
                    val isRekey = !snapshot.groupSecret.contentEquals(group.groupSecret)
                    EpochTimelineRow(
                        epoch = epoch,
                        isCurrent = isCurrent,
                        isPinned = isPinned,
                        isRekey = isRekey,
                        description = snapshot.changeDescription,
                        memberCount = snapshot.members.size,
                        isLast = idx == sorted.lastIndex,
                        onTap = {
                            if (isPinned) onUnpin() else onPinRequested(epoch)
                        }
                    )
                }
            }
        }
    }
}

@Composable
private fun EpochTimelineRow(
    epoch: Long,
    isCurrent: Boolean,
    isPinned: Boolean,
    isRekey: Boolean,
    description: String,
    memberCount: Int,
    isLast: Boolean,
    onTap: () -> Unit
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onTap)
            .padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp)
    ) {
        Column(
            modifier = Modifier.width(18.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Box(
                modifier = Modifier
                    .size(16.dp)
                    .clip(CircleShape)
                    .background(
                        if (isCurrent) MaterialTheme.colorScheme.primary.copy(alpha = 0.25f)
                        else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.2f)
                    ),
                contentAlignment = Alignment.Center
            ) {
                Box(
                    modifier = Modifier
                        .size(8.dp)
                        .clip(CircleShape)
                        .background(
                            if (isCurrent) MaterialTheme.colorScheme.primary
                            else MaterialTheme.colorScheme.onSurfaceVariant
                        )
                )
            }
            if (!isLast) {
                Box(
                    modifier = Modifier
                        .width(1.5.dp)
                        .height(36.dp)
                        .background(MaterialTheme.colorScheme.outlineVariant)
                )
            }
        }
        Column(
            modifier = Modifier
                .weight(1f)
                .padding(bottom = if (isLast) 0.dp else 12.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp)
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp)
            ) {
                Text("Epoch $epoch", fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                if (isCurrent) {
                    Text(
                        "CURRENT",
                        fontSize = 10.sp,
                        fontWeight = FontWeight.SemiBold,
                        color = MaterialTheme.colorScheme.primary
                    )
                }
                if (isPinned) {
                    Icon(
                        Icons.Filled.PushPin,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.primary,
                        modifier = Modifier.size(11.dp)
                    )
                }
                if (isRekey) {
                    Icon(
                        Icons.Filled.Lock,
                        contentDescription = null,
                        tint = Color(0xFF2E7D32),
                        modifier = Modifier.size(11.dp)
                    )
                }
            }
            Text(description, fontSize = 12.sp)
            Text(
                "$memberCount ${if (memberCount == 1) "member" else "members"}",
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

// MARK: - Invite bottom sheet

@Composable
private fun InviteSheetContent(
    groupName: String,
    inviteLink: String?,
    recipientKey: String,
    onRecipientKeyChange: (String) -> Unit,
    inviteStatus: String?,
    onCopyLink: () -> Unit,
    onShareLink: () -> Unit,
    onPasteKey: () -> Unit,
    onScanQR: () -> Unit,
    onSendInvite: () -> Unit
) {
    var linkCopied by remember { mutableStateOf(false) }
    var showAdvanced by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(horizontal = 20.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp)
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text("Invite to $groupName", fontSize = 20.sp, fontWeight = FontWeight.Bold)
            Text(
                "Anyone with this link can join. It's safe — messages stay end-to-end encrypted.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }

        if (inviteLink != null) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(14.dp))
                    .background(MaterialTheme.colorScheme.surfaceContainerHigh)
                    .padding(14.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                Icon(
                    Icons.Filled.Link,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary
                )
                Text(
                    if (inviteLink.length > 40) inviteLink.take(28) + "…" else inviteLink,
                    style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f)
                )
                Button(
                    onClick = {
                        onCopyLink()
                        linkCopied = true
                        scope.launch {
                            delay(2000)
                            linkCopied = false
                        }
                    },
                    colors = ButtonDefaults.buttonColors(),
                    contentPadding = PaddingValues(horizontal = 14.dp, vertical = 6.dp)
                ) {
                    Text(if (linkCopied) "Copied" else "Copy", fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
                }
            }

            Button(
                onClick = onShareLink,
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(14.dp),
                contentPadding = PaddingValues(vertical = 14.dp)
            ) {
                Icon(Icons.Filled.Share, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(modifier = Modifier.width(8.dp))
                Text("Share link…", fontWeight = FontWeight.SemiBold)
            }
        }

        // Advanced: inbox key
        Column {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { showAdvanced = !showAdvanced }
                    .padding(vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    "OR INVITE BY INBOX KEY",
                    fontSize = 11.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f)
                )
                Icon(
                    if (showAdvanced) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            AnimatedVisibility(visible = showAdvanced) {
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        OutlinedButton(
                            onClick = onPasteKey,
                            modifier = Modifier.weight(1f),
                            shape = RoundedCornerShape(12.dp)
                        ) {
                            Icon(Icons.Filled.ContentCopy, contentDescription = null, modifier = Modifier.size(16.dp))
                            Spacer(modifier = Modifier.width(6.dp))
                            Text("Paste")
                        }
                        OutlinedButton(
                            onClick = onScanQR,
                            modifier = Modifier.weight(1f),
                            shape = RoundedCornerShape(12.dp)
                        ) {
                            Icon(Icons.Filled.QrCodeScanner, contentDescription = null, modifier = Modifier.size(16.dp))
                            Spacer(modifier = Modifier.width(6.dp))
                            Text("Scan QR")
                        }
                    }
                    OutlinedTextField(
                        value = recipientKey,
                        onValueChange = onRecipientKeyChange,
                        label = { Text("X25519 inbox key (64 hex chars)") },
                        modifier = Modifier.fillMaxWidth(),
                        singleLine = true,
                        textStyle = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace)
                    )
                    Button(
                        onClick = onSendInvite,
                        modifier = Modifier.fillMaxWidth(),
                        enabled = recipientKey.trim().length == 64,
                        shape = RoundedCornerShape(12.dp),
                        contentPadding = PaddingValues(vertical = 12.dp)
                    ) {
                        Text("Send invitation", fontWeight = FontWeight.SemiBold)
                    }
                    inviteStatus?.let { status ->
                        Text(
                            status,
                            style = MaterialTheme.typography.bodySmall,
                            color = if (status.startsWith("Error")) MaterialTheme.colorScheme.error
                                    else Color(0xFF2E7D32)
                        )
                    }
                }
            }
        }

        Spacer(modifier = Modifier.height(8.dp))
    }
}

// MARK: - Helpers

private const val GROUP_AVATAR_MAX_EDGE_PX = 256

/** Decode the picked image URI, downscale it to a max edge of
 *  [GROUP_AVATAR_MAX_EDGE_PX], and re-encode as JPEG for compact storage. */
private fun decodeAndDownscaleAvatar(context: Context, uri: Uri): ByteArray? {
    val resolver = context.contentResolver
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
    val longest = maxOf(bounds.outWidth, bounds.outHeight).coerceAtLeast(1)
    var sample = 1
    while (longest / sample > GROUP_AVATAR_MAX_EDGE_PX * 2) sample *= 2
    val decodeOpts = BitmapFactory.Options().apply { inSampleSize = sample }
    val bitmap = resolver.openInputStream(uri)?.use {
        BitmapFactory.decodeStream(it, null, decodeOpts)
    } ?: return null
    val w = bitmap.width
    val h = bitmap.height
    val scale = maxOf(w, h).let { if (it > GROUP_AVATAR_MAX_EDGE_PX) GROUP_AVATAR_MAX_EDGE_PX.toFloat() / it else 1f }
    val scaled = if (scale < 1f) {
        Bitmap.createScaledBitmap(bitmap, (w * scale).toInt(), (h * scale).toInt(), true)
    } else bitmap
    val buffer = java.io.ByteArrayOutputStream()
    scaled.compress(Bitmap.CompressFormat.JPEG, 85, buffer)
    if (scaled !== bitmap) scaled.recycle()
    bitmap.recycle()
    return buffer.toByteArray()
}

private fun initialsForGroup(name: String): String {
    val parts = name.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }
    if (parts.isEmpty()) return "?"
    if (parts.size == 1) return parts[0].take(2).uppercase()
    val first = parts[0].firstOrNull()?.toString() ?: ""
    val second = parts[1].firstOrNull()?.toString() ?: ""
    return (first + second).uppercase()
}

private val com.stellarmls.mls.SEPTier.memberCap: String
    get() = when (this) {
        com.stellarmls.mls.SEPTier.SMALL -> "32"
        com.stellarmls.mls.SEPTier.MEDIUM -> "256"
        com.stellarmls.mls.SEPTier.LARGE -> "2,048"
    }
