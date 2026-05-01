package chat.onym.android.ui.screens

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import android.widget.Toast
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
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
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.ChevronRight
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.ContentPaste
import androidx.compose.material.icons.filled.Error
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.PhotoCamera
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.VerifiedUser
import androidx.compose.material.icons.filled.VpnKey
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import chat.onym.android.ui.TestTags
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate as drawRotate
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import chat.onym.android.model.toHex
import chat.onym.android.nostr.InvitationTransport
import chat.onym.android.ui.components.GroupAccent
import chat.onym.android.ui.components.GroupAccentStore
import chat.onym.android.ui.components.QRCodeImage
import chat.onym.android.ui.components.QRScannerView
import chat.onym.android.viewmodel.CreateGroupViewModel
import chat.onym.android.viewmodel.CreationPhase
import chat.onym.android.viewmodel.GroupListViewModel
import chat.onym.android.viewmodel.InvitationStatus
import chat.onym.android.viewmodel.OnChainPublishStatus
import com.stellarmls.mls.SEPGroupType

private enum class InputStep { IDENTITY, PEOPLE }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CreateGroupScreen(
    viewModel: CreateGroupViewModel,
    keyManager: chat.onym.android.crypto.KeyManager,
    groupListViewModel: GroupListViewModel,
    invitationTransport: InvitationTransport,
    onBack: () -> Unit,
    onNavigateToChat: (groupId: String) -> Unit = {}
) {
    val context = LocalContext.current
    var showScanner by remember { mutableStateOf(false) }
    var showKeyDialog by remember { mutableStateOf(false) }
    var step by remember { mutableStateOf(InputStep.IDENTITY) }
    var accent by remember { mutableStateOf(GroupAccent.BLUE) }
    var searchText by remember { mutableStateOf("") }

    val isProcessing = viewModel.phase == CreationPhase.CREATING ||
        viewModel.phase == CreationPhase.PUBLISHING ||
        viewModel.phase == CreationPhase.INVITING

    BackHandler(enabled = isProcessing) { /* blocked during creation */ }
    BackHandler(enabled = !isProcessing && viewModel.phase == CreationPhase.INPUT && step == InputStep.PEOPLE) {
        step = InputStep.IDENTITY
    }

    // Persist accent once the group is created.
    val createdGroupId = viewModel.createdGroup?.id
    LaunchedEffect(createdGroupId) {
        if (createdGroupId != null) {
            GroupAccentStore.set(context, accent, createdGroupId)
        }
    }

    if (showScanner) {
        Box(modifier = Modifier.fillMaxSize()) {
            QRScannerView { scanned ->
                val trimmed = scanned.trim().lowercase()
                if (trimmed.length == 64 && trimmed.all { it in '0'..'9' || it in 'a'..'f' }) {
                    viewModel.keyInput = trimmed
                    viewModel.addParticipant(keyManager.keyAgreementPublicKeyHex)
                }
                showScanner = false
            }
        }
        return
    }

    val bg = MaterialTheme.colorScheme.surfaceContainerLowest

    Scaffold(
        modifier = Modifier.testTag(TestTags.CreateGroup.Screen),
        containerColor = bg,
        topBar = {
            TopAppBar(
                title = {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Text(
                            when (viewModel.phase) {
                                CreationPhase.INPUT -> if (step == InputStep.IDENTITY) "New Group"
                                else if (viewModel.participantKeys.isEmpty()) "Add People"
                                else "Add People · ${viewModel.participantKeys.size}"
                                CreationPhase.CREATING, CreationPhase.PUBLISHING, CreationPhase.INVITING ->
                                    "Creating ${viewModel.groupName.ifBlank { "group" }}"
                                CreationPhase.DONE ->
                                    if (viewModel.groupName.isBlank()) "Group created" else "${viewModel.groupName} is live"
                            },
                            fontSize = 17.sp,
                            fontWeight = FontWeight.SemiBold
                        )
                        if (viewModel.phase == CreationPhase.INPUT) {
                            Text(
                                if (step == InputStep.IDENTITY) "Step 1 of 2" else "Step 2 of 2",
                                fontSize = 11.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        }
                    }
                },
                navigationIcon = {
                    when {
                        viewModel.phase == CreationPhase.INPUT && step == InputStep.PEOPLE -> {
                            TextButton(
                                onClick = { step = InputStep.IDENTITY },
                                modifier = Modifier.testTag(TestTags.CreateGroup.BackButton)
                            ) { Text("Back") }
                        }
                        viewModel.phase == CreationPhase.INPUT -> {
                            TextButton(
                                onClick = onBack,
                                modifier = Modifier.testTag(TestTags.CreateGroup.CancelButton)
                            ) { Text("Cancel") }
                        }
                        viewModel.phase == CreationPhase.DONE -> {
                            IconButton(onClick = {
                                val groupId = viewModel.createdGroup?.id
                                if (groupId != null) onNavigateToChat(groupId) else onBack()
                            }) {
                                Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Close")
                            }
                        }
                    }
                },
                actions = {
                    when {
                        viewModel.phase == CreationPhase.INPUT && step == InputStep.IDENTITY -> {
                            TextButton(
                                onClick = { step = InputStep.PEOPLE },
                                enabled = viewModel.groupName.isNotBlank(),
                                modifier = Modifier.testTag(TestTags.CreateGroup.NextButton)
                            ) { Text("Next", fontWeight = FontWeight.SemiBold) }
                        }
                        viewModel.phase == CreationPhase.INPUT && step == InputStep.PEOPLE -> {
                            TextButton(
                                onClick = {
                                    viewModel.startCreation(
                                        keyManager,
                                        groupListViewModel.defaultGroupTier,
                                        groupListViewModel,
                                        invitationTransport
                                    )
                                },
                                modifier = Modifier.testTag(TestTags.CreateGroup.CreateButton)
                            ) { Text("Create", fontWeight = FontWeight.SemiBold) }
                        }
                        viewModel.phase == CreationPhase.DONE -> {
                            TextButton(onClick = {
                                val groupId = viewModel.createdGroup?.id
                                if (groupId != null) onNavigateToChat(groupId) else onBack()
                            }) { Text("Open", fontWeight = FontWeight.SemiBold) }
                        }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = bg),
                windowInsets = WindowInsets(0, 0, 0, 0)
            )
        }
    ) { padding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .imePadding()
        ) {
            when (viewModel.phase) {
                CreationPhase.INPUT -> when (step) {
                    InputStep.IDENTITY -> IdentityStep(
                        viewModel = viewModel,
                        accent = accent,
                        onAccentChange = { accent = it }
                    )
                    InputStep.PEOPLE -> PeopleStep(
                        viewModel = viewModel,
                        groupListViewModel = groupListViewModel,
                        keyManager = keyManager,
                        accent = accent,
                        searchText = searchText,
                        onSearchChange = { searchText = it },
                        onShowKeyDialog = { showKeyDialog = true }
                    )
                }
                CreationPhase.CREATING, CreationPhase.PUBLISHING, CreationPhase.INVITING ->
                    ProgressStage(
                        viewModel = viewModel,
                        groupListViewModel = groupListViewModel,
                        invitationTransport = invitationTransport,
                        keyManager = keyManager,
                        accent = accent
                    )
                CreationPhase.DONE -> DoneStage(
                    viewModel = viewModel,
                    groupListViewModel = groupListViewModel,
                    accent = accent,
                    context = context
                )
            }
        }
    }

    if (showKeyDialog) {
        InviteByKeyDialog(
            ownInboxKey = keyManager.keyAgreementPublicKeyHex,
            existing = viewModel.participantKeys.toList(),
            onAdd = { key ->
                viewModel.keyInput = key
                viewModel.addParticipant(keyManager.keyAgreementPublicKeyHex)
            },
            onShowScanner = { showScanner = true; showKeyDialog = false },
            onDismiss = { showKeyDialog = false }
        )
    }
}

