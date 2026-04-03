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

@MainActor @Observable
final class CallManager: NSObject {
    var state: CallState = .idle
    var direction: CallDirection = .outgoing
    var callId: String = ""
    var remoteBlsPubkey: Data?
    var isMuted = false
    var isSpeaker = false
    var callDuration: TimeInterval = 0

    /// Set by AppState — sends signaling JSON over the group channel.
    var sendSignal: (([String: Any]) async throws -> Void)?

    private var peerConnection: RTCPeerConnection?
    private var localAudioTrack: RTCAudioTrack?
    private var factory: RTCPeerConnectionFactory?
    private var durationTimer: Timer?
    private var ringTimer: Timer?

    private static let iceServers = [
        RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"]),
        RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"]),
    ]

    // MARK: - Start Call (Outgoing)

    func startCall() async throws {
        guard state == .idle else { return }
        callId = Self.generateCallId()
        direction = .outgoing
        state = .ringing

        setupPeerConnection()
        addAudioTrack()

        let offer = try await createOffer()
        let signal: [String: Any] = [
            "action": "offer",
            "callId": callId,
            "mediaType": "audio",
            "sdp": offer.sdp,
        ]
        try await sendSignal?(signal)
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
            state = .ringing

            setupPeerConnection()
            addAudioTrack()

            if let sdp = callDict["sdp"] as? String {
                let remoteDesc = RTCSessionDescription(type: .offer, sdp: sdp)
                try? await peerConnection?.setRemoteDescription(remoteDesc)
            }
            startRingTimer()

        case "answer":
            guard incomingCallId == callId, state == .ringing, direction == .outgoing else { return }
            ringTimer?.invalidate()
            if let sdp = callDict["sdp"] as? String {
                let remoteDesc = RTCSessionDescription(type: .answer, sdp: sdp)
                try? await peerConnection?.setRemoteDescription(remoteDesc)
            }
            state = .active
            startDurationTimer()
            configureAudioSession(speaker: false)

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
                try? await peerConnection?.add(candidate)
            }

        case "hangup":
            guard incomingCallId == callId else { return }
            endCall(sendHangup: false)

        case "busy":
            guard incomingCallId == callId, state == .ringing, direction == .outgoing else { return }
            endCall(sendHangup: false)

        case "reject":
            guard incomingCallId == callId, state == .ringing, direction == .outgoing else { return }
            endCall(sendHangup: false)

        default:
            break
        }
    }

    // MARK: - Accept Call (Incoming)

    func acceptCall() async throws {
        guard state == .ringing, direction == .incoming else { return }
        ringTimer?.invalidate()

        let answer = try await createAnswer()
        let signal: [String: Any] = [
            "action": "answer",
            "callId": callId,
            "sdp": answer.sdp,
        ]
        try await sendSignal?(signal)
        state = .active
        startDurationTimer()
        configureAudioSession(speaker: false)
    }

    // MARK: - Reject / End

    func rejectCall() {
        guard state == .ringing, direction == .incoming else { return }
        let signal: [String: Any] = ["action": "reject", "callId": callId]
        Task { try? await sendSignal?(signal) }
        endCall(sendHangup: false)
    }

    func hangup() {
        endCall(sendHangup: true)
    }

    func endCall(sendHangup: Bool) {
        ringTimer?.invalidate()
        durationTimer?.invalidate()

        if sendHangup {
            let signal: [String: Any] = ["action": "hangup", "callId": callId]
            Task { try? await sendSignal?(signal) }
        }

        peerConnection?.close()
        peerConnection = nil
        localAudioTrack = nil
        factory = nil
        state = .ended
        callDuration = 0

        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)

        // Return to idle after a brief delay
        Task { @MainActor in
            try? await Task.sleep(for: .seconds(1))
            if self.state == .ended {
                self.state = .idle
                self.remoteBlsPubkey = nil
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

    // MARK: - Private

    private func setupPeerConnection() {
        let factory = RTCPeerConnectionFactory()
        self.factory = factory

        let config = RTCConfiguration()
        config.iceServers = Self.iceServers
        config.sdpSemantics = .unifiedPlan
        config.bundlePolicy = .maxBundle
        config.rtcpMuxPolicy = .require

        let constraints = RTCMediaConstraints(
            mandatoryConstraints: nil,
            optionalConstraints: nil
        )
        let pc = factory.peerConnection(with: config, constraints: constraints, delegate: nil)
        self.peerConnection = pc

        // Set delegate via the wrapper to forward ICE candidates
        pc?.delegate = CallPeerConnectionDelegate(callManager: self)
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

    private func createOffer() async throws -> RTCSessionDescription {
        let constraints = RTCMediaConstraints(
            mandatoryConstraints: [
                "OfferToReceiveAudio": "true",
                "OfferToReceiveVideo": "false",
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

    private func createAnswer() async throws -> RTCSessionDescription {
        let constraints = RTCMediaConstraints(
            mandatoryConstraints: [
                "OfferToReceiveAudio": "true",
                "OfferToReceiveVideo": "false",
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
    func peerConnection(_ peerConnection: RTCPeerConnection, didAdd stream: RTCMediaStream) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didRemove stream: RTCMediaStream) {}
    func peerConnectionShouldNegotiate(_ peerConnection: RTCPeerConnection) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didChange newState: RTCIceGatheringState) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didRemove candidates: [RTCIceCandidate]) {}
    func peerConnection(_ peerConnection: RTCPeerConnection, didOpen dataChannel: RTCDataChannel) {}

    func peerConnection(_ peerConnection: RTCPeerConnection, didGenerate candidate: RTCIceCandidate) {
        callManager?.sendICECandidate(candidate)
    }

    func peerConnection(_ peerConnection: RTCPeerConnection, didChange newState: RTCIceConnectionState) {
        Task { @MainActor in
            guard let callManager else { return }
            if newState == .failed || newState == .disconnected {
                // Give 15 seconds for ICE to recover before ending
                try? await Task.sleep(for: .seconds(15))
                if callManager.state == .active {
                    let currentState = peerConnection.iceConnectionState
                    if currentState == .failed || currentState == .disconnected {
                        callManager.endCall(sendHangup: true)
                    }
                }
            }
        }
    }
}
