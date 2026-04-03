import AVFoundation
import Foundation
import WebRTC

enum CallState {
    case idle
    case ringing       // Outgoing: waiting for answer. Incoming: showing call UI.
    case active        // Media flowing
    case ended
}

enum CallDirection {
    case outgoing
    case incoming
}

enum CallEndReason: String {
    case none
    case hangup
    case rejected
    case busy
    case timeout
    case iceFailed
    case answeredElsewhere
}

enum ICEStatus: String {
    case new
    case checking
    case connected
    case completed
    case failed
    case disconnected
    case closed
}

@MainActor @Observable
final class CallManager: NSObject {
    var state: CallState = .idle
    var direction: CallDirection = .outgoing
    var callId: String = ""
    var remoteBlsPubkey: Data?
    var isMuted = false
    var isSpeaker = false
    var isVideoEnabled = false
    var isUsingFrontCamera = true
    var isVideoCall = false
    var callDuration: TimeInterval = 0
    var iceStatus: ICEStatus = .new
    var callEndReason: CallEndReason = .none
    /// Tracks whether the first answer has been received (first-answer-wins).
    private var answerReceived = false

    /// Remote video track for rendering in the UI.
    var remoteVideoTrack: RTCVideoTrack?
    /// Local video track for PiP rendering.
    var localVideoTrack: RTCVideoTrack?

    /// Set by AppState — sends signaling JSON over the group channel.
    var sendSignal: (([String: Any]) async throws -> Void)?

    /// CallKit integration — set after init.
    var callKit: CallKitProvider?

    private var peerConnection: RTCPeerConnection?
    private var peerConnectionDelegate: CallPeerConnectionDelegate?
    private var localAudioTrack: RTCAudioTrack?
    private var capturer: RTCCameraVideoCapturer?
    private var factory: RTCPeerConnectionFactory?
    private var durationTimer: Timer?
    private var ringTimer: Timer?
    /// ICE candidates received before remote description is set.
    private var pendingCandidates: [RTCIceCandidate] = []

    /// Custom TURN server configuration. Set from AppState before calls.
    var turnURLs: [String] = []
    var turnUsername: String = ""
    var turnPassword: String = ""
    var turnEnabled: Bool = false

    private var iceServers: [RTCIceServer] {
        var servers: [RTCIceServer] = [
            RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"]),
            RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"]),
        ]
        // Built-in EU TURN (Metered) — TCP on 443 for firewall compatibility
        servers.append(RTCIceServer(
            urlStrings: [
                "turn:eu-turn.metered.ca:443?transport=tcp",
                "turns:eu-turn.metered.ca:443?transport=tcp",
            ],
            username: "stellarchat",
            credential: "stellarchat-turn-2026"
        ))
        // User-configured TURN servers
        if turnEnabled, !turnURLs.isEmpty, !turnUsername.isEmpty {
            servers.append(RTCIceServer(
                urlStrings: turnURLs,
                username: turnUsername,
                credential: turnPassword
            ))
        }
        return servers
    }

    // MARK: - Start Call (Outgoing)

    func startCall(video: Bool = false) async throws {
        guard state == .idle else { return }
        callId = Self.generateCallId()
        direction = .outgoing
        isVideoCall = video
        isVideoEnabled = video
        callEndReason = .none
        iceStatus = .new
        state = .ringing

        setupPeerConnection()
        addAudioTrack()
        if video { addVideoTrack() }

        let offer = try await createOffer(video: video)
        let signal: [String: Any] = [
            "action": "offer",
            "callId": callId,
            "mediaType": video ? "video" : "audio",
            "sdp": offer.sdp,
        ]
        try await sendSignal?(signal)
        callKit?.startOutgoingCall(callerName: "Call", hasVideo: video)
        startRingTimer()
    }

    // MARK: - Handle Incoming Signaling