// MARK: - Identity step

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun IdentityStep(
    viewModel: CreateGroupViewModel,
    accent: GroupAccent,
    onAccentChange: (GroupAccent) -> Unit
) {
    val context = LocalContext.current
    val photoPickerLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.PickVisualMedia()
    ) { uri: Uri? ->
        if (uri != null) viewModel.onAvatarPicked(context, uri)
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Spacer(Modifier.height(16.dp))

        // Hero avatar with camera badge — tap anywhere on it to pick a photo.
        Box(
            modifier = Modifier
                .clip(RoundedCornerShape(54.dp))
                .clickable {
                    photoPickerLauncher.launch(
                        PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)
                    )
                }
                .semantics { contentDescription = "Change group photo" },
            contentAlignment = Alignment.BottomEnd
        ) {
            GroupAvatar(
                size = 108.dp,
                accent = accent,
                name = viewModel.groupName,
                filled = viewModel.groupName.isNotBlank(),
                corner = 54.dp,
                avatarBytes = viewModel.avatarBytes
            )
            Box(
                modifier = Modifier
                    .offset(x = 2.dp, y = 2.dp)
                    .size(36.dp)
                    .clip(CircleShape)
                    .background(MaterialTheme.colorScheme.primary)
                    .border(3.dp, MaterialTheme.colorScheme.surfaceContainerLowest, CircleShape),
                contentAlignment = Alignment.Center
            ) {
                Icon(
                    Icons.Filled.PhotoCamera,
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier.size(18.dp)
                )
            }
        }

        Spacer(Modifier.height(20.dp))

        // Name input card
        OutlinedTextField(
            value = viewModel.groupName,
            onValueChange = { viewModel.groupName = it },
            placeholder = { Text("Group name", fontSize = 18.sp) },
            modifier = Modifier
                .fillMaxWidth()
                .testTag(TestTags.CreateGroup.NameField),
            singleLine = true,
            shape = RoundedCornerShape(14.dp)
        )
        Text(
            "Visible to members. You can change this anytime.",
            fontSize = 11.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 4.dp, vertical = 6.dp)
        )

        Spacer(Modifier.height(16.dp))

        // Accent picker
        Column(Modifier.fillMaxWidth()) {
            Text(
                "ACCENT COLOR",
                fontSize = 11.sp,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 4.dp, vertical = 6.dp)
            )
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                for (option in GroupAccent.entries) {
                    val selected = option == accent
                    Box(
                        modifier = Modifier
                            .size(46.dp)
                            .clip(CircleShape)
                            .border(
                                width = 2.dp,
                                color = if (selected) MaterialTheme.colorScheme.primary else Color.Transparent,
                                shape = CircleShape
                            )
                            .padding(3.dp)
                            .clip(CircleShape)
                            .background(option.brush())
                            .clickable { onAccentChange(option) },
                        contentAlignment = Alignment.Center
                    ) {
                        if (selected) {
                            Icon(
                                Icons.Filled.Check,
                                contentDescription = null,
                                tint = Color.White,
                                modifier = Modifier.size(18.dp)
                            )
                        }
                    }
                }
            }
        }

        Spacer(Modifier.height(20.dp))

        // Governance picker
        Column(Modifier.fillMaxWidth()) {
            Text(
                "GOVERNANCE",
                fontSize = 11.sp,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 4.dp, vertical = 6.dp)
            )
            val types = listOf(
                SEPGroupType.ANARCHY to "Anarchy",
                SEPGroupType.ONE_ON_ONE to "1v1",
                SEPGroupType.DEMOCRACY to "Democracy",
                SEPGroupType.OLIGARCHY to "Oligarchy"
            )
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                types.forEachIndexed { index, (type, label) ->
                    SegmentedButton(
                        selected = viewModel.groupType == type,
                        onClick = { viewModel.selectGroupType(type) },
                        shape = SegmentedButtonDefaults.itemShape(
                            index = index,
                            count = types.size
                        ),
                        label = { Text(label, fontSize = 12.sp) }
                    )
                }
            }
            val help = when (viewModel.groupType) {
                SEPGroupType.ANARCHY -> "Any member can add or remove anyone. Default behavior."
                SEPGroupType.ONE_ON_ONE -> "Sealed 2-person chat. Membership cannot change."
                SEPGroupType.DEMOCRACY -> "Membership changes require a majority ballot."
                SEPGroupType.OLIGARCHY -> "Only admins can add or remove members. Creator is first admin."
            }
            Text(
                help,
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 4.dp, vertical = 6.dp)
            )
        }

        Spacer(Modifier.height(20.dp))

        // E2EE banner
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(14.dp))
                .background(Color(0xFF30D158).copy(alpha = 0.10f))
                .padding(14.dp),
            verticalAlignment = Alignment.Top
        ) {
            Icon(
                Icons.Filled.Lock,
                contentDescription = null,
                tint = Color(0xFF30D158),
                modifier = Modifier.size(18.dp)
            )
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text("End-to-end encrypted", fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
                Text(
                    "Only members can read messages. Onym never sees the content.",
                    fontSize = 11.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
        }

        viewModel.errorMessage?.let { error ->
            Spacer(Modifier.height(12.dp))
            Text(error, color = MaterialTheme.colorScheme.error, fontSize = 12.sp)
        }

        Spacer(Modifier.height(40.dp))
    }
}

