package chat.onym.android.call

import android.content.Context
import android.media.AudioManager
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.*
import org.json.JSONObject
import org.webrtc.*
import java.security.SecureRandom
import org.webrtc.Camera2Enumerator

enum class CallState { IDLE, RINGING, ACTIVE, ENDED }
enum class CallDirection { OUTGOING, INCOMING }
enum class CallEndReason { NONE, HANGUP, REJECTED, BUSY, TIMEOUT, ICE_FAILED, ANSWERED_ELSEWHERE }
enum class ICEStatus { NEW, CHECKING, CONNECTED, COMPLETED, FAILED, DISCONNECTED, CLOSED }

class CallManager(private val context: Context) {
    var state by mutableStateOf(CallState.IDLE)
    var direction by mutableStateOf(CallDirection.OUTGOING)
    var callId by mutableStateOf("")
    var remoteBlsPubkey: ByteArray? = null
    var isMuted by mutableStateOf(false)
    var isSpeaker by mutableStateOf(false)
    var isVideoEnabled by mutableStateOf(false)
    var isUsingFrontCamera by mutableStateOf(true)
    var isVideoCall by mutableStateOf(false)
    var callDuration by mutableLongStateOf(0L)
    var iceStatus by mutableStateOf(ICEStatus.NEW)
    var callEndReason by mutableStateOf(CallEndReason.NONE)
    /** Tracks whether the first answer has been received (first-answer-wins). */
    private var answerReceived = false

    /** Remote video track for rendering. */
    var remoteVideoTrack by mutableStateOf<VideoTrack?>(null)
    /** Local video track for PiP rendering. */
    var localVideoTrack by mutableStateOf<VideoTrack?>(null)

    /** Set by ViewModel — sends signaling JSON over the group channel. */
    var sendSignal: ((JSONObject) -> Unit)? = null

    private var peerConnection: PeerConnection? = null
    private var localAudioTrack: AudioTrack? = null
    private var capturer: CameraVideoCapturer? = null
    private var surfaceTextureHelper: SurfaceTextureHelper? = null
    private var factory: PeerConnectionFactory? = null
    private val scope = CoroutineScope(Dispatchers.Main + SupervisorJob())
    private var durationJob: Job? = null
    private var ringJob: Job? = null
    /** ICE candidates received before remote description is set. */
    private val pendingCandidates = mutableListOf<IceCandidate>()

    /** Custom TURN server configuration. Set from ViewModel before calls. */
    var turnURLs: List<String> = emptyList()
    var turnUsername: String = ""
    var turnPassword: String = ""
    var turnEnabled: Boolean = false

    private val iceServers: List<PeerConnection.IceServer>
        get() {
            val servers = mutableListOf(
                PeerConnection.IceServer.builder("stun:stun.l.google.com:19302").createIceServer(),
                PeerConnection.IceServer.builder("stun:stun1.l.google.com:19302").createIceServer(),
                // Built-in EU TURN (Metered) — TCP on 443 for firewall compatibility
                PeerConnection.IceServer.builder("turn:eu-turn.metered.ca:443?transport=tcp")
                    .setUsername("stellarchat")
                    .setPassword("stellarchat-turn-2026")
                    .createIceServer(),
                PeerConnection.IceServer.builder("turns:eu-turn.metered.ca:443?transport=tcp")
                    .setUsername("stellarchat")
                    .setPassword("stellarchat-turn-2026")
                    .createIceServer(),
            )
            // User-configured TURN servers
            if (turnEnabled && turnURLs.isNotEmpty() && turnUsername.isNotEmpty()) {
                servers.add(
                    PeerConnection.IceServer.builder(turnURLs)
                        .setUsername(turnUsername)
                        .setPassword(turnPassword)
                        .createIceServer()
                )
            }
            return servers
        }

    companion object {
        private const val TAG = "CallManager"

        fun initialize(context: Context) {
            PeerConnectionFactory.initialize(
                PeerConnectionFactory.InitializationOptions.builder(context)
                    .setEnableInternalTracer(false)
                    .createInitializationOptions()
            )
        }
    }