    func handleSignal(_ callDict: [String: Any], senderBlsPubkey: Data) async {
        guard let action = callDict["action"] as? String else { return }
        let incomingCallId = callDict["callId"] as? String ?? ""

        switch action {
        case "offer":
            guard state == .idle else {
                // Already in a call — send busy
                let busy: [String: Any] = ["action": "busy", "callId": incomingCallId]
                try? await sendSignal?(busy)
                return
            }
            callId = incomingCallId
            direction = .incoming
            remoteBlsPubkey = senderBlsPubkey
            isVideoCall = (callDict["mediaType"] as? String) == "video"
            isVideoEnabled = isVideoCall
            state = .ringing

            setupPeerConnection()
            addAudioTrack()
            if isVideoCall { addVideoTrack() }

            if let sdp = callDict["sdp"] as? String {
                let remoteDesc = RTCSessionDescription(type: .offer, sdp: sdp)
                try? await peerConnection?.setRemoteDescription(remoteDesc)
                drainPendingCandidates()
            }
            try? await callKit?.reportIncomingCall(callerName: "Incoming Call", hasVideo: isVideoCall)
            startRingTimer()

        case "answer":
            guard incomingCallId == callId, direction == .outgoing else { return }
            // First-answer-wins: ignore duplicate/late answers.
            // Duplicates arrive from multiple relays for the same answer.
            // Late answerers will time out via the ring timer on their end.
            guard !answerReceived else { return }
            guard state == .ringing else { return }
            answerReceived = true
            ringTimer?.invalidate()
            if let sdp = callDict["sdp"] as? String {
                let remoteDesc = RTCSessionDescription(type: .answer, sdp: sdp)
                try? await peerConnection?.setRemoteDescription(remoteDesc)
                drainPendingCandidates()
            }
            state = .active
            startDurationTimer()
            configureAudioSession(speaker: isVideoCall)
            callKit?.reportOutgoingCallConnected()

        case "ice":
            guard incomingCallId == callId else { return }
            if let candidateStr = callDict["candidate"] as? String,
               let sdpMid = callDict["sdpMid"] as? String,
               let sdpMLineIndex = callDict["sdpMLineIndex"] as? Int {
                let candidate = RTCIceCandidate(
                    sdp: candidateStr,
                    sdpMLineIndex: Int32(sdpMLineIndex),
                    sdpMid: sdpMid
                )
                if peerConnection?.remoteDescription != nil {
                    try? await peerConnection?.add(candidate)
                } else {
                    pendingCandidates.append(candidate)
                }
            }

        case "hangup":
            guard incomingCallId == callId else { return }
            let reason = callDict["reason"] as? String
            // "answered" hangups are meant for late answerers still ringing,
            // not for connected parties. Ignore if already active.
            if reason == "answered", state == .active { return }
            callEndReason = reason == "answered" ? .answeredElsewhere : .hangup
            endCall(sendHangup: false)

        case "busy", "reject":
            // In a multi-member group, individual busy/reject responses should
            // not kill the entire call — other members may still answer.
            // The ring timer (30s) handles the case where nobody answers.
            break

        default:
            break
        }
    }

    // MARK: - Accept Call (Incoming)

    func acceptCall() async throws {
        guard state == .ringing, direction == .incoming else { return }
        ringTimer?.invalidate()

        let answer = try await createAnswer(video: isVideoCall)
        let signal: [String: Any] = [
            "action": "answer",
            "callId": callId,
            "sdp": answer.sdp,
        ]
        try await sendSignal?(signal)
        state = .active
        startDurationTimer()
        configureAudioSession(speaker: isVideoCall)
    }

    // MARK: - Reject / End

    func rejectCall() {
        guard state == .ringing, direction == .incoming else { return }
        let signal: [String: Any] = ["action": "reject", "callId": callId]
        Task { try? await sendSignal?(signal) }
        callEndReason = .rejected
        endCall(sendHangup: false)
    }

    func hangup() {
        callEndReason = .hangup
        endCall(sendHangup: true)
    }

    func endCall(sendHangup: Bool) {
        ringTimer?.invalidate()
        durationTimer?.invalidate()

        if sendHangup {
            let signal: [String: Any] = ["action": "hangup", "callId": callId]
            Task { try? await sendSignal?(signal) }
        }

        capturer?.stopCapture()
        capturer = nil
        peerConnection?.close()
        peerConnection = nil
        peerConnectionDelegate = nil
        localAudioTrack = nil
        localVideoTrack = nil
        remoteVideoTrack = nil
        factory = nil
        state = .ended
        callDuration = 0
        isVideoCall = false
        isVideoEnabled = false
        answerReceived = false
        pendingCandidates.removeAll()

        #if DEBUG
        print("[CallManager] Call ended: reason=\(callEndReason.rawValue)")
        #endif

        callKit?.reportCallEnded(reason: sendHangup ? .remoteEnded : .remoteEnded)

        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)

