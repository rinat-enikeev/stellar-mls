package com.stellarmls.chat.ui.components

import android.Manifest
import android.media.MediaRecorder
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGesturesAfterLongPress
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.outlined.Mic
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import java.io.File
import java.util.UUID

@Composable
fun VoiceRecordButton(
    hasBlossomServers: Boolean,
    onSend: (File) -> Unit,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val density = LocalDensity.current
    var isRecording by remember { mutableStateOf(false) }
    var isCancelled by remember { mutableStateOf(false) }
    var durationMs by remember { mutableLongStateOf(0L) }
    var totalDragX by remember { mutableFloatStateOf(0f) }
    val recorder = remember { mutableStateOf<MediaRecorder?>(null) }
    val outputFile = remember { mutableStateOf<File?>(null) }
    var hasPermission by remember { mutableStateOf(false) }
    var pendingRecord by remember { mutableStateOf(false) }

    val cancelThresholdPx = with(density) { 60.dp.toPx() }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted ->
        hasPermission = granted
        if (granted && pendingRecord) {
            pendingRecord = false
            startRecording(context, recorder, outputFile)
            isRecording = true
            durationMs = 0
            totalDragX = 0f
            isCancelled = false
        }
    }

    // Check permission on compose
    LaunchedEffect(Unit) {
        hasPermission = context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) ==
            android.content.pm.PackageManager.PERMISSION_GRANTED
    }

    // Duration timer
    LaunchedEffect(isRecording) {
        if (isRecording) {
            val startTime = System.currentTimeMillis()
            while (isRecording) {
                durationMs = System.currentTimeMillis() - startTime
                delay(100)
            }
        }
    }

    DisposableEffect(Unit) {
        onDispose {
            recorder.value?.let {
                try { it.stop() } catch (_: Exception) { }
                it.release()
            }
            recorder.value = null
        }
    }

    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp)
    ) {
        if (isRecording) {
            Box(
                modifier = Modifier
                    .size(8.dp)
                    .background(Color.Red, CircleShape)
            )
            Text(
                text = formatRecordingDuration(durationMs),
                style = MaterialTheme.typography.labelSmall,
                color = Color.Red
            )
            Text(
                text = if (isCancelled) "Release to cancel" else "Slide to cancel",
                style = MaterialTheme.typography.labelSmall,
                color = if (isCancelled) Color.Red
                    else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f)
            )
        }

        Icon(
            imageVector = if (isRecording) Icons.Filled.Mic else Icons.Outlined.Mic,
            contentDescription = "Record voice message",
            tint = when {
                !hasBlossomServers -> MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f)
                isRecording -> Color.Red
                else -> MaterialTheme.colorScheme.primary
            },
            modifier = modifier
                .size(24.dp)
                .pointerInput(hasBlossomServers) {
                    if (!hasBlossomServers) return@pointerInput
                    detectDragGesturesAfterLongPress(
                        onDragStart = {
                            if (!hasPermission) {
                                pendingRecord = true
                                permissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
                                return@detectDragGesturesAfterLongPress
                            }
                            startRecording(context, recorder, outputFile)
                            isRecording = true
                            durationMs = 0
                            totalDragX = 0f
                            isCancelled = false
                        },
                        onDragEnd = {
                            if (!isRecording) return@detectDragGesturesAfterLongPress
                            isRecording = false
                            recorder.value?.let {
                                try { it.stop() } catch (_: Exception) { }
                                it.release()
                            }
                            recorder.value = null

                            if (isCancelled || durationMs < 1000) {
                                outputFile.value?.delete()
                            } else {
                                outputFile.value?.let { onSend(it) }
                            }
                            outputFile.value = null
                            isCancelled = false
                        },
                        onDragCancel = {
                            isRecording = false
                            recorder.value?.let {
                                try { it.stop() } catch (_: Exception) { }
                                it.release()
                            }
                            recorder.value = null
                            outputFile.value?.delete()
                            outputFile.value = null
                            isCancelled = false
                        },
                        onDrag = { _, dragAmount ->
                            totalDragX += dragAmount.x
                            isCancelled = totalDragX < -cancelThresholdPx
                        }
                    )
                }
        )
    }
}

private fun startRecording(
    context: android.content.Context,
    recorderState: androidx.compose.runtime.MutableState<MediaRecorder?>,
    fileState: androidx.compose.runtime.MutableState<File?>
) {
    val file = File(context.cacheDir, "${UUID.randomUUID()}.m4a")
    @Suppress("DEPRECATION")
    val rec = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        MediaRecorder(context)
    } else {
        MediaRecorder()
    }
    rec.setAudioSource(MediaRecorder.AudioSource.MIC)
    rec.setOutputFormat(MediaRecorder.OutputFormat.MPEG_4)
    rec.setAudioEncoder(MediaRecorder.AudioEncoder.AAC)
    rec.setAudioEncodingBitRate(64000)
    rec.setAudioSamplingRate(44100)
    rec.setOutputFile(file.absolutePath)
    rec.prepare()
    rec.start()
    recorderState.value = rec
    fileState.value = file
}

private fun formatRecordingDuration(ms: Long): String {
    val totalSecs = (ms / 1000).toInt()
    return String.format("%d:%02d", totalSecs / 60, totalSecs % 60)
}
