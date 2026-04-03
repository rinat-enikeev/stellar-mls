package com.stellarmls.chat.ui.screens

import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.stellarmls.chat.model.PendingInvitation
import com.stellarmls.chat.onchain.OnChainVerificationResult
import com.stellarmls.chat.ui.components.VerificationBadge
import com.stellarmls.chat.viewmodel.GroupListViewModel

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PendingInvitationsScreen(
    invitations: List<PendingInvitation>,
    groupListViewModel: GroupListViewModel? = null,
    onAccept: (PendingInvitation, OnChainVerificationResult?) -> Unit,
    onDecline: (PendingInvitation) -> Unit,
    onBack: () -> Unit
) {
    val verificationResults = remember { mutableStateMapOf<String, OnChainVerificationResult>() }
    val verifyingIDs = remember { mutableStateListOf<String>() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Invitations") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                }
            )
        }
    ) { padding ->
        if (invitations.isEmpty()) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text(
                    "No pending invitations",
                    style = MaterialTheme.typography.titleLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
            }
        } else {
            LazyColumn(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding)
                    .padding(horizontal = 16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                items(invitations, key = { it.id }) { invitation ->
                    // Auto-verify on-chain when card appears
                    if (groupListViewModel?.isContractConfigured == true &&
                        !verificationResults.containsKey(invitation.id) &&
                        !verifyingIDs.contains(invitation.id)
                    ) {
                        LaunchedEffect(invitation.id) {
                            verifyingIDs.add(invitation.id)
                            val tempGroup = invitation.payload.toChatGroup()
                            groupListViewModel.verifyGroupOnChain(tempGroup) { result ->
                                verificationResults[invitation.id] = result
                                verifyingIDs.remove(invitation.id)
                            }
                        }
                    }

                    InvitationCard(
                        invitation = invitation,
                        verificationResult = verificationResults[invitation.id],
                        isVerifying = verifyingIDs.contains(invitation.id),
                        onAccept = { onAccept(invitation, verificationResults[invitation.id]) },
                        onDecline = { onDecline(invitation) }
                    )
                }
            }
        }
    }
}

@Composable
private fun InvitationCard(
    invitation: PendingInvitation,
    verificationResult: OnChainVerificationResult?,
    isVerifying: Boolean,
    onAccept: () -> Unit,
    onDecline: () -> Unit
) {
    val payload = invitation.payload

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(payload.name, style = MaterialTheme.typography.titleMedium)

            Spacer(modifier = Modifier.height(4.dp))

            Text(
                "From: ${payload.senderNostrPubkey.take(12)}...",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            Text(
                "Members: ${payload.members.size} | Epoch: ${payload.epoch}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )

            // On-chain verification badge
            if (verificationResult != null) {
                Spacer(modifier = Modifier.height(6.dp))
                VerificationBadge(verificationResult)
            } else if (isVerifying) {
                Spacer(modifier = Modifier.height(6.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    CircularProgressIndicator(modifier = Modifier.size(12.dp), strokeWidth = 1.5.dp)
                    Spacer(modifier = Modifier.width(6.dp))
                    Text(
                        "Verifying on-chain...",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }

            Spacer(modifier = Modifier.height(12.dp))

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.End
            ) {
                OutlinedButton(onClick = onDecline) {
                    Text("Decline")
                }
                Spacer(modifier = Modifier.width(8.dp))
                Button(onClick = onAccept) {
                    Text("Accept")
                }
            }
        }
    }
}

