package chat.onym.android.ui.screens

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.view.WindowManager
import android.widget.Toast
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity

/**
 * Displays the BIP39 recovery phrase behind biometric/passcode authentication.
 * FLAG_SECURE is set to prevent screenshots while the phrase is visible.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RecoveryPhraseScreen(
    recoveryPhrase: String,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    val words = recoveryPhrase.split(" ")
    var authenticated by remember { mutableStateOf(false) }
    var authError by remember { mutableStateOf<String?>(null) }

    // Set FLAG_SECURE while this screen is visible
    DisposableEffect(Unit) {
        val activity = context as? android.app.Activity
        activity?.window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onDispose {
            activity?.window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        }
    }

    // Trigger biometric/passcode authentication on first composition
    DisposableEffect(Unit) {
        val activity = context as? FragmentActivity
        if (activity == null) {
            // Fallback: allow access if we can't get FragmentActivity
            authenticated = true
            onDispose {}
        } else {
            val biometricManager = BiometricManager.from(context)
            val canAuth = biometricManager.canAuthenticate(
                BiometricManager.Authenticators.BIOMETRIC_STRONG or
                    BiometricManager.Authenticators.DEVICE_CREDENTIAL
            )
            if (canAuth != BiometricManager.BIOMETRIC_SUCCESS) {
                // No biometric/passcode set — allow access (like iOS)
                authenticated = true
            } else {
                val executor = ContextCompat.getMainExecutor(context)
                val callback = object : BiometricPrompt.AuthenticationCallback() {
                    override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                        authenticated = true
                        authError = null
                    }
                    override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                        authError = errString.toString()
                    }
                    override fun onAuthenticationFailed() {
                        authError = "Authentication failed"
                    }
                }
                val prompt = BiometricPrompt(activity, executor, callback)
                val promptInfo = BiometricPrompt.PromptInfo.Builder()
                    .setTitle("View Recovery Phrase")
                    .setSubtitle("Authenticate to view your recovery phrase")
                    .setAllowedAuthenticators(
                        BiometricManager.Authenticators.BIOMETRIC_STRONG or
                            BiometricManager.Authenticators.DEVICE_CREDENTIAL
                    )
                    .build()
                prompt.authenticate(promptInfo)
            }
            onDispose {}
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Recovery Phrase") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                }
            )
        }
    ) { padding ->
        if (!authenticated) {
            // Show authentication gate
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding)
                    .padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center
            ) {
                Icon(
                    Icons.Default.Lock,
                    contentDescription = null,
                    modifier = Modifier.padding(bottom = 16.dp),
                    tint = MaterialTheme.colorScheme.onSurfaceVariant
                )
                if (authError != null) {
                    Text(
                        authError ?: "Authentication failed",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.error,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.padding(bottom = 16.dp)
                    )
                    Button(onClick = {
                        authError = null
                        val activity = context as? FragmentActivity ?: return@Button
                        val executor = ContextCompat.getMainExecutor(context)
                        val callback = object : BiometricPrompt.AuthenticationCallback() {
                            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                                authenticated = true
                                authError = null
                            }
                            override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                                authError = errString.toString()
                            }
                            override fun onAuthenticationFailed() {
                                authError = "Authentication failed"
                            }
                        }
                        val prompt = BiometricPrompt(activity, executor, callback)
                        val promptInfo = BiometricPrompt.PromptInfo.Builder()
                            .setTitle("View Recovery Phrase")
                            .setSubtitle("Authenticate to view your recovery phrase")
                            .setAllowedAuthenticators(
                                BiometricManager.Authenticators.BIOMETRIC_STRONG or
                                    BiometricManager.Authenticators.DEVICE_CREDENTIAL
                            )
                            .build()
                        prompt.authenticate(promptInfo)
                    }) {
                        Text("Try Again")
                    }
                } else {
                    Text(
                        "Verify your identity to view the recovery phrase.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center
                    )
                }
            }
            return@Scaffold
        }

        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(16.dp)
                .verticalScroll(rememberScrollState())
        ) {
            // Warning card
            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.errorContainer.copy(alpha = 0.3f)
                )
            ) {
                Row(
                    modifier = Modifier.padding(16.dp),
                    verticalAlignment = Alignment.Top
                ) {
                    Icon(
                        Icons.Default.Warning,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.error,
                        modifier = Modifier.padding(end = 12.dp)
                    )
                    Text(
                        "Write down these words in order. Store them securely offline. Anyone with this phrase can access your identity.",
                        style = MaterialTheme.typography.bodyMedium
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            // Word grid (3 columns)
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text(
                        "Recovery Phrase",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.Bold
                    )
                    Spacer(modifier = Modifier.height(12.dp))

                    for ((rowIndex, row) in words.chunked(3).withIndex()) {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = 4.dp),
                            horizontalArrangement = Arrangement.SpaceEvenly
                        ) {
                            for ((i, word) in row.withIndex()) {
                                val index = rowIndex * 3 + i + 1
                                Row(
                                    modifier = Modifier.weight(1f),
                                    verticalAlignment = Alignment.CenterVertically
                                ) {
                                    Text(
                                        "$index.",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        modifier = Modifier.width(24.dp)
                                    )
                                    Text(
                                        word,
                                        style = MaterialTheme.typography.bodyMedium,
                                        fontWeight = FontWeight.Medium,
                                        fontFamily = FontFamily.Monospace
                                    )
                                }
                            }
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(16.dp))

            // Copy button
            OutlinedButton(
                onClick = {
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    clipboard.setPrimaryClip(ClipData.newPlainText("Recovery Phrase", recoveryPhrase))
                    Toast.makeText(context, "Copied. Will clear in 60 seconds.", Toast.LENGTH_LONG).show()
                    // Clear clipboard after 60 seconds
                    Handler(Looper.getMainLooper()).postDelayed({
                        val current = clipboard.primaryClip
                        if (current != null && current.itemCount > 0 &&
                            current.getItemAt(0).text?.toString() == recoveryPhrase
                        ) {
                            clipboard.setPrimaryClip(ClipData.newPlainText("", ""))
                        }
                    }, 60_000)
                },
                modifier = Modifier.fillMaxWidth()
            ) {
                Icon(Icons.Default.ContentCopy, contentDescription = null, modifier = Modifier.padding(end = 8.dp))
                Text("Copy to Clipboard")
            }

            Spacer(modifier = Modifier.height(8.dp))

            Text(
                "Never share your recovery phrase. Never enter it on a website. Stellar Chat will never ask for it outside of the restore flow.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )

            Spacer(modifier = Modifier.height(16.dp))
        }
    }
}