    // MARK: - Start Call (Outgoing)

    fun startCall(video: Boolean = false) {
        if (state != CallState.IDLE) return
        callId = generateCallId()
        direction = CallDirection.OUTGOING
        isVideoCall = video
        isVideoEnabled = video
        callEndReason = CallEndReason.NONE
        iceStatus = ICEStatus.NEW
        state = CallState.RINGING

        setupPeerConnection()
        addAudioTrack()
        if (video) addVideoTrack()

        peerConnection?.createOffer(object : SdpObserverAdapter() {
            override fun onCreateSuccess(sdp: SessionDescription) {
                peerConnection?.setLocalDescription(SdpObserverAdapter(), sdp)
                val signal = JSONObject().apply {
                    put("action", "offer")
                    put("callId", callId)
                    put("mediaType", if (video) "video" else "audio")
                    put("sdp", sdp.description)
                }
                scope.launch { sendSignal?.invoke(signal) }
            }
        }, mediaConstraints(video))

        startRingTimer()
        CallConnectionService.placeOutgoingCall(context, "Call", video)
    }

    // MARK: - Handle Incoming Signaling

    fun handleSignal(callJson: JSONObject, senderPubkey: ByteArray) {
        val action = callJson.optString("action")
        val incomingCallId = callJson.optString("callId")

        when (action) {
            "offer" -> {
                if (state != CallState.IDLE) {
                    val busy = JSONObject().apply {
                        put("action", "busy")
                        put("callId", incomingCallId)
                    }
                    sendSignal?.invoke(busy)
                    return
                }
                callId = incomingCallId
                direction = CallDirection.INCOMING
                remoteBlsPubkey = senderPubkey
                isVideoCall = callJson.optString("mediaType") == "video"
                isVideoEnabled = isVideoCall
                state = CallState.RINGING

                setupPeerConnection()
                addAudioTrack()
                if (isVideoCall) addVideoTrack()

                val sdpStr = callJson.optString("sdp")
                if (sdpStr.isNotEmpty()) {
                    peerConnection?.setRemoteDescription(
                        SdpObserverAdapter(),
                        SessionDescription(SessionDescription.Type.OFFER, sdpStr)
                    )
                    drainPendingCandidates()
                }
                CallConnectionService.reportIncomingCall(context, "Incoming Call", isVideoCall)
                startRingTimer()
            }
            "answer" -> {
                if (incomingCallId != callId || direction != CallDirection.OUTGOING) return
                // First-answer-wins: ignore duplicate/late answers.
                // Duplicates arrive from multiple relays for the same answer.
                // Late answerers will time out via the ring timer on their end.
                if (answerReceived) return
                if (state != CallState.RINGING) return
                answerReceived = true
                ringJob?.cancel()
                val sdpStr = callJson.optString("sdp")
                if (sdpStr.isNotEmpty()) {
                    peerConnection?.setRemoteDescription(
                        SdpObserverAdapter(),
                        SessionDescription(SessionDescription.Type.ANSWER, sdpStr)
                    )
                    drainPendingCandidates()
                }
                state = CallState.ACTIVE
                startDurationTimer()
                configureAudioSession(isVideoCall)
            }
            "ice" -> {
                if (incomingCallId != callId) return
                val candidateStr = callJson.optString("candidate")
                val sdpMid = callJson.optString("sdpMid")
                val sdpMLineIndex = callJson.optInt("sdpMLineIndex", 0)
                if (candidateStr.isNotEmpty()) {
                    val candidate = IceCandidate(sdpMid, sdpMLineIndex, candidateStr)
                    if (peerConnection?.remoteDescription != null) {
                        peerConnection?.addIceCandidate(candidate)
                    } else {
                        pendingCandidates.add(candidate)
                    }
                }
            }
            "hangup" -> {
                if (incomingCallId != callId) return
                val reason = callJson.optString("reason")
                // "answered" hangups are meant for late answerers still ringing,
                // not for connected parties. Ignore if already active.
                if (reason == "answered" && state == CallState.ACTIVE) return
                callEndReason = if (reason == "answered") CallEndReason.ANSWERED_ELSEWHERE else CallEndReason.HANGUP
                endCall(sendHangup = false)
            }
            "busy", "reject" -> {
                // In a multi-member group, individual busy/reject responses should
                // not kill the entire call — other members may still answer.
                // The ring timer (30s) handles the case where nobody answers.
            }
        }
    }

