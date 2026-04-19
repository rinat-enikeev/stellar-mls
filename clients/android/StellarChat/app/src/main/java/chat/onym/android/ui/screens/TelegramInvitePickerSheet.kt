package chat.onym.android.ui.screens

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import chat.onym.android.contacts.PhoneContact
import chat.onym.android.model.ChatGroup
import chat.onym.android.model.InviteCode
import chat.onym.android.persistence.InvitedContact
import chat.onym.android.persistence.InvitedContactKind
import chat.onym.android.persistence.InvitedContactStatus
import chat.onym.android.persistence.InvitedContactStore
import chat.onym.android.persistence.StellarChatDao
import kotlinx.coroutines.launch
import java.net.URLEncoder
import java.security.SecureRandom
import java.util.Date

private enum class InviteMode { GROUP, ONBOARD }

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TelegramInvitePickerSheet(
    contact: PhoneContact,
    groups: List<ChatGroup>,
    myKeyAgreementPublicKeyHex: String,
    invitedContactStore: InvitedContactStore,
    dao: StellarChatDao,
    onDismiss: () -> Unit
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)

    var mode by remember { mutableStateOf(InviteMode.ONBOARD) }
    var selectedGroupID by remember { mutableStateOf(groups.firstOrNull()?.id) }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 8.dp)
        ) {
            Text(
                "Invite ${contact.displayName}",
                style = MaterialTheme.typography.titleMedium
            )
            Text(
                contact.phone,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            Spacer(modifier = Modifier.height(16.dp))

            ListItem(
                headlineContent = { Text("Start a personal chat") },
                supportingContent = {
                    Text(
                        "Opens a fresh 1:1 chat with you once they install Onym.",
                        style = MaterialTheme.typography.bodySmall
                    )
                },
                leadingContent = {
                    RadioButton(
                        selected = mode == InviteMode.ONBOARD,
                        onClick = { mode = InviteMode.ONBOARD }
                    )
                },
                modifier = Modifier.fillMaxWidth()
            )

            ListItem(
                headlineContent = { Text("Invite to a group") },
                supportingContent = {
                    Text(
                        if (groups.isEmpty()) "You have no groups to invite to."
                        else "Pick one of your groups below.",
                        style = MaterialTheme.typography.bodySmall
                    )
                },
                leadingContent = {
                    RadioButton(
                        selected = mode == InviteMode.GROUP,
                        onClick = { if (groups.isNotEmpty()) mode = InviteMode.GROUP }
                    )
                },
                modifier = Modifier.fillMaxWidth()
            )

            if (mode == InviteMode.GROUP && groups.isNotEmpty()) {
                HorizontalDivider()
                LazyColumn(modifier = Modifier.height(200.dp).fillMaxWidth()) {
                    items(groups, key = { it.id }) { group ->
                        ListItem(
                            headlineContent = { Text(group.name) },
                            leadingContent = {
                                RadioButton(
                                    selected = selectedGroupID == group.id,
                                    onClick = { selectedGroupID = group.id }
                                )
                            },
                            modifier = Modifier.fillMaxWidth()
                        )
                    }
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            Button(
                onClick = {
                    val (url, kind, nonce, groupID) = when (mode) {
                        InviteMode.ONBOARD -> buildOnboardInvite(myKeyAgreementPublicKeyHex)
                        InviteMode.GROUP -> {
                            val group = groups.find { it.id == selectedGroupID }
                                ?: return@Button
                            buildGroupInvite(group)
                        }
                    }
                    scope.launch {
                        val record = InvitedContact(
                            phone = contact.phone,
                            displayName = contact.displayName,
                            kind = kind,
                            inviteURL = url,
                            handshakeNonce = nonce,
                            groupID = groupID,
                            invitedAt = Date(),
                            status = InvitedContactStatus.PENDING,
                            resolvedPubkey = null
                        )
                        invitedContactStore.upsert(record, dao)
                        val launched = launchTelegram(context, url)
                        if (launched) {
                            invitedContactStore.setStatus(
                                contact.phone,
                                InvitedContactStatus.SENT,
                                dao
                            )
                        }
                        onDismiss()
                    }
                },
                enabled = mode == InviteMode.ONBOARD
                    || (mode == InviteMode.GROUP && selectedGroupID != null),
                modifier = Modifier.fillMaxWidth()
            ) {
                Text("Open Telegram")
            }

            Spacer(modifier = Modifier.height(16.dp))
        }
    }
}

private data class InvitePayload(
    val url: String,
    val kind: InvitedContactKind,
    val nonce: String?,
    val groupID: String?
)

private operator fun InvitePayload.component1() = url
private operator fun InvitePayload.component2() = kind
private operator fun InvitePayload.component3() = nonce
private operator fun InvitePayload.component4() = groupID

private fun buildOnboardInvite(myKeyHex: String): InvitePayload {
    val nonce = randomNonceHex(8)
    val url = "https://onym.chat/onboard?inviter=$myKeyHex&nonce=$nonce"
    return InvitePayload(url, InvitedContactKind.ONBOARD, nonce, null)
}

private fun buildGroupInvite(group: ChatGroup): InvitePayload {
    val invite = InviteCode(
        groupID = group.groupIDData,
        groupSecret = group.groupSecret,
        name = group.name,
        relayHints = group.relayHints,
        members = group.members.toList(),
        epoch = group.epoch,
        salt = group.salt,
        commitment = group.commitment,
        tierRawValue = group.tier.id,
        groupTypeRawValue = group.groupType.id,
        adminPubkeys = group.adminPubkeys.toList()
    )
    val url = "https://onym.chat/join?code=${invite.encode()}"
    return InvitePayload(url, InvitedContactKind.GROUP, null, group.id)
}

private fun randomNonceHex(bytes: Int): String {
    val buf = ByteArray(bytes)
    SecureRandom().nextBytes(buf)
    return buf.joinToString("") { "%02x".format(it) }
}

// Telegram silently drops the `text` param on tg://resolve?phone=… links, so we use
// a share intent targeted at Telegram's package — it reliably honors EXTRA_TEXT and
// lets the user pick the recipient from Telegram's own contact picker.
internal fun launchTelegram(context: Context, inviteURL: String): Boolean {
    val cta = "Join me on Onym — end-to-end encrypted chat. $inviteURL"

    val sendIntent = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_TEXT, cta)
        setPackage("org.telegram.messenger")
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    if (sendIntent.resolveActivity(context.packageManager) != null) {
        try {
            context.startActivity(sendIntent)
            return true
        } catch (_: Exception) {
            // fall through to web
        }
    }
    return fallbackWeb(context, inviteURL, cta)
}

private fun fallbackWeb(context: Context, inviteURL: String, cta: String): Boolean {
    val encodedURL = URLEncoder.encode(inviteURL, "UTF-8")
    val encodedText = URLEncoder.encode(cta, "UTF-8")
    val webUri = Uri.parse("https://t.me/share/url?url=$encodedURL&text=$encodedText")
    val webIntent = Intent(Intent.ACTION_VIEW, webUri).apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    return try {
        context.startActivity(webIntent)
        true
    } catch (_: Exception) {
        false
    }
}