        // Return to idle after a brief delay
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(1))
            if self.state == .ended {
                self.state = .idle
                self.remoteBlsPubkey = nil
                self.iceStatus = .new
                self.callEndReason = .none
            }
        }
    }

    // MARK: - Mute / Speaker

    func toggleMute() {
        isMuted.toggle()
        localAudioTrack?.isEnabled = !isMuted
    }

    func toggleSpeaker() {
        isSpeaker.toggle()
        configureAudioSession(speaker: isSpeaker)
    }

    func toggleVideo() {
        isVideoEnabled.toggle()
        localVideoTrack?.isEnabled = isVideoEnabled
        if isVideoEnabled {
            startCapture()
        } else {
            capturer?.stopCapture()
        }
    }

    func flipCamera() {
        isUsingFrontCamera.toggle()
        startCapture()
    }

    // MARK: - Private

    private func drainPendingCandidates() {
        guard !pendingCandidates.isEmpty else { return }
        let candidates = pendingCandidates
        pendingCandidates.removeAll()
        Task {
            for candidate in candidates {
                try? await peerConnection?.add(candidate)
            }
        }
    }

    private func setupPeerConnection() {
        let encoderFactory = RTCDefaultVideoEncoderFactory()
        let decoderFactory = RTCDefaultVideoDecoderFactory()
        let factory = RTCPeerConnectionFactory(
            encoderFactory: encoderFactory,
            decoderFactory: decoderFactory
        )
        self.factory = factory

        let config = RTCConfiguration()
        let servers = iceServers
        config.iceServers = servers
        #if DEBUG
        let urls = servers.flatMap(\.urlStrings)
        print("[CallManager] ICE servers: \(urls)")
        #endif
        config.sdpSemantics = .unifiedPlan
        config.bundlePolicy = .maxBundle
        config.rtcpMuxPolicy = .require

        let constraints = RTCMediaConstraints(
            mandatoryConstraints: nil,
            optionalConstraints: nil
        )
        let pc = factory.peerConnection(with: config, constraints: constraints, delegate: nil)
        self.peerConnection = pc

        // Set delegate via the wrapper to forward ICE candidates.
        // Must store a strong reference — RTCPeerConnection.delegate is weak.
        let delegate = CallPeerConnectionDelegate(callManager: self)
        self.peerConnectionDelegate = delegate
        pc?.delegate = delegate
    }

    private func addAudioTrack() {
        guard let factory, let pc = peerConnection else { return }
        let audioSource = factory.audioSource(with: RTCMediaConstraints(
            mandatoryConstraints: nil,
            optionalConstraints: nil
        ))
        let track = factory.audioTrack(with: audioSource, trackId: "audio0")
        localAudioTrack = track
        pc.add(track, streamIds: ["stream0"])
    }

    private func addVideoTrack() {
        guard let factory, let pc = peerConnection else { return }
        let videoSource = factory.videoSource()
        let capturer = RTCCameraVideoCapturer(delegate: videoSource)
        self.capturer = capturer

        let track = factory.videoTrack(with: videoSource, trackId: "video0")
        localVideoTrack = track
        pc.add(track, streamIds: ["stream0"])
        startCapture()
    }

    private func startCapture() {
        guard let capturer else { return }
        let devices = RTCCameraVideoCapturer.captureDevices()
        let position: AVCaptureDevice.Position = isUsingFrontCamera ? .front : .back
        guard let device = devices.first(where: { $0.position == position }) ?? devices.first else { return }

        // Pick a reasonable format: 640x480 or closest
        let formats = RTCCameraVideoCapturer.supportedFormats(for: device)
        let targetWidth: Int32 = 640
        let format = formats.min(by: {
            let d0 = CMVideoFormatDescriptionGetDimensions($0.formatDescription)
            let d1 = CMVideoFormatDescriptionGetDimensions($1.formatDescription)
            return abs(d0.width - targetWidth) < abs(d1.width - targetWidth)
        }) ?? formats.first
        guard let format else { return }

        let fps = format.videoSupportedFrameRateRanges
            .max(by: { $0.maxFrameRate < $1.maxFrameRate })?
            .maxFrameRate ?? 30
        capturer.startCapture(with: device, format: format, fps: Int(min(fps, 30)))
    }

    private func createOffer(video: Bool = false) async throws -> RTCSessionDescription {
        let constraints = RTCMediaConstraints(
            mandatoryConstraints: [
                "OfferToReceiveAudio": "true",
                "OfferToReceiveVideo": video ? "true" : "false",
            ],
            optionalConstraints: nil
        )
        return try await withCheckedThrowingContinuation { continuation in
            peerConnection?.offer(for: constraints) { sdp, error in
                if let error {
                    continuation.resume(throwing: error)
                } else if let sdp {
                    self.peerConnection?.setLocalDescription(sdp) { setError in
                        if let setError {
                            continuation.resume(throwing: setError)
                        } else {
                            continuation.resume(returning: sdp)
                        }
                    }
                }
            }
        }
    }

    private func createAnswer(video: Bool = false) async throws -> RTCSessionDescription {
        let constraints = RTCMediaConstraints(
            mandatoryConstraints: [
                "OfferToReceiveAudio": "true",
                "OfferToReceiveVideo": video ? "true" : "false",
            ],
            optionalConstraints: nil
        )
        return try await withCheckedThrowingContinuation { continuation in
            peerConnection?.answer(for: constraints) { sdp, error in
                if let error {
                    continuation.resume(throwing: error)
                } else if let sdp {
                    self.peerConnection?.setLocalDescription(sdp) { setError in
                        if let setError {
                            continuation.resume(throwing: setError)
                        } else {
                            continuation.resume(returning: sdp)
                        }
                    }
                }
            }
        }
    }

    nonisolated func sendICECandidate(_ candidate: RTCIceCandidate) {
        Task { @MainActor in
            let signal: [String: Any] = [
                "action": "ice",
                "callId": callId,
                "candidate": candidate.sdp,
                "sdpMid": candidate.sdpMid ?? "0",
                "sdpMLineIndex": candidate.sdpMLineIndex,
            ]
            try? await sendSignal?(signal)
        }
    }

    private func configureAudioSession(speaker: Bool) {
        let session = AVAudioSession.sharedInstance()
        try? session.setCategory(.playAndRecord, mode: .voiceChat,
                                  options: speaker ? [.defaultToSpeaker, .allowBluetooth] : [.allowBluetooth])
        try? session.setActive(true)
    }

    private func startDurationTimer() {
        callDuration = 0
        durationTimer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.callDuration += 1
            }
        }
    }

    private func startRingTimer() {
        ringTimer = Timer.scheduledTimer(withTimeInterval: 30, repeats: false) { [weak self] _ in
            Task { @MainActor in
                guard let self, self.state == .ringing else { return }
                self.callEndReason = .timeout
                self.endCall(sendHangup: self.direction == .outgoing)
            }
        }
    }

    private static func generateCallId() -> String {
        var bytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, 32, &bytes)
        return bytes.map { String(format: "%02x", $0) }.joined()
    }
}

