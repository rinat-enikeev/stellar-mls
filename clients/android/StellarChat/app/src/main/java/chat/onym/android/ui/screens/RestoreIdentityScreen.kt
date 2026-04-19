package chat.onym.android.ui.screens

import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Cancel
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.ContentPaste
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import chat.onym.android.crypto.Bip39

@OptIn(ExperimentalMaterial3Api::class, ExperimentalComposeUiApi::class)
@Composable
fun RestoreIdentityScreen(
    onRestore: (mnemonic: String, onResult: (Boolean) -> Unit) -> Unit,
    onBack: () -> Unit
) {
    val context = LocalContext.current
    val words: SnapshotStateList<String> = remember { mutableStateListOf<String>().apply { repeat(12) { add("") } } }
    var activeSlot by remember { mutableStateOf(0) }
    var currentInput by remember { mutableStateOf("") }
    var errorMessage by remember { mutableStateOf<String?>(null) }
    var isRestoring by remember { mutableStateOf(false) }
    var restored by remember { mutableStateOf(false) }
    var showConfirmDialog by remember { mutableStateOf(false) }

    val focusRequester = remember { FocusRequester() }
    val keyboardController = LocalSoftwareKeyboardController.current

    val suggestions by remember {
        derivedStateOf { Bip39.suggestions(prefix = currentInput, limit = 4) }
    }
    val allFilled by remember { derivedStateOf { words.all { it.isNotBlank() } } }

    fun advanceFocus() {
        val nextEmpty = ((activeSlot + 1) until words.size).firstOrNull { words[it].isEmpty() }
        activeSlot = when {
            nextEmpty != null -> nextEmpty
            else -> words.indexOfFirst { it.isEmpty() }.takeIf { it >= 0 } ?: (words.size - 1)
        }
        errorMessage = null
    }

    fun acceptSuggestion(word: String) {
        if (activeSlot in words.indices) {
            words[activeSlot] = word
            currentInput = ""
            advanceFocus()
        }
    }

    fun commitTopSuggestion() {
        val trimmed = currentInput.trim().lowercase()
        if (trimmed.isEmpty()) return
        when {
            Bip39.isKnownWord(trimmed) -> acceptSuggestion(trimmed)
            suggestions.isNotEmpty() -> acceptSuggestion(suggestions.first())
            else -> errorMessage = "\"$trimmed\" is not in the BIP39 wordlist."
        }
    }

    fun applyPaste(text: String) {
        val tokens = text.trim().lowercase().split("\\s+".toRegex()).filter { it.isNotBlank() }
        if (tokens.size != words.size) {
            errorMessage = "Expected ${words.size} words, got ${tokens.size}."
            return
        }
        for (i in words.indices) words[i] = tokens[i]
        currentInput = ""
        errorMessage = null
        activeSlot = words.size - 1
        keyboardController?.hide()
    }

    LaunchedEffect(Unit) {
        focusRequester.requestFocus()
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Restore") },
                navigationIcon = {
                    TextButton(onClick = onBack, enabled = !isRestoring) { Text("Cancel") }
                },
                actions = {
                    if (restored) {
                        TextButton(onClick = onBack) { Text("Done", fontWeight = FontWeight.SemiBold) }
                    }
                }
            )
        },
        bottomBar = {
            InputBar(
                value = currentInput,
                onValueChange = { new ->
                    if (new.endsWith(" ")) {
                        currentInput = new.trimEnd()
                        commitTopSuggestion()
                    } else {
                        currentInput = new
                    }
                },
                placeholder = "Word ${activeSlot + 1}",
                focusRequester = focusRequester,
                onSubmit = { commitTopSuggestion() },
                onClear = { currentInput = "" },
                enabled = !isRestoring && !restored
            )
        },
        containerColor = MaterialTheme.colorScheme.surfaceContainerLowest
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .verticalScroll(rememberScrollState())
        ) {
            Text(
                "Type the 12 words from your recovery phrase. We'll suggest matches as you go.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 32.dp, vertical = 10.dp)
            )

            SlotGrid(
                words = words,
                activeSlot = activeSlot,
                currentInput = currentInput,
                onSlotClick = { i ->
                    activeSlot = i
                    currentInput = ""
                    focusRequester.requestFocus()
                },
                modifier = Modifier.padding(horizontal = 16.dp)
            )

            if (suggestions.isNotEmpty()) {
                SuggestionChips(
                    suggestions = suggestions,
                    onTap = { acceptSuggestion(it) },
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 10.dp)
                )
            }

            errorMessage?.let {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 32.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Icon(
                        Icons.Default.Warning,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.error,
                        modifier = Modifier.size(16.dp)
                    )
                    Spacer(Modifier.width(8.dp))
                    Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
                }
            }

            if (restored) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 32.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Box(
                        modifier = Modifier
                            .size(18.dp)
                            .background(Color(0xFF34C759), CircleShape),
                        contentAlignment = Alignment.Center
                    ) {
                        Icon(Icons.Default.Check, contentDescription = null, tint = Color.White, modifier = Modifier.size(12.dp))
                    }
                    Spacer(Modifier.width(8.dp))
                    Text(
                        "Identity restored successfully",
                        style = MaterialTheme.typography.bodySmall,
                        color = Color(0xFF34C759)
                    )
                }
            }

            Button(
                onClick = {
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    val text = clipboard.primaryClip?.getItemAt(0)?.text?.toString()
                    if (text != null) applyPaste(text)
                },
                enabled = !isRestoring && !restored,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp)
                    .padding(top = 18.dp)
                    .height(46.dp),
                shape = RoundedCornerShape(14.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.primary.copy(alpha = 0.12f),
                    contentColor = MaterialTheme.colorScheme.primary
                )
            ) {
                Icon(Icons.Default.ContentPaste, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
                Text("Paste phrase", fontWeight = FontWeight.SemiBold)
            }

            Button(
                onClick = { showConfirmDialog = true },
                enabled = allFilled && !isRestoring && !restored,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp)
                    .padding(top = 10.dp, bottom = 40.dp)
                    .height(52.dp),
                shape = RoundedCornerShape(14.dp)
            ) {
                if (isRestoring) {
                    CircularProgressIndicator(
                        color = Color.White,
                        strokeWidth = 2.dp,
                        modifier = Modifier.size(20.dp)
                    )
                } else {
                    Text("Restore Identity", fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }

    if (showConfirmDialog) {
        AlertDialog(
            onDismissRequest = { showConfirmDialog = false },
            title = { Text("Replace Current Identity?") },
            text = { Text("This will replace your current identity keys. If your current identity is not backed up, it will be permanently lost.") },
            confirmButton = {
                TextButton(onClick = {
                    showConfirmDialog = false
                    errorMessage = null
                    isRestoring = true
                    val mnemonic = words.joinToString(" ") { it.lowercase() }
                    onRestore(mnemonic) { success ->
                        if (success) {
                            restored = true
                            isRestoring = false
                            Toast.makeText(context, "Identity restored", Toast.LENGTH_SHORT).show()
                        } else {
                            errorMessage = "Invalid recovery phrase. Check your words and try again."
                            isRestoring = false
                        }
                    }
                }) {
                    Text("Replace", fontWeight = FontWeight.Bold, color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { showConfirmDialog = false }) { Text("Cancel") }
            }
        )
    }
}

@Composable
private fun SlotGrid(
    words: List<String>,
    activeSlot: Int,
    currentInput: String,
    onSlotClick: (Int) -> Unit,
    modifier: Modifier = Modifier
) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surfaceContainer, RoundedCornerShape(16.dp))
            .padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp)
    ) {
        for (rowIdx in 0 until 6) {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                for (colIdx in 0..1) {
                    val i = rowIdx * 2 + colIdx
                    SlotChip(
                        index = i,
                        value = words[i],
                        active = activeSlot == i,
                        currentInput = if (activeSlot == i) currentInput else "",
                        onClick = { onSlotClick(i) },
                        modifier = Modifier.weight(1f)
                    )
                }
            }
        }
    }
}