// MARK: - People step

@Composable
private fun PeopleStep(
    viewModel: CreateGroupViewModel,
    groupListViewModel: GroupListViewModel,
    keyManager: chat.onym.android.crypto.KeyManager,
    accent: GroupAccent,
    searchText: String,
    onSearchChange: (String) -> Unit,
    onShowKeyDialog: () -> Unit
) {
    val contactInboxKeys = remember { groupListViewModel.lookupContactInboxKeys() }
    val aliasStore = groupListViewModel.contactAliasStore

    val fromChats = remember(groupListViewModel.groups, contactInboxKeys) {
        val groupNamesByBls = mutableMapOf<String, MutableSet<String>>()
        for (group in groupListViewModel.groups) {
            for (member in group.members) {
                val hex = member.publicKeyCompressed.toHex()
                groupNamesByBls.getOrPut(hex) { mutableSetOf() }.add(group.name)
            }
        }
        contactInboxKeys.mapNotNull { (blsHex, inboxHex) ->
            val groupNames = groupNamesByBls[blsHex].orEmpty().sorted()
            val subtitle = groupNames.joinToString(", ").ifEmpty { blsHex.take(12) + "…" }
            ChatContact(
                blsHex = blsHex,
                inboxHex = inboxHex,
                alias = aliasStore.displayName(blsHex),
                subtitle = subtitle
            )
        }.sortedBy { (it.alias ?: it.blsHex).lowercase() }
    }

    val filtered = remember(fromChats, searchText) {
        val needle = searchText.trim().lowercase()
        if (needle.isEmpty()) fromChats
        else fromChats.filter { contact ->
            (contact.alias ?: "").lowercase().contains(needle) ||
                contact.blsHex.lowercase().contains(needle) ||
                contact.subtitle.lowercase().contains(needle)
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
    ) {
        // Selected chips
        if (viewModel.participantKeys.isNotEmpty()) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .horizontalScroll(rememberScrollState())
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                for (key in viewModel.participantKeys) {
                    val contact = fromChats.firstOrNull { it.inboxHex == key }
                    val seed = contact?.blsHex ?: key
                    val label = contact?.alias ?: (key.take(8) + "…")
                    Row(
                        modifier = Modifier
                            .clip(RoundedCornerShape(18.dp))
                            .background(MaterialTheme.colorScheme.surfaceContainerHigh)
                            .padding(start = 4.dp, end = 10.dp, top = 4.dp, bottom = 4.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        SeedAvatar(seed = seed, size = 26.dp, name = contact?.alias ?: "?")
                        Spacer(Modifier.width(6.dp))
                        Text(label, fontSize = 12.sp, fontWeight = FontWeight.Medium, maxLines = 1)
                        Spacer(Modifier.width(4.dp))
                        Box(
                            modifier = Modifier
                                .size(18.dp)
                                .clip(CircleShape)
                                .clickable { viewModel.removeParticipant(key) },
                            contentAlignment = Alignment.Center
                        ) {
                            Icon(
                                Icons.Filled.Close,
                                contentDescription = "Remove",
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.size(12.dp)
                            )
                        }
                    }
                }
            }
        }

        // Search
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = if (viewModel.participantKeys.isEmpty()) 12.dp else 4.dp)
                .clip(RoundedCornerShape(10.dp))
                .background(MaterialTheme.colorScheme.surfaceContainerHigh)
                .padding(horizontal = 12.dp, vertical = 2.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Icon(
                Icons.Filled.Search,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(18.dp)
            )
            OutlinedTextField(
                value = searchText,
                onValueChange = { v ->
                    onSearchChange(v)
                    val trimmed = v.trim().lowercase()
                    if (trimmed.length == 64 && trimmed.all { it in '0'..'9' || it in 'a'..'f' }) {
                        viewModel.keyInput = trimmed
                        viewModel.addParticipant(keyManager.keyAgreementPublicKeyHex)
                        onSearchChange("")
                    }
                },
                placeholder = { Text("Search name or paste invite key", fontSize = 14.sp) },
                singleLine = true,
                modifier = Modifier.weight(1f),
                colors = androidx.compose.material3.OutlinedTextFieldDefaults.colors(
                    unfocusedBorderColor = Color.Transparent,
                    focusedBorderColor = Color.Transparent,
                    focusedContainerColor = Color.Transparent,
                    unfocusedContainerColor = Color.Transparent
                )
            )
            if (searchText.isNotEmpty()) {
                IconButton(onClick = { onSearchChange("") }, modifier = Modifier.size(28.dp)) {
                    Icon(
                        Icons.Filled.Close,
                        contentDescription = "Clear",
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(16.dp)
                    )
                }
            }
        }

        // From your chats
        if (filtered.isNotEmpty()) {
            SectionHeader("FROM YOUR CHATS")
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp)
                    .clip(RoundedCornerShape(14.dp))
                    .background(MaterialTheme.colorScheme.surfaceContainerHigh)
            ) {
                for ((index, contact) in filtered.withIndex()) {
                    ContactRow(
                        alias = contact.alias ?: (contact.blsHex.take(12) + "…"),
                        subtitle = contact.subtitle,
                        seed = contact.blsHex,
                        selected = viewModel.participantKeys.contains(contact.inboxHex),
                        onTap = {
                            if (viewModel.participantKeys.contains(contact.inboxHex)) {
                                viewModel.removeParticipant(contact.inboxHex)
                            } else {
                                viewModel.keyInput = contact.inboxHex
                                viewModel.addParticipant(keyManager.keyAgreementPublicKeyHex)
                            }
                        }
                    )
                    if (index < filtered.size - 1) {
                        HorizontalDivider(
                            modifier = Modifier.padding(start = 70.dp),
                            color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f)
                        )
                    }
                }
            }
        }

        // Invite by key advanced row
        SectionHeader(
            if (filtered.isEmpty()) "ADD PEOPLE" else "",
            topPadding = if (filtered.isEmpty()) 0.dp else 20.dp
        )
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp)
                .clip(RoundedCornerShape(14.dp))
                .background(MaterialTheme.colorScheme.surfaceContainerHigh)
                .clickable(onClick = onShowKeyDialog)
                .padding(horizontal = 14.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Box(
                modifier = Modifier
                    .size(36.dp)
                    .clip(RoundedCornerShape(10.dp))
                    .background(MaterialTheme.colorScheme.surfaceContainerHighest),
                contentAlignment = Alignment.Center
            ) {
                Icon(
                    Icons.Filled.VpnKey,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.size(16.dp)
                )
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text("Invite by inbox key", fontSize = 14.sp)
                Text(
                    "Paste or scan a 64-char key",
                    fontSize = 11.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            Icon(
                Icons.Filled.ChevronRight,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(18.dp)
            )
        }

        if (filtered.isEmpty()) {
            Text(
                "People you've chatted with will appear here. Until then, paste a contact's inbox key to invite them.",
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 20.dp, vertical = 12.dp)
            )
        }

        viewModel.errorMessage?.let { error ->
            Text(
                error,
                color = MaterialTheme.colorScheme.error,
                fontSize = 12.sp,
                modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp)
            )
        }

        Spacer(Modifier.height(40.dp))
    }
}

