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
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.tween
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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Fingerprint
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import kotlinx.coroutines.delay
import java.text.DateFormat
import java.util.Date

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RecoveryPhraseScreen(
    recoveryPhrase: String,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    var step by remember { mutableStateOf<BackupStep>(BackupStep.Intro) }
    var authError by remember { mutableStateOf<String?>(null) }

    val obscured = step is BackupStep.Reveal || step is BackupStep.Verify
    DisposableEffect(obscured) {
        val activity = context as? android.app.Activity
        if (obscured) {
            activity?.window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        } else {
            activity?.window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        }
        onDispose {
            activity?.window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        }
    }

    fun proceedToReveal() {
        step = BackupStep.Reveal(phrase = recoveryPhrase, revealed = false)
    }

    fun authenticate() {
        val activity = context as? FragmentActivity
        if (activity == null) {
            proceedToReveal()
            return
        }
        val biometricManager = BiometricManager.from(context)
        val canAuth = biometricManager.canAuthenticate(
            BiometricManager.Authenticators.BIOMETRIC_STRONG or
                BiometricManager.Authenticators.DEVICE_CREDENTIAL
        )
        if (canAuth != BiometricManager.BIOMETRIC_SUCCESS) {
            proceedToReveal()
            return
        }
        val executor = ContextCompat.getMainExecutor(context)
        val callback = object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                proceedToReveal()
            }
            override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                if (errorCode == BiometricPrompt.ERROR_USER_CANCELED ||
                    errorCode == BiometricPrompt.ERROR_NEGATIVE_BUTTON ||
                    errorCode == BiometricPrompt.ERROR_CANCELED
                ) return
                authError = errString.toString()
            }
            override fun onAuthenticationFailed() {
                authError = "Authentication failed"
            }
        }
        val prompt = BiometricPrompt(activity, executor, callback)
        val info = BiometricPrompt.PromptInfo.Builder()
            .setTitle("View Recovery Phrase")
            .setSubtitle("Authenticate to reveal your recovery phrase")
            .setAllowedAuthenticators(
                BiometricManager.Authenticators.BIOMETRIC_STRONG or
                    BiometricManager.Authenticators.DEVICE_CREDENTIAL
            )
            .build()
        prompt.authenticate(info)
    }

    val title = when (step) {
        BackupStep.Intro -> "Back up keys"
        is BackupStep.Reveal -> "Recovery phrase"
        is BackupStep.Verify -> "Verify"
        BackupStep.Done -> ""
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title) },
                navigationIcon = {
                    if (step !is BackupStep.Done) {
                        TextButton(onClick = onBack) { Text("Cancel") }
                    }
                }
            )
        },
        containerColor = MaterialTheme.colorScheme.surfaceContainerLowest
    ) { padding ->
        when (val s = step) {
            BackupStep.Intro -> IntroStep(
                paddingValues = padding,
                onContinue = { authenticate() },
                onCancel = onBack
            )
            is BackupStep.Reveal -> RevealStep(
                paddingValues = padding,
                phrase = s.phrase,
                revealed = s.revealed,
                onReveal = { step = s.copy(revealed = true) },
                onCopy = {
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    clipboard.setPrimaryClip(ClipData.newPlainText("Recovery Phrase", s.phrase))
                    Toast.makeText(context, "Copied. Will clear in 60 seconds.", Toast.LENGTH_LONG).show()
                    Handler(Looper.getMainLooper()).postDelayed({
                        val current = clipboard.primaryClip
                        if (current != null && current.itemCount > 0 &&
                            current.getItemAt(0).text?.toString() == s.phrase
                        ) {
                            clipboard.setPrimaryClip(ClipData.newPlainText("", ""))
                        }
                    }, 60_000)
                },
                onContinue = { step = BackupStep.Verify(phrase = s.phrase, rounds = buildRounds(s.phrase), index = 0, state = VerifyState.Idle) }
            )
            is BackupStep.Verify -> VerifyStep(
                paddingValues = padding,
                state = s,
                onPick = { word ->
                    val correct = s.rounds[s.index].correct
                    if (word == correct) {
                        step = s.copy(state = VerifyState.Correct)
                    } else {
                        step = s.copy(state = VerifyState.Wrong(word))
                    }
                }
            )
            BackupStep.Done -> DoneStep(paddingValues = padding, onDone = onBack)
        }
    }

    // Advance verify rounds after a brief success delay
    LaunchedEffect(step) {
        val s = step
        if (s is BackupStep.Verify && s.state is VerifyState.Correct) {
            delay(450)
            step = if (s.index + 1 >= s.rounds.size) {
                BackupStep.Done
            } else {
                s.copy(index = s.index + 1, state = VerifyState.Idle)
            }
        }
    }

    if (authError != null) {
        AlertDialog(
            onDismissRequest = { authError = null },
            title = { Text("Authentication Failed") },
            text = { Text(authError ?: "") },
            confirmButton = {
                TextButton(onClick = { authError = null; authenticate() }) { Text("Try Again") }
            },
            dismissButton = {
                TextButton(onClick = { authError = null; onBack() }) { Text("Cancel") }
            }
        )
    }
}

