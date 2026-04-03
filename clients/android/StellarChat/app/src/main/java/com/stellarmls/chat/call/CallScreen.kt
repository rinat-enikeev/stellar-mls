package com.stellarmls.chat.call

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CallEnd
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.Phone
import androidx.compose.material.icons.filled.VolumeUp
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp

@Composable
fun CallScreen(
    callManager: CallManager,
    remoteName: String,
    onDismiss: () -> Unit
) {
    if (callManager.state == CallState.IDLE) {
        onDismiss()
        return
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color.Black),
        contentAlignment = Alignment.Center
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.SpaceBetween,
            modifier = Modifier
                .fillMaxSize()
                .padding(32.dp)
        ) {
            Spacer(modifier = Modifier.weight(1f))

            // Remote name
            Text(
                text = remoteName,
                style = MaterialTheme.typography.headlineMedium,
                color = Color.White
            )

            Spacer(modifier = Modifier.height(8.dp))

            // Status
            Text(
                text = when (callManager.state) {
                    CallState.RINGING -> if (callManager.direction == CallDirection.OUTGOING) "Calling..." else "Incoming call"
                    CallState.ACTIVE -> "Connected"
                    CallState.ENDED -> "Call ended"
                    else -> ""
                },
                style = MaterialTheme.typography.bodyLarge,
                color = Color.Gray
            )

            // Duration
            if (callManager.state == CallState.ACTIVE) {
                Spacer(modifier = Modifier.height(16.dp))
                val mins = callManager.callDuration / 60
                val secs = callManager.callDuration % 60
                Text(
                    text = "%d:%02d".format(mins, secs),
                    style = MaterialTheme.typography.headlineSmall,
                    color = Color.White,
                    fontFamily = FontFamily.Monospace
                )
            }

            Spacer(modifier = Modifier.weight(1f))

            // Controls
            when {
                callManager.state == CallState.RINGING && callManager.direction == CallDirection.INCOMING -> {
                    // Incoming: decline + accept
                    Row(horizontalArrangement = Arrangement.spacedBy(60.dp)) {
                        IconButton(
                            onClick = { callManager.rejectCall() },
                            modifier = Modifier
                                .size(64.dp)
                                .background(Color.Red, CircleShape)
                        ) {
                            Icon(Icons.Default.CallEnd, contentDescription = "Decline", tint = Color.White)
                        }
                        IconButton(
                            onClick = { callManager.acceptCall() },
                            modifier = Modifier
                                .size(64.dp)
                                .background(Color(0xFF4CAF50), CircleShape)
                        ) {
                            Icon(Icons.Default.Phone, contentDescription = "Accept", tint = Color.White)
                        }
                    }
                }
                callManager.state == CallState.RINGING && callManager.direction == CallDirection.OUTGOING -> {
                    // Outgoing: cancel
                    IconButton(
                        onClick = { callManager.hangup() },
                        modifier = Modifier
                            .size(64.dp)
                            .background(Color.Red, CircleShape)
                    ) {
                        Icon(Icons.Default.CallEnd, contentDescription = "Cancel", tint = Color.White)
                    }
                }
                callManager.state == CallState.ACTIVE -> {
                    // Active: mute, speaker, hangup
                    Row(horizontalArrangement = Arrangement.spacedBy(40.dp)) {
                        IconButton(
                            onClick = { callManager.toggleMute() },
                            modifier = Modifier
                                .size(56.dp)
                                .background(
                                    if (callManager.isMuted) Color.Red else Color.Gray.copy(alpha = 0.5f),
                                    CircleShape
                                )
                        ) {
                            Icon(
                                if (callManager.isMuted) Icons.Default.MicOff else Icons.Default.Mic,
                                contentDescription = "Mute",
                                tint = Color.White
                            )
                        }
                        IconButton(
                            onClick = { callManager.toggleSpeaker() },
                            modifier = Modifier
                                .size(56.dp)
                                .background(
                                    if (callManager.isSpeaker) Color.Blue else Color.Gray.copy(alpha = 0.5f),
                                    CircleShape
                                )
                        ) {
                            Icon(Icons.Default.VolumeUp, contentDescription = "Speaker", tint = Color.White)
                        }
                        IconButton(
                            onClick = { callManager.hangup() },
                            modifier = Modifier
                                .size(56.dp)
                                .background(Color.Red, CircleShape)
                        ) {
                            Icon(Icons.Default.CallEnd, contentDescription = "End", tint = Color.White)
                        }
                    }
                }
                else -> {}
            }

            Spacer(modifier = Modifier.height(40.dp))
        }
    }
}