@Composable
private fun SectionHeader(title: String, topPadding: androidx.compose.ui.unit.Dp = 16.dp) {
    if (title.isEmpty()) return
    Text(
        title,
        fontSize = 11.sp,
        fontWeight = FontWeight.SemiBold,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(start = 20.dp, top = topPadding, bottom = 6.dp)
    )
}

private data class ChatContact(
    val blsHex: String,
    val inboxHex: String,
    val alias: String?,
    val subtitle: String
)

@Composable
private fun ContactRow(
    alias: String,
    subtitle: String,
    seed: String,
    selected: Boolean,
    onTap: () -> Unit
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(
                if (selected) MaterialTheme.colorScheme.primary.copy(alpha = 0.08f)
                else Color.Transparent
            )
            .clickable(onClick = onTap)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        SeedAvatar(seed = seed, size = 42.dp, name = alias)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(
                alias,
                fontSize = 15.sp,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
            if (subtitle.isNotEmpty()) {
                Text(
                    subtitle,
                    fontSize = 11.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis
                )
            }
        }
        Box(
            modifier = Modifier
                .size(24.dp)
                .clip(CircleShape)
                .background(
                    if (selected) MaterialTheme.colorScheme.primary else Color.Transparent
                )
                .border(
                    width = if (selected) 0.dp else 1.5.dp,
                    color = if (selected) Color.Transparent else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f),
                    shape = CircleShape
                ),
            contentAlignment = Alignment.Center
        ) {
            if (selected) {
                Icon(
                    Icons.Filled.Check,
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier.size(14.dp)
                )
            }
        }
    }
}