// MARK: - RTCPeerConnectionDelegate wrapper

private class CallPeerConnectionDelegate: NSObject, RTCPeerConnectionDelegate {
    private weak var callManager: CallManager?

    init(callManager: CallManager) {
        self.callManager = callManager
    }

    func peerConnection(_ peerConnection: RTCPeerConnection, didChange stateChanged: RTCSignalingState) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didAdd stream: RTCMediaStream) {
        Task { @MainActor in
            callManager?.remoteVideoTrack = stream.videoTracks.first
        }
    }
    func peerConnection(_ peerConnection: RTCPeerConnection, didRemove stream: RTCMediaStream) {}
    func peerConnectionShouldNegotiate(_ peerConnection: RTCPeerConnection) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didChange newState: RTCIceGatheringState) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didRemove candidates: [RTCIceCandidate]) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didOpen dataChannel: RTCDataChannel) {}

    // Modern track delivery (unified plan) — preferred over deprecated didAdd stream:
    func peerConnection(_ peerConnection: RTCPeerConnection, didAdd rtpReceiver: RTCRtpReceiver, streams: [RTCMediaStream]) {
        Task { @MainActor in
            if let videoTrack = rtpReceiver.track as? RTCVideoTrack {
                callManager?.remoteVideoTrack = videoTrack
            }
        }
    }

    func peerConnection(_ peerConnection: RTCPeerConnection, didGenerate candidate: RTCIceCandidate) {
        #if DEBUG
        let candidateType: String
        if candidate.sdp.contains("typ relay") {
            candidateType = "relay"
        } else if candidate.sdp.contains("typ srflx") {
            candidateType = "srflx"
        } else if candidate.sdp.contains("typ host") {
            candidateType = "host"
        } else {
            candidateType = "unknown"
        }
        print("[CallManager] ICE candidate: \(candidateType)")
        #endif
        callManager?.sendICECandidate(candidate)
    }

    func peerConnection(_ peerConnection: RTCPeerConnection, didChange newState: RTCIceConnectionState) {
        Task { @MainActor in
            guard let callManager else { return }

            let status: ICEStatus = switch newState {
            case .new: .new
            case .checking: .checking
            case .connected: .connected
            case .completed: .completed
            case .failed: .failed
            case .disconnected: .disconnected
            case .closed: .closed
            case .count: .new
            @unknown default: .new
            }
            callManager.iceStatus = status
            #if DEBUG
            print("[CallManager] ICE state: \(status.rawValue)")
            #endif

            if newState == .failed || newState == .disconnected {
                // Give 15 seconds for ICE to recover before ending
                try? await Task.sleep(for: .seconds(15))
                if callManager.state == .active {
                    let currentState = peerConnection.iceConnectionState
                    if currentState == .failed || currentState == .disconnected {
                        callManager.callEndReason = .iceFailed
                        callManager.endCall(sendHangup: true)
                    }
                }
            }
        }
    }
}