    // MARK: - Accept Call (Incoming)

    fun acceptCall() {
        if (state != CallState.RINGING || direction != CallDirection.INCOMING) return
        ringJob?.cancel()

        peerConnection?.createAnswer(object : SdpObserverAdapter() {
            override fun onCreateSuccess(sdp: SessionDescription) {
                peerConnection?.setLocalDescription(SdpObserverAdapter(), sdp)
                val signal = JSONObject().apply {
                    put("action", "answer")
                    put("callId", callId)
                    put("sdp", sdp.description)
                }
                scope.launch { sendSignal?.invoke(signal) }
            }
        }, mediaConstraints(isVideoCall))

        state = CallState.ACTIVE
        startDurationTimer()
        configureAudioSession(isVideoCall)
    }

    // MARK: - Reject / End

    fun rejectCall() {
        if (state != CallState.RINGING || direction != CallDirection.INCOMING) return
        val signal = JSONObject().apply {
            put("action", "reject")
            put("callId", callId)
        }
        sendSignal?.invoke(signal)
        callEndReason = CallEndReason.REJECTED
        endCall(sendHangup = false)
    }

    fun hangup() {
        callEndReason = CallEndReason.HANGUP
        endCall(sendHangup = true)
    }

    fun endCall(sendHangup: Boolean) {
        ringJob?.cancel()
        durationJob?.cancel()

        if (sendHangup) {
            val signal = JSONObject().apply {
                put("action", "hangup")
                put("callId", callId)
            }
            sendSignal?.invoke(signal)
        }

        // Null out video tracks first so Compose drops the renderers.
        localVideoTrack = null
        remoteVideoTrack = null
        localAudioTrack = null

        capturer?.stopCapture()
        capturer?.dispose()
        capturer = null
        surfaceTextureHelper?.dispose()
        surfaceTextureHelper = null

        // Defer native resource disposal — Compose needs a frame to
        // recompose, remove the video views, and run onDispose before
        // the underlying EGL/factory objects are destroyed.
        val pc = peerConnection
        val f = factory
        val egl = eglBase
        peerConnection = null
        factory = null
        eglBase = null
        scope.launch {
            delay(300)
            pc?.close()
            f?.dispose()
            egl?.release()
        }
        state = CallState.ENDED
        callDuration = 0
        isVideoCall = false
        isVideoEnabled = false
        answerReceived = false
        pendingCandidates.clear()

        if (chat.onym.android.BuildConfig.DEBUG) {
            Log.d(TAG, "Call ended: reason=${callEndReason.name}")
        }

        CallConnectionService.reportCallEnded()

        val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        audioManager.mode = AudioManager.MODE_NORMAL

        scope.launch {
            delay(1000)
            if (state == CallState.ENDED) {
                state = CallState.IDLE
                remoteBlsPubkey = null
                iceStatus = ICEStatus.NEW
                callEndReason = CallEndReason.NONE
            }
        }
    }

    // MARK: - Mute / Speaker

    fun toggleMute() {
        isMuted = !isMuted
        localAudioTrack?.setEnabled(!isMuted)
    }

    fun toggleSpeaker() {
        isSpeaker = !isSpeaker
        configureAudioSession(isSpeaker)
    }

    fun toggleVideo() {
        isVideoEnabled = !isVideoEnabled
        localVideoTrack?.setEnabled(isVideoEnabled)
        if (isVideoEnabled) {
            startCapture()
        } else {
            capturer?.stopCapture()
        }
    }

