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

enum class CallState { IDLE, RINGING, ACTIVE, ENDED }
enum class CallDirection { OUTGOING, INCOMING }

class CallManager(private val context: Context) {
    var state by mutableStateOf(CallState.IDLE)
    var direction by mutableStateOf(CallDirection.OUTGOING)
    var callId by mutableStateOf("")
    var remoteBlsPubkey: ByteArray? = null
    var isMuted by mutableStateOf(false)
    var isSpeaker by mutableStateOf(false)
    var callDuration by mutableLongStateOf(0L)

    /** Set by ViewModel — sends signaling JSON over the group channel. */
    var sendSignal: ((JSONObject) -> Unit)? = null

    private var peerConnection: PeerConnection? = null
    private var localAudioTrack: AudioTrack? = null
    private var factory: PeerConnectionFactory? = null
    private val scope = CoroutineScope(Dispatchers.Main + SupervisorJob())
    private var durationJob: Job? = null
    private var ringJob: Job? = null

    companion object {
        private const val TAG = "CallManager"
        private val iceServers = listOf(
            PeerConnection.IceServer.builder("stun:stun.l.google.com:19302").createIceServer(),
            PeerConnection.IceServer.builder("stun:stun1.l.google.com:19302").createIceServer(),
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

    fun startCall() {
        if (state != CallState.IDLE) return
        callId = generateCallId()
        direction = CallDirection.OUTGOING
        state = CallState.RINGING

        setupPeerConnection()
        addAudioTrack()

        peerConnection?.createOffer(object : SdpObserverAdapter() {
            override fun onCreateSuccess(sdp: SessionDescription) {
                peerConnection?.setLocalDescription(SdpObserverAdapter(), sdp)
                val signal = JSONObject().apply {
                    put("action", "offer")
                    put("callId", callId)
                    put("mediaType", "audio")
                    put("sdp", sdp.description)
                }
                sendSignal?.invoke(signal)
            }
        }, audioOnlyConstraints())

        startRingTimer()
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
                state = CallState.RINGING

                setupPeerConnection()
                addAudioTrack()

                val sdpStr = callJson.optString("sdp")
                if (sdpStr.isNotEmpty()) {
                    peerConnection?.setRemoteDescription(
                        SdpObserverAdapter(),
                        SessionDescription(SessionDescription.Type.OFFER, sdpStr)
                    )
                }
                startRingTimer()
            }
            "answer" -> {
                if (incomingCallId != callId || state != CallState.RINGING || direction != CallDirection.OUTGOING) return
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
                configureAudioSession(false)
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
        }, audioOnlyConstraints())

        state = CallState.ACTIVE
        startDurationTimer()
        configureAudioSession(false)
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

        peerConnection?.close()
        peerConnection = null
        localAudioTrack = null
        factory?.dispose()
        factory = null
        state = CallState.ENDED
        callDuration = 0

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

    // MARK: - Private

    private fun setupPeerConnection() {
        val factory = PeerConnectionFactory.builder()
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
            override fun onAddTrack(receiver: RtpReceiver?, streams: Array<out MediaStream>?) {}

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

    private fun audioOnlyConstraints() = MediaConstraints().apply {
        mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveAudio", "true"))
        mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveVideo", "false"))
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