@Composable
private fun SlotChip(
    index: Int,
    value: String,
    active: Boolean,
    currentInput: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    val filled = value.isNotEmpty()
    val borderColor = if (active) MaterialTheme.colorScheme.primary else Color.Transparent
    val bgColor = if (active) MaterialTheme.colorScheme.primary.copy(alpha = 0.12f)
    else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.04f)

    Row(
        modifier = modifier
            .background(bgColor, RoundedCornerShape(8.dp))
            .border(1.2.dp, borderColor, RoundedCornerShape(8.dp))
            .clickable { onClick() }
            .padding(horizontal = 10.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(
            "${index + 1}",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.4f),
            fontFamily = FontFamily.Monospace,
            modifier = Modifier.width(18.dp)
        )
        Spacer(Modifier.width(8.dp))
        when {
            active && !filled -> {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        currentInput,
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = FontWeight.Medium
                    )
                    Box(
                        modifier = Modifier
                            .padding(start = 1.dp)
                            .width(1.5.dp)
                            .height(14.dp)
                            .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.9f))
                    )
                }
            }
            filled -> Text(
                value,
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.Medium
            )
            else -> Text("", style = MaterialTheme.typography.bodyMedium)
        }
    }
}

@Composable
private fun SuggestionChips(
    suggestions: List<String>,
    onTap: (String) -> Unit,
    modifier: Modifier = Modifier
) {
    Row(
        modifier = modifier.horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(6.dp)
    ) {
        for ((idx, word) in suggestions.withIndex()) {
            val isPrimary = idx == 0
            val containerColor = if (isPrimary) MaterialTheme.colorScheme.primary
            else MaterialTheme.colorScheme.surfaceContainerHigh
            val textColor = if (isPrimary) Color.White else MaterialTheme.colorScheme.onSurface
            Box(
                modifier = Modifier
                    .background(containerColor, RoundedCornerShape(50))
                    .clickable { onTap(word) }
                    .padding(horizontal = 12.dp, vertical = 7.dp)
            ) {
                Text(word, color = textColor, fontWeight = FontWeight.Medium)
            }
        }
    }
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
private fun InputBar(
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String,
    focusRequester: FocusRequester,
    onSubmit: () -> Unit,
    onClear: () -> Unit,
    enabled: Boolean
) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        tonalElevation = 3.dp,
        modifier = Modifier.fillMaxWidth()
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .imePadding()
                .navigationBarsPadding()
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Box(modifier = Modifier.weight(1f)) {
                BasicTextField(
                    value = value,
                    onValueChange = onValueChange,
                    textStyle = LocalTextStyle.current.merge(
                        TextStyle(
                            fontFamily = FontFamily.Monospace,
                            color = MaterialTheme.colorScheme.onSurface
                        )
                    ),
                    cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                    singleLine = true,
                    enabled = enabled,
                    keyboardOptions = KeyboardOptions(
                        autoCorrect = false,
                        capitalization = KeyboardCapitalization.None,
                        imeAction = ImeAction.Next
                    ),
                    keyboardActions = KeyboardActions(
                        onNext = { onSubmit() },
                        onDone = { onSubmit() }
                    ),
                    modifier = Modifier
                        .fillMaxWidth()
                        .focusRequester(focusRequester)
                )
                if (value.isEmpty()) {
                    Text(
                        placeholder,
                        style = LocalTextStyle.current.merge(
                            TextStyle(
                                fontFamily = FontFamily.Monospace,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        )
                    )
                }
            }
            if (value.isNotEmpty()) {
                Icon(
                    Icons.Default.Cancel,
                    contentDescription = "Clear",
                    tint = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.4f),
                    modifier = Modifier
                        .size(20.dp)
                        .clickable { onClear() }
                )
            }
        }
    }
}