@Composable
private fun SeedAvatar(seed: String, size: androidx.compose.ui.unit.Dp, name: String) {
    val accent = GroupAccent.fromSeed(seed)
    val initials = initialsFor(name)
    Box(
        modifier = Modifier
            .size(size)
            .clip(CircleShape)
            .background(accent.brush()),
        contentAlignment = Alignment.Center
    ) {
        Text(
            initials,
            color = Color.White,
            fontSize = (size.value * 0.38f).sp,
            fontWeight = FontWeight.Bold
        )
    }
}

private fun initialsFor(name: String): String {
    val parts = name.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }
    if (parts.isEmpty()) return "?"
    if (parts.size == 1) return parts[0].take(2).uppercase()
    val first = parts[0].firstOrNull()?.toString() ?: ""
    val second = parts[1].firstOrNull()?.toString() ?: ""
    return (first + second).uppercase()
}

// MARK: - Avatar

@Composable
private fun GroupAvatar(
    size: androidx.compose.ui.unit.Dp,
    accent: GroupAccent,
    name: String,
    filled: Boolean,
    corner: androidx.compose.ui.unit.Dp,
    avatarBytes: ByteArray? = null
) {
    val imageBitmap: ImageBitmap? = remember(avatarBytes) {
        avatarBytes?.let { bytes ->
            try {
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
            } catch (_: Exception) { null }
        }
    }
    val brush: Brush = if (filled) accent.brush() else SolidColor(MaterialTheme.colorScheme.surfaceContainerHighest)
    Box(
        modifier = Modifier
            .size(size)
            .clip(RoundedCornerShape(corner))
            .background(brush),
        contentAlignment = Alignment.Center
    ) {
        when {
            imageBitmap != null -> Image(
                bitmap = imageBitmap,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.size(size).clip(RoundedCornerShape(corner))
            )
            filled -> Text(
                initialsFor(name),
                color = Color.White,
                fontSize = (size.value * 0.36f).sp,
                fontWeight = FontWeight.Bold
            )
            else -> Icon(
                Icons.Filled.PhotoCamera,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(size * 0.26f)
            )
        }
    }
}

// MARK: - Progress stage