// MARK: - Model

@Stable
private sealed class BackupStep {
    data object Intro : BackupStep()
    data class Reveal(val phrase: String, val revealed: Boolean) : BackupStep()
    data class Verify(
        val phrase: String,
        val rounds: List<VerifyRound>,
        val index: Int,
        val state: VerifyState
    ) : BackupStep()
    data object Done : BackupStep()
}

private data class VerifyRound(val position: Int, val correct: String, val options: List<String>)

private sealed class VerifyState {
    data object Idle : VerifyState()
    data object Correct : VerifyState()
    data class Wrong(val picked: String) : VerifyState()
}

private fun buildRounds(phrase: String): List<VerifyRound> {
    val words = phrase.split(" ").filter { it.isNotBlank() }
    val positions = (0 until words.size).shuffled().take(3).sorted()
    return positions.map { pos ->
        val correct = words[pos]
        val opts = mutableSetOf(correct)
        val distractors = words.filter { it != correct }.shuffled()
        var i = 0
        while (opts.size < 4 && i < distractors.size) {
            opts.add(distractors[i])
            i++
        }
        // Fallback: pad with alternate random words if phrase has duplicates (unlikely)
        while (opts.size < 4) {
            val candidate = chat.onym.android.crypto.Bip39.randomWord(exclude = opts)
            opts.add(candidate)
        }
        VerifyRound(position = pos + 1, correct = correct, options = opts.toList().shuffled())
    }
}

// MARK: - Intro