    fun flipCamera() {
        isUsingFrontCamera = !isUsingFrontCamera
        startCapture()
    }

    // MARK: - Private

    private var eglBase: EglBase? = null

    /** Expose EGL context for SurfaceViewRenderers. */
    val eglBaseContext: EglBase.Context?
        get() = eglBase?.eglBaseContext

    private fun drainPendingCandidates() {
        if (pendingCandidates.isEmpty()) return
        val candidates = pendingCandidates.toList()
        pendingCandidates.clear()
        for (candidate in candidates) {
            peerConnection?.addIceCandidate(candidate)
        }
    }

    private fun setupPeerConnection() {
        val egl = EglBase.create()
        this.eglBase = egl

        val factory = PeerConnectionFactory.builder()
            .setVideoDecoderFactory(DefaultVideoDecoderFactory(egl.eglBaseContext))
            .setVideoEncoderFactory(DefaultVideoEncoderFactory(egl.eglBaseContext, true, true))
            .createPeerConnectionFactory()
        this.factory = factory

        val servers = iceServers
        if (chat.onym.android.BuildConfig.DEBUG) {
            Log.d(TAG, "ICE servers: ${servers.map { it.urls }}")
        }
        val config = PeerConnection.RTCConfiguration(servers).apply {
            sdpSemantics = PeerConnection.SdpSemantics.UNIFIED_PLAN
            bundlePolicy = PeerConnection.BundlePolicy.MAXBUNDLE
            rtcpMuxPolicy = PeerConnection.RtcpMuxPolicy.REQUIRE
        }

        peerConnection = factory.createPeerConnection(config, object : PeerConnection.Observer {
            override fun onSignalingChange(state: PeerConnection.SignalingState?) {}
            override fun onIceConnectionReceivingChange(receiving: Boolean) {}
            override fun onIceGatheringChange(state: PeerConnection.IceGatheringState?) {}
            override fun onAddStream(stream: MediaStream?) {}
            override fun onRemoveStream(stream: MediaStream?) {}
            override fun onDataChannel(channel: DataChannel?) {}
            override fun onRenegotiationNeeded() {}
            override fun onAddTrack(receiver: RtpReceiver?, streams: Array<out MediaStream>?) {
                val track = receiver?.track()
                if (track is VideoTrack) {
                    scope.launch { remoteVideoTrack = track }
                }
            }

            override fun onIceCandidate(candidate: IceCandidate) {
                if (chat.onym.android.BuildConfig.DEBUG) {
                    val candidateType = when {
                        candidate.sdp.contains("typ relay") -> "relay"
                        candidate.sdp.contains("typ srflx") -> "srflx"
                        candidate.sdp.contains("typ host") -> "host"
                        else -> "unknown"
                    }
                    Log.d(TAG, "ICE candidate: $candidateType")
                }
                val signal = JSONObject().apply {
                    put("action", "ice")
                    put("callId", callId)
                    put("candidate", candidate.sdp)
                    put("sdpMid", candidate.sdpMid)
                    put("sdpMLineIndex", candidate.sdpMLineIndex)
                }
                scope.launch { sendSignal?.invoke(signal) }
            }

            override fun onIceCandidatesRemoved(candidates: Array<out IceCandidate>?) {}

            override fun onIceConnectionChange(newState: PeerConnection.IceConnectionState?) {
                val status = when (newState) {
                    PeerConnection.IceConnectionState.NEW -> ICEStatus.NEW
                    PeerConnection.IceConnectionState.CHECKING -> ICEStatus.CHECKING
                    PeerConnection.IceConnectionState.CONNECTED -> ICEStatus.CONNECTED
                    PeerConnection.IceConnectionState.COMPLETED -> ICEStatus.COMPLETED
                    PeerConnection.IceConnectionState.FAILED -> ICEStatus.FAILED
                    PeerConnection.IceConnectionState.DISCONNECTED -> ICEStatus.DISCONNECTED
                    PeerConnection.IceConnectionState.CLOSED -> ICEStatus.CLOSED
                    else -> ICEStatus.NEW
                }
                scope.launch {
                    iceStatus = status
                    if (chat.onym.android.BuildConfig.DEBUG) {
                        Log.d(TAG, "ICE state: ${status.name}")
                    }
                }

                if (newState == PeerConnection.IceConnectionState.FAILED ||
                    newState == PeerConnection.IceConnectionState.DISCONNECTED) {
                    scope.launch {
                        delay(15_000)
                        if (this@CallManager.state == CallState.ACTIVE) {
                            val current = peerConnection?.iceConnectionState()
                            if (current == PeerConnection.IceConnectionState.FAILED ||
                                current == PeerConnection.IceConnectionState.DISCONNECTED) {
                                callEndReason = CallEndReason.ICE_FAILED
                                endCall(sendHangup = true)
                            }
                        }
                    }
                }
            }
        })
    }