@Composable
private fun ProgressStage(
    viewModel: CreateGroupViewModel,
    groupListViewModel: GroupListViewModel,
    invitationTransport: InvitationTransport,
    keyManager: chat.onym.android.crypto.KeyManager,
    accent: GroupAccent
) {
    val transition = rememberInfiniteTransition(label = "spinnerRing")
    val angle by transition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(
            animation = tween(durationMillis = 1500, easing = LinearEasing)
        ),
        label = "angle"
    )

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState()),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Spacer(Modifier.height(32.dp))

        // Animated hero avatar with ring
        Box(contentAlignment = Alignment.Center, modifier = Modifier.size(140.dp)) {
            androidx.compose.foundation.Canvas(modifier = Modifier.size(140.dp)) {
                drawCircle(
                    color = accent.primary.copy(alpha = 0.2f),
                    radius = size.minDimension / 2 - 1.dp.toPx(),
                    style = Stroke(width = 2.dp.toPx())
                )
                drawRotate(angle) {
                    drawArc(
                        color = accent.primary,
                        startAngle = 0f,
                        sweepAngle = 360f * 0.16f,
                        useCenter = false,
                        topLeft = Offset(1.dp.toPx(), 1.dp.toPx()),
                        size = Size(size.width - 2.dp.toPx(), size.height - 2.dp.toPx()),
                        style = Stroke(width = 2.dp.toPx())
                    )
                }
            }
            Box(
                modifier = Modifier.shadow(
                    elevation = 16.dp,
                    shape = RoundedCornerShape(60.dp),
                    ambientColor = accent.shadow,
                    spotColor = accent.shadow
                )
            ) {
                GroupAvatar(
                    size = 120.dp,
                    accent = accent,
                    name = viewModel.groupName,
                    filled = true,
                    corner = 60.dp,
                    avatarBytes = viewModel.avatarBytes
                )
            }
        }

        Spacer(Modifier.height(32.dp))

        // Steps card
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp)
                .clip(RoundedCornerShape(14.dp))
                .background(MaterialTheme.colorScheme.surfaceContainerHigh)
        ) {
            ProgressRow(
                title = "Setting up encrypted group",
                subtitle = "Generating keys on your device",
                state = stageStateFor(CreationPhase.CREATING, viewModel.phase)
            )
            if (groupListViewModel.isContractConfigured) {
                HorizontalDivider(
                    modifier = Modifier.padding(start = 52.dp),
                    color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f)
                )
                val onChain = viewModel.onChainStatus
                val failed = onChain is OnChainPublishStatus.Failed
                val subtitle = when (onChain) {
                    is OnChainPublishStatus.Failed -> onChain.reason
                    is OnChainPublishStatus.Published -> "Verified publicly"
                    else -> "So anyone can verify this group is real"
                }
                ProgressRow(
                    title = if (failed) "Anchoring on Stellar (failed)" else "Anchoring on Stellar",
                    subtitle = subtitle,
                    state = stageStateFor(CreationPhase.PUBLISHING, viewModel.phase),
                    failed = failed
                )
                if (failed) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(start = 52.dp, end = 16.dp, bottom = 12.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        TextButton(onClick = {
                            viewModel.retryOnChainPublish(groupListViewModel, invitationTransport, keyManager)
                        }) { Text("Retry") }
                        TextButton(
                            onClick = {
                                viewModel.skipOnChainAndContinue(
                                    invitationTransport,
                                    keyManager,
                                    groupListViewModel.relayURLs.toList()
                                )
                            },
                            colors = ButtonDefaults.textButtonColors(contentColor = Color(0xFFFF9800))
                        ) { Text("Skip") }
                    }
                }
            }
            if (viewModel.participantKeys.isNotEmpty()) {
                HorizontalDivider(
                    modifier = Modifier.padding(start = 52.dp),
                    color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f)
                )
                val count = viewModel.participantKeys.size
                ProgressRow(
                    title = "Sending $count invitation${if (count == 1) "" else "s"}",
                    subtitle = "Delivered via relays",
                    state = stageStateFor(CreationPhase.INVITING, viewModel.phase)
                )
            }
        }

        Spacer(Modifier.height(20.dp))
        Text(
            "This usually takes a few seconds. It's safe to close this — we'll finish in the background.",
            fontSize = 11.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(horizontal = 32.dp)
        )
        Spacer(Modifier.height(20.dp))
    }
}

private enum class StageState { PENDING, ACTIVE, DONE }

private fun stageStateFor(target: CreationPhase, current: CreationPhase): StageState {
    val order = listOf(CreationPhase.CREATING, CreationPhase.PUBLISHING, CreationPhase.INVITING, CreationPhase.DONE)
    val currentIdx = order.indexOf(current)
    val targetIdx = order.indexOf(target)
    if (currentIdx < 0 || targetIdx < 0) return StageState.PENDING
    return when {
        currentIdx > targetIdx -> StageState.DONE
        currentIdx == targetIdx -> StageState.ACTIVE
        else -> StageState.PENDING
    }
}

@Composable
private fun ProgressRow(
    title: String,
    subtitle: String,
    state: StageState,
    failed: Boolean = false
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(14.dp),
        verticalAlignment = Alignment.Top
    ) {
        Box(modifier = Modifier.size(24.dp), contentAlignment = Alignment.Center) {
            when {
                failed -> Icon(
                    Icons.Filled.Error,
                    contentDescription = null,
                    tint = Color(0xFFFF9800),
                    modifier = Modifier.size(20.dp)
                )
                state == StageState.DONE -> Icon(
                    Icons.Filled.Check,
                    contentDescription = null,
                    tint = Color(0xFF30D158),
                    modifier = Modifier.size(20.dp)
                )
                state == StageState.ACTIVE -> CircularProgressIndicator(
                    strokeWidth = 2.dp,
                    modifier = Modifier.size(18.dp)
                )
                else -> Box(
                    modifier = Modifier
                        .size(18.dp)
                        .clip(CircleShape)
                        .border(1.5.dp, MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f), CircleShape)
                )
            }
        }
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(
                title,
                fontSize = 14.sp,
                fontWeight = if (state == StageState.ACTIVE) FontWeight.SemiBold else FontWeight.Medium,
                color = if (state == StageState.PENDING) MaterialTheme.colorScheme.onSurfaceVariant
                else MaterialTheme.colorScheme.onSurface
            )
            Text(
                subtitle,
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}