@Composable
private fun IntroStep(
    paddingValues: PaddingValues,
    onContinue: () -> Unit,
    onCancel: () -> Unit
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(paddingValues)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 8.dp)
    ) {
        // Hero card
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(18.dp))
                .padding(horizontal = 20.dp, vertical = 22.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Box(
                modifier = Modifier
                    .size(72.dp)
                    .shadow(10.dp, RoundedCornerShape(22.dp))
                    .background(
                        brush = Brush.linearGradient(
                            colors = listOf(MaterialTheme.colorScheme.primary, Color(0xFF9B5DE5))
                        ),
                        shape = RoundedCornerShape(22.dp)
                    ),
                contentAlignment = Alignment.Center
            ) {
                Icon(
                    Icons.Default.Lock,
                    contentDescription = null,
                    tint = Color.White,
                    modifier = Modifier.size(30.dp)
                )
            }
            Spacer(Modifier.height(12.dp))
            Text(
                "Your identity, in 12 words",
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.Bold,
                textAlign = TextAlign.Center
            )
            Spacer(Modifier.height(6.dp))
            Text(
                "Write them down. Keep them offline. This phrase restores your Nostr, Stellar, and BLS keys on any device.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center
            )
        }

        Spacer(Modifier.height(18.dp))

        SectionHeader("BEFORE YOU START")
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(10.dp))
        ) {
            InfoRow(
                icon = Icons.Default.Warning,
                iconBg = Color(0xFFFF9500),
                title = "Never share or photograph",
                separator = true
            )
            InfoRow(
                icon = Icons.Default.Shield,
                iconBg = Color(0xFF34C759),
                title = "Store offline (paper or metal)",
                separator = true
            )
            InfoRow(
                icon = Icons.Default.Lock,
                iconBg = Color(0xFF8E8E93),
                title = "Anyone with it can read your chats",
                separator = false
            )
        }

        Spacer(Modifier.height(22.dp))

        Button(
            onClick = onContinue,
            modifier = Modifier.fillMaxWidth().height(52.dp),
            shape = RoundedCornerShape(14.dp)
        ) {
            Icon(Icons.Default.Fingerprint, contentDescription = null)
            Spacer(Modifier.width(8.dp))
            Text("Continue", fontWeight = FontWeight.SemiBold)
        }
        Spacer(Modifier.height(10.dp))
        Button(
            onClick = onCancel,
            modifier = Modifier.fillMaxWidth().height(52.dp),
            shape = RoundedCornerShape(14.dp),
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainer,
                contentColor = MaterialTheme.colorScheme.primary
            )
        ) {
            Text("Not now", fontWeight = FontWeight.SemiBold)
        }

        Spacer(Modifier.height(28.dp))
    }
}

@Composable
private fun SectionHeader(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.labelMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp)
    )
}

@Composable
private fun InfoRow(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    iconBg: Color,
    title: String,
    separator: Boolean
) {
    Column {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Box(
                modifier = Modifier
                    .size(30.dp)
                    .background(iconBg, RoundedCornerShape(7.dp)),
                contentAlignment = Alignment.Center
            ) {
                Icon(icon, contentDescription = null, tint = Color.White, modifier = Modifier.size(16.dp))
            }
            Spacer(Modifier.width(12.dp))
            Text(title, style = MaterialTheme.typography.bodyLarge)
        }
        if (separator) {
            Box(
                Modifier
                    .padding(start = 58.dp)
                    .fillMaxWidth()
                    .height(0.5.dp)
                    .background(MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f))
            )
        }
    }
}

// MARK: - Reveal

