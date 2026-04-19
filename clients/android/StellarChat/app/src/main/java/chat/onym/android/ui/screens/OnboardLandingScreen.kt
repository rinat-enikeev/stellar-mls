package chat.onym.android.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
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
import chat.onym.android.viewmodel.GroupListViewModel
import kotlinx.coroutines.launch

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun OnboardLandingScreen(
    inviterX25519Hex: String,
    nonceHex: String,
    groupListViewModel: GroupListViewModel,
    onDone: () -> Unit
) {
    val scope = rememberCoroutineScope()
    var displayName by remember { mutableStateOf("New Chat") }
    var busy by remember { mutableStateOf(false) }
    var done by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("You're invited") },
                navigationIcon = {
                    IconButton(onClick = onDone) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back"
                        )
                    }
                }
            )
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            Text(
                "Someone invited you to chat on Onym.",
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold
            )
            Text(
                "Accepting will create a private 1:1 chat with the person who invited you. " +
                    "You can rename it at any time.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )

            OutlinedTextField(
                value = displayName,
                onValueChange = { displayName = it },
                label = { Text("Chat name") },
                singleLine = true,
                enabled = !busy && !done,
                modifier = Modifier.fillMaxWidth()
            )

            Text(
                "Inviter key: " + inviterX25519Hex.take(16) + "…",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )

            if (busy) {
                Box(
                    modifier = Modifier.fillMaxWidth(),
                    contentAlignment = Alignment.Center
                ) {
                    CircularProgressIndicator()
                }
            }

            error?.let {
                Text(it, color = Color.Red, style = MaterialTheme.typography.bodySmall)
            }

            if (done) {
                Text(
                    "Chat created — you'll be connected once they come online.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = Color(0xFF43A047)
                )
                Button(
                    onClick = onDone,
                    modifier = Modifier.fillMaxWidth()
                ) { Text("Done") }
            } else {
                Button(
                    onClick = {
                        val name = displayName.trim().ifEmpty { "New Chat" }
                        busy = true
                        error = null
                        scope.launch {
                            val group = groupListViewModel.completeOnboardInvite(
                                inviterKeyAgreementKeyHex = inviterX25519Hex,
                                nonceHex = nonceHex,
                                displayName = name
                            )
                            busy = false
                            if (group != null) {
                                done = true
                            } else {
                                error = "Couldn't complete onboarding. Please try again."
                            }
                        }
                    },
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth()
                ) { Text("Accept invitation") }

                TextButton(
                    onClick = onDone,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth()
                ) { Text("Not now") }
            }
        }
    }
}
