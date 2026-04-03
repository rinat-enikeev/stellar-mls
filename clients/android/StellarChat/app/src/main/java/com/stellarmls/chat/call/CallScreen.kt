package com.stellarmls.chat.call

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CallEnd
import androidx.compose.material.icons.filled.Cameraswitch
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.MicOff
import androidx.compose.material.icons.filled.Phone
import androidx.compose.material.icons.filled.Videocam
import androidx.compose.material.icons.filled.VideocamOff
import androidx.compose.material.icons.filled.VolumeUp
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import org.webrtc.RendererCommon
import org.webrtc.SurfaceViewRenderer
import org.webrtc.VideoTrack

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
        // Remote video (fullscreen background)
        if (callManager.isVideoCall && callManager.remoteVideoTrack != null) {
            WebRTCVideoView(
                track = callManager.remoteVideoTrack!!,
                eglContext = callManager.eglBaseContext,
                modifier = Modifier.fillMaxSize()
            )
        }

        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.SpaceBetween,
            modifier = Modifier
                .fillMaxSize()
                .padding(32.dp)
        ) {
            // Local video PiP (top-right)
            if (callManager.isVideoCall && callManager.localVideoTrack != null && callManager.isVideoEnabled) {
                Box(modifier = Modifier.fillMaxWidth()) {
                    WebRTCVideoView(
                        track = callManager.localVideoTrack!!,
                        eglContext = callManager.eglBaseContext,
                        mirror = callManager.isUsingFrontCamera,
                        modifier = Modifier
                            .size(120.dp, 160.dp)
                            .clip(RoundedCornerShape(12.dp))
                            .align(Alignment.TopEnd)
                    )
                }
            }

            Spacer(modifier = Modifier.weight(1f))

            // Name & status (hide when remote video is showing)
            if (!callManager.isVideoCall || callManager.remoteVideoTrack == null) {
                Text(
                    text = remoteName,
                    style = MaterialTheme.typography.headlineMedium,
                    color = Color.White
                )

                Spacer(modifier = Modifier.height(8.dp))

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
            }

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
                    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
                        // Video controls row (only for video calls)
                        if (callManager.isVideoCall) {
                            Row(horizontalArrangement = Arrangement.spacedBy(40.dp)) {
                                IconButton(
                                    onClick = { callManager.toggleVideo() },
                                    modifier = Modifier
                                        .size(56.dp)
                                        .background(
                                            if (callManager.isVideoEnabled) Color.Gray.copy(alpha = 0.5f) else Color.Red,
                                            CircleShape
                                        )
                                ) {
                                    Icon(
                                        if (callManager.isVideoEnabled) Icons.Default.Videocam else Icons.Default.VideocamOff,
                                        contentDescription = "Video",
                                        tint = Color.White
                                    )
                                }
                                IconButton(
                                    onClick = { callManager.flipCamera() },
                                    modifier = Modifier
                                        .size(56.dp)
                                        .background(Color.Gray.copy(alpha = 0.5f), CircleShape)
                                ) {
                                    Icon(Icons.Default.Cameraswitch, contentDescription = "Flip", tint = Color.White)
                                }
                            }
                        }
                        // Audio controls row
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
                }
                else -> {}
            }

            Spacer(modifier = Modifier.height(40.dp))
        }
    }
}

@Composable
fun WebRTCVideoView(
    track: VideoTrack,
    eglContext: org.webrtc.EglBase.Context?,
    mirror: Boolean = false,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val renderer = remember {
        SurfaceViewRenderer(context).apply {
            init(eglContext, null)
            setScalingType(RendererCommon.ScalingType.SCALE_ASPECT_FILL)
            setMirror(mirror)
        }
    }

    DisposableEffect(track) {
        track.addSink(renderer)
        onDispose {
            try { track.removeSink(renderer) } catch (_: Exception) {}
            try { renderer.release() } catch (_: Exception) {}
        }
    }

    AndroidView(
        factory = { renderer },
        modifier = modifier,
        update = { it.setMirror(mirror) }
    )
}