@Composable
private fun RevealStep(
    paddingValues: PaddingValues,
    phrase: String,
    revealed: Boolean,
    onReveal: () -> Unit,
    onCopy: () -> Unit,
    onContinue: () -> Unit
) {
    val words = phrase.split(" ").filter { it.isNotBlank() }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(paddingValues)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp)
    ) {
        Spacer(Modifier.height(12.dp))
        Text(
            "Write down these ${words.size} words in order. You'll confirm three of them on the next screen.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 16.dp)
        )
        Spacer(Modifier.height(10.dp))

        // Phrase card
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(18.dp))
                .padding(16.dp),
            contentAlignment = Alignment.Center
        ) {
            Column(modifier = Modifier.fillMaxWidth()) {
                for (rowWords in words.chunked(2).mapIndexed { rowIdx, pair -> rowIdx to pair }) {
                    val (rowIdx, pair) = rowWords
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(vertical = 5.dp),
                        horizontalArrangement = Arrangement.spacedBy(14.dp)
                    ) {
                        for ((colIdx, word) in pair.withIndex()) {
                            val index = rowIdx * 2 + colIdx + 1
                            Row(
                                modifier = Modifier
                                    .weight(1f)
                                    .background(
                                        MaterialTheme.colorScheme.onSurface.copy(alpha = 0.03f),
                                        RoundedCornerShape(8.dp)
                                    )
                                    .padding(horizontal = 10.dp, vertical = 7.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Text(
                                    "$index",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.4f),
                                    fontFamily = FontFamily.Monospace,
                                    modifier = Modifier.width(18.dp),
                                    textAlign = TextAlign.End
                                )
                                Spacer(Modifier.width(8.dp))
                                Text(
                                    if (revealed) word else "••••••",
                                    style = MaterialTheme.typography.bodyMedium,
                                    fontWeight = FontWeight.Medium
                                )
                            }
                        }
                        if (pair.size == 1) Spacer(Modifier.weight(1f))
                    }
                }
            }

            if (!revealed) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(MaterialTheme.colorScheme.surfaceContainer.copy(alpha = 0.92f), RoundedCornerShape(18.dp))
                        .clickable { onReveal() },
                    contentAlignment = Alignment.Center
                ) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Box(
                            modifier = Modifier
                                .size(44.dp)
                                .background(MaterialTheme.colorScheme.onSurface.copy(alpha = 0.08f), CircleShape),
                            contentAlignment = Alignment.Center
                        ) {
                            Icon(Icons.Default.VisibilityOff, contentDescription = null, modifier = Modifier.size(22.dp))
                        }
                        Spacer(Modifier.height(10.dp))
                        Text("Tap to reveal", fontWeight = FontWeight.SemiBold)
                    }
                }
            }
        }

        Spacer(Modifier.height(12.dp))

        Button(
            onClick = onCopy,
            enabled = revealed,
            modifier = Modifier.fillMaxWidth().height(46.dp),
            shape = RoundedCornerShape(14.dp),
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.primary.copy(alpha = 0.12f),
                contentColor = MaterialTheme.colorScheme.primary,
                disabledContainerColor = MaterialTheme.colorScheme.primary.copy(alpha = 0.06f),
                disabledContentColor = MaterialTheme.colorScheme.primary.copy(alpha = 0.5f)
            )
        ) {
            Icon(Icons.Default.ContentCopy, contentDescription = null, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text("Copy", fontWeight = FontWeight.SemiBold)
        }

        Spacer(Modifier.height(10.dp))

        Button(
            onClick = onContinue,
            enabled = revealed,
            modifier = Modifier.fillMaxWidth().height(52.dp),
            shape = RoundedCornerShape(14.dp)
        ) {
            Text("I've written it down", fontWeight = FontWeight.SemiBold)
        }

        Spacer(Modifier.height(18.dp))
        Text(
            "Screenshots are blocked on this screen. The phrase is generated on-device and never sent to StellarChat.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 16.dp)
        )
        Spacer(Modifier.height(28.dp))
    }
}

// MARK: - Verify

@Composable
private fun VerifyStep(
    paddingValues: PaddingValues,
    state: BackupStep.Verify,
    onPick: (String) -> Unit
) {
    val round = state.rounds[state.index]
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(paddingValues)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 16.dp)
    ) {
        Spacer(Modifier.height(8.dp))
        ProgressDots(current = state.index, total = state.rounds.size)
        Spacer(Modifier.height(22.dp))

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(18.dp))
                .padding(vertical = 20.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Text(
                "Select word number",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
            Text(
                "${round.position}",
                fontSize = 64.sp,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.primary,
                fontFamily = FontFamily.Monospace
            )
        }

        Spacer(Modifier.height(20.dp))

        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            for (option in round.options) {
                val isCorrect = state.state is VerifyState.Correct && option == round.correct
                val isWrong = state.state is VerifyState.Wrong && state.state.picked == option
                VerifyOption(word = option, isCorrect = isCorrect, isWrong = isWrong) { onPick(option) }
            }
        }

        if (state.state is VerifyState.Wrong) {
            Spacer(Modifier.height(18.dp))
            Text(
                "Not the right word. Check your phrase and try again.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp)
            )
        }

        Spacer(Modifier.height(28.dp))
    }
}

