package chat.onym.android.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Cancel
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import chat.onym.android.model.ChatGroup
import chat.onym.android.model.EpochSnapshot
import chat.onym.android.model.toHex
import com.stellarmls.mls.SEPGroupMemberLeaf

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ForkGroupScreen(
    originalGroup: ChatGroup,
    myBlsPubkey: ByteArray,
    epochSnapshots: Map<Long, EpochSnapshot>,
    removedByPubkeyHex: String? = null,
    onCreateFork: (excludingMembers: List<ByteArray>, forkName: String?, onResult: (Result<ChatGroup>) -> Unit) -> Unit,
    onNavigateToGroup: (String) -> Unit,
    onBack: () -> Unit
) {
    // Find the latest epoch where user was a member
    val forkSnapshot = remember(epochSnapshots) {
        epochSnapshots.values
            .filter { snapshot -> snapshot.members.any { it.publicKeyCompressed.contentEquals(myBlsPubkey) } }
            .maxByOrNull { it.epoch }
    }

    var forkName by remember { mutableStateOf("Fork of ${originalGroup.name}") }
    val excludedMembers = remember {
        mutableStateMapOf<String, ByteArray>().also { map ->
            // Pre-exclude the member who removed us
            if (removedByPubkeyHex != null && forkSnapshot != null) {
                forkSnapshot.members.find { it.publicKeyCompressed.toHex() == removedByPubkeyHex }?.let {
                    map[removedByPubkeyHex] = it.publicKeyCompressed
                }
            }
        }
    }
    var isCreating by remember { mutableStateOf(false) }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    var createdGroup by remember { mutableStateOf<ChatGroup?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Fork Group") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                }
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(16.dp)
                .verticalScroll(rememberScrollState())
        ) {
            if (forkSnapshot == null) {
                Text(
                    "No epoch snapshot found where you were a member.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.error
                )
                return@Scaffold
            }

            // Source info
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Source", style = MaterialTheme.typography.titleMedium)
                    Spacer(modifier = Modifier.height(8.dp))
                    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text("Fork from", style = MaterialTheme.typography.labelMedium)
                        Text("Epoch ${forkSnapshot.epoch}", style = MaterialTheme.typography.bodyMedium)
                    }
                    Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        Text("Members at epoch", style = MaterialTheme.typography.labelMedium)
                        Text("${forkSnapshot.members.size}", style = MaterialTheme.typography.bodyMedium)
                    }
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        "The fork starts from the last epoch where you were a member.",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            // Fork name
            OutlinedTextField(
                value = forkName,
                onValueChange = { forkName = it },
                label = { Text("Fork name") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth()
            )

            Spacer(modifier = Modifier.height(16.dp))

            // Member list
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Members", style = MaterialTheme.typography.titleMedium)
                    Spacer(modifier = Modifier.height(4.dp))
                    val included = forkSnapshot.members.size - excludedMembers.size
                    Text(
                        "Tap to exclude members. $included member${if (included == 1) "" else "s"} will be in the fork.",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    Spacer(modifier = Modifier.height(8.dp))

                    for (member in forkSnapshot.members) {
                        val isMe = member.publicKeyCompressed.contentEquals(myBlsPubkey)
                        val hex = member.publicKeyCompressed.toHex()
                        val isExcluded = excludedMembers.containsKey(hex)
                        val isRemover = hex == removedByPubkeyHex

                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable(enabled = !isMe && !isCreating) {
                                    if (isExcluded) excludedMembers.remove(hex)
                                    else excludedMembers[hex] = member.publicKeyCompressed
                                }
                                .padding(vertical = 6.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    hex.take(16) + "...",
                                    style = MaterialTheme.typography.bodySmall
                                )
                                if (isMe) {
                                    Text(
                                        "You",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.primary
                                    )
                                }
                                if (isRemover) {
                                    Text(
                                        "Removed you",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.error
                                    )
                                }
                            }
                            Icon(
                                imageVector = if (isMe) Icons.Filled.CheckCircle
                                    else if (isExcluded) Icons.Filled.Cancel
                                    else Icons.Filled.CheckCircle,
                                contentDescription = null,
                                tint = if (isMe) MaterialTheme.colorScheme.primary
                                    else if (isExcluded) MaterialTheme.colorScheme.error
                                    else Color(0xFF4CAF50),
                                modifier = Modifier.size(24.dp)
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            errorMessage?.let { error ->
                Text(error, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                Spacer(modifier = Modifier.height(8.dp))
            }

            if (createdGroup != null) {
                Button(
                    onClick = { createdGroup?.let { onNavigateToGroup(it.id) } },
                    modifier = Modifier.fillMaxWidth()
                ) {
                    Text("Go to Fork")
                }
            } else {
                Button(
                    onClick = {
                        isCreating = true
                        errorMessage = null
                        onCreateFork(
                            excludedMembers.values.toList(),
                            forkName.trim().takeIf { it.isNotEmpty() }
                        ) { result ->
                            isCreating = false
                            result.onSuccess { createdGroup = it }
                            result.onFailure { errorMessage = it.message ?: "Failed to create fork" }
                        }
                    },
                    enabled = !isCreating,
                    modifier = Modifier.fillMaxWidth()
                ) {
                    if (isCreating) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp,
                            color = MaterialTheme.colorScheme.onPrimary
                        )
                    } else {
                        Text("Create Fork")
                    }
                }
            }
        }
    }
}
