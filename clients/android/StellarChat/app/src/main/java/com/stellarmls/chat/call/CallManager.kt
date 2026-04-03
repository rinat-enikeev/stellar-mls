package com.stellarmls.chat.call

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

    companion object {
        private const val TAG = "CallManager"
        private val iceServers = listOf(
            PeerConnection.IceServer.builder("stun:stun.l.google.com:19302").createIceServer(),
            PeerConnection.IceServer.builder("stun:stun1.l.google.com:19302").createIceServer(),
            // EU TURN (Metered) — TCP on 443 for firewall compatibility
            PeerConnection.IceServer.builder("turn:eu-turn.metered.ca:443?transport=tcp")
                .setUsername("stellarchat")
                .setPassword("stellarchat-turn-2026")
                .createIceServer(),
            PeerConnection.IceServer.builder("turns:eu-turn.metered.ca:443?transport=tcp")
                .setUsername("stellarchat")
                .setPassword("stellarchat-turn-2026")
                .createIceServer(),
        )

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
                sendSignal?.invoke(signal)
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
                }
                CallConnectionService.reportIncomingCall(context, "Incoming Call", isVideoCall)
                startRingTimer()
            }
            "answer" -> {
                if (incomingCallId != callId || direction != CallDirection.OUTGOING) return
                // First-answer-wins: dismiss late answerers
                if (answerReceived) {
                    val hangup = JSONObject().apply {
                        put("action", "hangup")
                        put("callId", callId)
                        put("reason", "answered")
                    }
                    sendSignal?.invoke(hangup)
                    return
                }
                if (state != CallState.RINGING) return
                answerReceived = true
                ringJob?.cancel()
                val sdpStr = callJson.optString("sdp")
                if (sdpStr.isNotEmpty()) {
                    peerConnection?.setRemoteDescription(
                        SdpObserverAdapter(),
                        SessionDescription(SessionDescription.Type.ANSWER, sdpStr)
                    )
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
                    peerConnection?.addIceCandidate(
                        IceCandidate(sdpMid, sdpMLineIndex, candidateStr)
                    )
                }
            }
            "hangup" -> {
                if (incomingCallId != callId) return
                endCall(sendHangup = false)
            }
            "busy" -> {
                if (incomingCallId != callId || state != CallState.RINGING || direction != CallDirection.OUTGOING) return
                endCall(sendHangup = false)
            }
            "reject" -> {
                if (incomingCallId != callId || state != CallState.RINGING || direction != CallDirection.OUTGOING) return
                endCall(sendHangup = false)
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
                sendSignal?.invoke(signal)
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
        endCall(sendHangup = false)
    }

    fun hangup() = endCall(sendHangup = true)

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

        capturer?.stopCapture()
        capturer?.dispose()
        capturer = null
        surfaceTextureHelper?.dispose()
        surfaceTextureHelper = null
        peerConnection?.close()
        peerConnection = null
        localAudioTrack = null
        localVideoTrack = null
        remoteVideoTrack = null
        factory?.dispose()
        factory = null
        eglBase?.release()
        eglBase = null
        state = CallState.ENDED
        callDuration = 0
        isVideoCall = false
        isVideoEnabled = false
        answerReceived = false

        CallConnectionService.reportCallEnded()

        val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        audioManager.mode = AudioManager.MODE_NORMAL

        scope.launch {
            delay(1000)
            if (state == CallState.ENDED) {
                state = CallState.IDLE
                remoteBlsPubkey = null
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

    private fun setupPeerConnection() {
        val egl = EglBase.create()
        this.eglBase = egl

        val factory = PeerConnectionFactory.builder()
            .setVideoDecoderFactory(DefaultVideoDecoderFactory(egl.eglBaseContext))
            .setVideoEncoderFactory(DefaultVideoEncoderFactory(egl.eglBaseContext, true, true))
            .createPeerConnectionFactory()
        this.factory = factory

        val config = PeerConnection.RTCConfiguration(iceServers).apply {
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
                    remoteVideoTrack = track
                }
            }

            override fun onIceCandidate(candidate: IceCandidate) {
                val signal = JSONObject().apply {
                    put("action", "ice")
                    put("callId", callId)
                    put("candidate", candidate.sdp)
                    put("sdpMid", candidate.sdpMid)
                    put("sdpMLineIndex", candidate.sdpMLineIndex)
                }
                sendSignal?.invoke(signal)
            }

            override fun onIceCandidatesRemoved(candidates: Array<out IceCandidate>?) {}

            override fun onIceConnectionChange(newState: PeerConnection.IceConnectionState?) {
                if (newState == PeerConnection.IceConnectionState.FAILED ||
                    newState == PeerConnection.IceConnectionState.DISCONNECTED) {
                    scope.launch {
                        delay(15_000)
                        if (this@CallManager.state == CallState.ACTIVE) {
                            val current = peerConnection?.iceConnectionState()
                            if (current == PeerConnection.IceConnectionState.FAILED ||
                                current == PeerConnection.IceConnectionState.DISCONNECTED) {
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
        if (com.stellarmls.chat.BuildConfig.DEBUG) Log.e("SdpObserver", "Create failed: $error")
    }
    override fun onSetFailure(error: String?) {
        if (com.stellarmls.chat.BuildConfig.DEBUG) Log.e("SdpObserver", "Set failed: $error")
    }
}