// MARK: - Done stage

@Composable
private fun DoneStage(
    viewModel: CreateGroupViewModel,
    groupListViewModel: GroupListViewModel,
    accent: GroupAccent,
    context: Context
) {
    val code = viewModel.inviteCode ?: return
    val inviteLink = remember(code) { "https://onym.chat/join?code=$code" }
    var showRawCode by remember { mutableStateOf(false) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState()),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Spacer(Modifier.height(16.dp))

        // Hero avatar + green check badge
        Box(contentAlignment = Alignment.BottomEnd) {
            GroupAvatar(
                size = 84.dp,
                accent = accent,
                name = viewModel.groupName,
                filled = true,
                corner = 42.dp,
                avatarBytes = viewModel.avatarBytes
            )
            Box(
                modifier = Modifier
                    .offset(x = 6.dp, y = 6.dp)
                    .size(32.dp)
                    .clip(CircleShape)
                    .background(Color(0xFF30D158))
                    .border(4.dp, MaterialTheme.colorScheme.surfaceContainerLowest, CircleShape),
                contentAlignment = Alignment.Center
            ) {
                Icon(
                    Icons.Filled.Check,
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier.size(16.dp)
                )
            }
        }

        Spacer(Modifier.height(14.dp))
        Text(viewModel.groupName, fontSize = 22.sp, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(4.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(
                Icons.Filled.Lock,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(12.dp)
            )
            Spacer(Modifier.width(4.dp))
            Text("End-to-end encrypted", fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Spacer(Modifier.height(16.dp))

        // Status chips
        Row(
            modifier = Modifier.padding(horizontal = 20.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            if (groupListViewModel.isContractConfigured) {
                val onChain = viewModel.onChainStatus
                val failed = onChain is OnChainPublishStatus.Failed
                StatusChip(
                    text = if (failed) "On-chain pending" else "Published on-chain",
                    icon = if (failed) Icons.Filled.Error else Icons.Filled.VerifiedUser,
                    color = if (failed) Color(0xFFFF9800) else Color(0xFF30D158)
                )
            }
            if (viewModel.invitationStatuses.isNotEmpty()) {
                val sent = viewModel.invitationStatuses.values.count { it is InvitationStatus.Sent }
                StatusChip(
                    text = "$sent invite${if (sent == 1) "" else "s"} sent",
                    icon = Icons.Filled.Check,
                    color = Color(0xFF30D158)
                )
            }
        }
        Spacer(Modifier.height(16.dp))

        // QR surface
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp)
                .clip(RoundedCornerShape(16.dp))
                .background(MaterialTheme.colorScheme.surfaceContainerHigh)
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 18.dp),
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Box(
                    modifier = Modifier
                        .clip(RoundedCornerShape(12.dp))
                        .background(Color.White)
                        .padding(8.dp)
                ) {
                    QRCodeImage(text = code, size = 168.dp)
                }
                Spacer(Modifier.height(10.dp))
                Text("Invite more people", fontSize = 13.sp, fontWeight = FontWeight.Medium)
                Text(
                    "Scan this QR, or share the link",
                    fontSize = 11.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f))
            Row(modifier = Modifier.fillMaxWidth()) {
                TextButton(
                    modifier = Modifier.weight(1f).padding(vertical = 6.dp),
                    onClick = {
                        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                        clipboard.setPrimaryClip(ClipData.newPlainText("Invite Link", inviteLink))
                        Toast.makeText(context, "Link copied", Toast.LENGTH_SHORT).show()
                    }
                ) {
                    Icon(Icons.Filled.Link, contentDescription = null, modifier = Modifier.size(16.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Copy Link")
                }
                Box(
                    modifier = Modifier
                        .width(1.dp)
                        .height(36.dp)
                        .background(MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f))
                )
                TextButton(
                    modifier = Modifier.weight(1f).padding(vertical = 6.dp),
                    onClick = {
                        val sendIntent = Intent(Intent.ACTION_SEND).apply {
                            type = "text/plain"
                            putExtra(Intent.EXTRA_TEXT, "Join my group on Stellar Chat: $inviteLink")
                        }
                        context.startActivity(Intent.createChooser(sendIntent, "Share invite"))
                    }
                ) {
                    Icon(Icons.Filled.Share, contentDescription = null, modifier = Modifier.size(16.dp))
                    Spacer(Modifier.width(6.dp))
                    Text("Share…")
                }
            }
        }

        Spacer(Modifier.height(16.dp))

        // Advanced: raw invite code
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp)
                .clip(RoundedCornerShape(14.dp))
                .background(MaterialTheme.colorScheme.surfaceContainerHigh)
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { showRawCode = !showRawCode }
                    .padding(horizontal = 16.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text("Show raw invite code", fontSize = 14.sp, modifier = Modifier.weight(1f))
                Icon(
                    Icons.Filled.ChevronRight,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier
                        .size(18.dp)
                        .rotate(if (showRawCode) 90f else 0f)
                )
            }
            if (showRawCode) {
                HorizontalDivider(
                    modifier = Modifier.padding(horizontal = 16.dp),
                    color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f)
                )
                Column(modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
                    Text(
                        code,
                        fontSize = 11.sp,
                        fontFamily = FontFamily.Monospace,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 6,
                        overflow = TextOverflow.Ellipsis
                    )
                    Spacer(Modifier.height(8.dp))
                    TextButton(
                        onClick = {
                            val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                            clipboard.setPrimaryClip(ClipData.newPlainText("Invite Code", code))
                            Toast.makeText(context, "Code copied", Toast.LENGTH_SHORT).show()
                        },
                        contentPadding = PaddingValues(horizontal = 0.dp)
                    ) {
                        Icon(Icons.Filled.ContentCopy, contentDescription = null, modifier = Modifier.size(14.dp))
                        Spacer(Modifier.width(6.dp))
                        Text("Copy code", fontSize = 12.sp)
                    }
                }
            }
        }

        viewModel.errorMessage?.let { error ->
            Spacer(Modifier.height(12.dp))
            Text(
                error,
                color = MaterialTheme.colorScheme.error,
                fontSize = 12.sp,
                modifier = Modifier.padding(horizontal = 20.dp)
            )
        }

        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun StatusChip(
    text: String,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    color: Color
) {
    Row(
        modifier = Modifier
            .clip(RoundedCornerShape(50))
            .background(color.copy(alpha = 0.12f))
            .padding(horizontal = 12.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Icon(icon, contentDescription = null, tint = color, modifier = Modifier.size(12.dp))
        Spacer(Modifier.width(4.dp))
        Text(text, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = color)
    }
}

// MARK: - Invite by Key dialog

@Composable
private fun InviteByKeyDialog(
    ownInboxKey: String,
    existing: List<String>,
    onAdd: (String) -> Unit,
    onShowScanner: () -> Unit,
    onDismiss: () -> Unit
) {
    var keyInput by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val context = LocalContext.current

    fun isValidKey(k: String): Boolean {
        val t = k.trim()
        return t.length == 64 && t.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }
    }

    fun submit() {
        val trimmed = keyInput.trim().lowercase()
        when {
            !isValidKey(trimmed) -> error = "Key must be exactly 64 hex characters."
            trimmed == ownInboxKey.lowercase() -> error = "Cannot add your own inbox key."
            existing.contains(trimmed) -> error = "This key has already been added."
            else -> {
                onAdd(trimmed)
                onDismiss()
            }
        }
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Invite by Inbox Key", fontWeight = FontWeight.SemiBold) },
        text = {
            Column {
                Text(
                    "Ask for their inbox key — they can find it in Settings → Advanced, or share a QR code from there.",
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Spacer(Modifier.height(12.dp))
                OutlinedTextField(
                    value = keyInput,
                    onValueChange = { keyInput = it; error = null },
                    placeholder = { Text("Paste 64-char inbox key", fontSize = 12.sp) },
                    modifier = Modifier.fillMaxWidth(),
                    textStyle = androidx.compose.ui.text.TextStyle(
                        fontFamily = FontFamily.Monospace,
                        fontSize = 12.sp
                    ),
                    maxLines = 3,
                    shape = RoundedCornerShape(14.dp)
                )
                error?.let {
                    Spacer(Modifier.height(6.dp))
                    Text(it, color = MaterialTheme.colorScheme.error, fontSize = 11.sp)
                }
                Spacer(Modifier.height(12.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(
                        onClick = onShowScanner,
                        modifier = Modifier.weight(1f)
                    ) {
                        Icon(Icons.Filled.QrCodeScanner, contentDescription = null, modifier = Modifier.size(16.dp))
                        Spacer(Modifier.width(6.dp))
                        Text("Scan QR", fontSize = 12.sp)
                    }
                    TextButton(
                        onClick = {
                            val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                            val clip = clipboard.primaryClip
                            if (clip != null && clip.itemCount > 0) {
                                keyInput = clip.getItemAt(0).text?.toString()?.trim() ?: ""
                            }
                        },
                        modifier = Modifier.weight(1f)
                    ) {
                        Icon(Icons.Filled.ContentPaste, contentDescription = null, modifier = Modifier.size(16.dp))
                        Spacer(Modifier.width(6.dp))
                        Text("Paste", fontSize = 12.sp)
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = { submit() }, enabled = isValidKey(keyInput)) {
                Text("Add", fontWeight = FontWeight.SemiBold)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        }
    )
}