@Composable
private fun ProgressDots(current: Int, total: Int) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically
    ) {
        for (i in 0 until total) {
            val activeWidth by animateDpAsState(
                targetValue = if (i == current) 24.dp else 8.dp,
                animationSpec = tween(200),
                label = "dotWidth"
            )
            Box(
                modifier = Modifier
                    .padding(horizontal = 4.dp)
                    .height(8.dp)
                    .widthIn(min = 8.dp)
                    .width(activeWidth)
                    .background(
                        color = if (i == current) MaterialTheme.colorScheme.primary
                        else MaterialTheme.colorScheme.outlineVariant,
                        shape = RoundedCornerShape(4.dp)
                    )
            )
        }
    }
}

@Composable
private fun VerifyOption(
    word: String,
    isCorrect: Boolean,
    isWrong: Boolean,
    onTap: () -> Unit
) {
    val containerColor = when {
        isCorrect -> Color(0xFF34C759).copy(alpha = 0.14f)
        isWrong -> MaterialTheme.colorScheme.error.copy(alpha = 0.12f)
        else -> MaterialTheme.colorScheme.surfaceContainer
    }
    val borderColor = when {
        isCorrect -> Color(0xFF34C759)
        isWrong -> MaterialTheme.colorScheme.error
        else -> Color.Transparent
    }
    val fg = when {
        isCorrect -> Color(0xFF34C759)
        isWrong -> MaterialTheme.colorScheme.error
        else -> MaterialTheme.colorScheme.onSurface
    }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(containerColor, RoundedCornerShape(14.dp))
            .border(1.5.dp, borderColor, RoundedCornerShape(14.dp))
            .clickable { onTap() }
            .padding(horizontal = 18.dp, vertical = 16.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(word, style = MaterialTheme.typography.bodyLarge, color = fg, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
        if (isCorrect) {
            Box(
                modifier = Modifier.size(22.dp).background(Color(0xFF34C759), CircleShape),
                contentAlignment = Alignment.Center
            ) {
                Icon(Icons.Default.Check, contentDescription = null, tint = Color.White, modifier = Modifier.size(12.dp))
            }
        } else if (isWrong) {
            Box(
                modifier = Modifier.size(22.dp).background(MaterialTheme.colorScheme.error, CircleShape),
                contentAlignment = Alignment.Center
            ) {
                Icon(Icons.Default.Close, contentDescription = null, tint = Color.White, modifier = Modifier.size(12.dp))
            }
        }
    }
}

// MARK: - Done

@Composable
private fun DoneStep(
    paddingValues: PaddingValues,
    onDone: () -> Unit
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(paddingValues)
            .padding(horizontal = 16.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Spacer(Modifier.weight(1f))
        Box(
            modifier = Modifier
                .size(96.dp)
                .shadow(16.dp, CircleShape)
                .background(
                    Brush.linearGradient(listOf(Color(0xFF34C759), Color(0xFF30C155))),
                    CircleShape
                ),
            contentAlignment = Alignment.Center
        ) {
            Icon(Icons.Default.Check, contentDescription = null, tint = Color.White, modifier = Modifier.size(44.dp))
        }
        Spacer(Modifier.height(24.dp))
        Text("Backup verified", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(10.dp))
        Text(
            "Your recovery phrase is confirmed. Store it somewhere safe — you'll only need it if you lose this device.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(horizontal = 28.dp)
        )
        Spacer(Modifier.height(36.dp))
        Button(
            onClick = onDone,
            modifier = Modifier.fillMaxWidth().height(52.dp),
            shape = RoundedCornerShape(14.dp)
        ) {
            Text("Done", fontWeight = FontWeight.SemiBold)
        }
        Spacer(Modifier.weight(1f))
        Text(
            "Backed up ${DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date())} · BIP-39 English",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(bottom = 32.dp, start = 32.dp, end = 32.dp)
        )
    }
}