    private fun addAudioTrack() {
        val factory = this.factory ?: return
        val audioSource = factory.createAudioSource(MediaConstraints())
        localAudioTrack = factory.createAudioTrack("audio0", audioSource)
        peerConnection?.addTrack(localAudioTrack, listOf("stream0"))
    }

    private fun addVideoTrack() {
        val factory = this.factory ?: return
        val egl = this.eglBase ?: return
        val videoSource = factory.createVideoSource(false)
        val helper = SurfaceTextureHelper.create("CaptureThread", egl.eglBaseContext)
        this.surfaceTextureHelper = helper

        val enumerator = Camera2Enumerator(context)
        val deviceName = if (isUsingFrontCamera) {
            enumerator.deviceNames.firstOrNull { enumerator.isFrontFacing(it) }
        } else {
            enumerator.deviceNames.firstOrNull { enumerator.isBackFacing(it) }
        } ?: enumerator.deviceNames.firstOrNull() ?: return

        val capturer = enumerator.createCapturer(deviceName, null) ?: return
        capturer.initialize(helper, context, videoSource.capturerObserver)
        this.capturer = capturer

        val track = factory.createVideoTrack("video0", videoSource)
        localVideoTrack = track
        peerConnection?.addTrack(track, listOf("stream0"))
        startCapture()
    }

    private fun startCapture() {
        capturer?.startCapture(640, 480, 30)
    }

    private fun mediaConstraints(video: Boolean) = MediaConstraints().apply {
        mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveAudio", "true"))
        mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveVideo", if (video) "true" else "false"))
    }

    private fun configureAudioSession(speaker: Boolean) {
        val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        audioManager.mode = AudioManager.MODE_IN_COMMUNICATION
        audioManager.isSpeakerphoneOn = speaker
    }

    private fun startDurationTimer() {
        callDuration = 0
        durationJob = scope.launch {
            while (isActive) {
                delay(1000)
                callDuration++
            }
        }
    }

    private fun startRingTimer() {
        ringJob = scope.launch {
            delay(30_000)
            if (state == CallState.RINGING) {
                callEndReason = CallEndReason.TIMEOUT
                endCall(sendHangup = direction == CallDirection.OUTGOING)
            }
        }
    }

    private fun generateCallId(): String {
        val bytes = ByteArray(32)
        SecureRandom().nextBytes(bytes)
        return bytes.joinToString("") { "%02x".format(it) }
    }
}

/** Adapter to reduce boilerplate for SDP callbacks. */
private open class SdpObserverAdapter : SdpObserver {
    override fun onCreateSuccess(sdp: SessionDescription) {}
    override fun onSetSuccess() {}
    override fun onCreateFailure(error: String?) {
        if (chat.onym.android.BuildConfig.DEBUG) Log.e("SdpObserver", "Create failed: $error")
    }
    override fun onSetFailure(error: String?) {
        if (chat.onym.android.BuildConfig.DEBUG) Log.e("SdpObserver", "Set failed: $error")
    }
}
