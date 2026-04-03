import CallKit
import AVFoundation

/// Bridges CallKit system UI with CallManager.
/// Reports incoming calls to the system, handles user actions (answer/decline/end)
/// from the lock screen or native call UI, and manages the audio session.
@MainActor
final class CallKitProvider: NSObject {
    private let provider: CXProvider
    private let callController = CXCallController()
    private weak var callManager: CallManager?

    /// Current CallKit call UUID — mapped 1:1 with callManager.callId.
    private var currentUUID: UUID?

    init(callManager: CallManager) {
        let config = CXProviderConfiguration()
        config.supportsVideo = true
        config.maximumCallGroups = 1
        config.supportedHandleTypes = [.generic]
        self.provider = CXProvider(configuration: config)
        self.callManager = callManager
        super.init()
        provider.setDelegate(self, queue: nil) // nil = main queue
    }

    // MARK: - Report to System

    /// Report an incoming call to CallKit (triggers system call UI).
    func reportIncomingCall(callerName: String, hasVideo: Bool) async throws {
        let uuid = UUID()
        currentUUID = uuid

        let update = CXCallUpdate()
        update.remoteHandle = CXHandle(type: .generic, value: callerName)
        update.localizedCallerName = callerName
        update.hasVideo = hasVideo
        update.supportsGrouping = false
        update.supportsHolding = false

        try await provider.reportNewIncomingCall(with: uuid, update: update)
    }

    /// Report that an outgoing call has started connecting.
    func reportOutgoingCallStarted() {
        guard let uuid = currentUUID else { return }
        provider.reportOutgoingCall(with: uuid, startedConnectingAt: Date())
    }

    /// Report that an outgoing call is now connected.
    func reportOutgoingCallConnected() {
        guard let uuid = currentUUID else { return }
        provider.reportOutgoingCall(with: uuid, connectedAt: Date())
    }

    /// Tell the system the call ended.
    func reportCallEnded(reason: CXCallEndedReason = .remoteEnded) {
        guard let uuid = currentUUID else { return }
        provider.reportCall(with: uuid, endedAt: Date(), reason: reason)
        currentUUID = nil
    }

    // MARK: - Request Actions (app-initiated)

    /// Request to start an outgoing call through CallKit.
    func startOutgoingCall(callerName: String, hasVideo: Bool) {
        let uuid = UUID()
        currentUUID = uuid
        let handle = CXHandle(type: .generic, value: callerName)
        let action = CXStartCallAction(call: uuid, handle: handle)
        action.isVideo = hasVideo
        callController.request(CXTransaction(action: action)) { error in
            if let error { print("CallKit startCall error: \(error)") }
        }
    }

    /// Request to answer the current incoming call through CallKit.
    func requestAnswerCall() {
        guard let uuid = currentUUID else { return }
        let action = CXAnswerCallAction(call: uuid)
        callController.request(CXTransaction(action: action)) { error in
            if let error { print("CallKit answerCall error: \(error)") }
        }
    }

    /// Request to end the current call through CallKit.
    func requestEndCall() {
        guard let uuid = currentUUID else { return }
        let action = CXEndCallAction(call: uuid)
        callController.request(CXTransaction(action: action)) { error in
            if let error { print("CallKit endCall error: \(error)") }
        }
    }
}

// MARK: - CXProviderDelegate

extension CallKitProvider: CXProviderDelegate {
    nonisolated func providerDidReset(_ provider: CXProvider) {
        Task { @MainActor in
            callManager?.endCall(sendHangup: false)
        }
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXAnswerCallAction) {
        Task { @MainActor in
            try? await callManager?.acceptCall()
            action.fulfill()
        }
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXEndCallAction) {
        Task { @MainActor in
            guard let callManager else {
                action.fulfill()
                return
            }
            if callManager.state == .ringing && callManager.direction == .incoming {
                callManager.rejectCall()
            } else {
                callManager.hangup()
            }
            action.fulfill()
        }
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXStartCallAction) {
        action.fulfill()
    }

    nonisolated func provider(_ provider: CXProvider, perform action: CXSetMutedCallAction) {
        Task { @MainActor in
            if callManager?.isMuted != action.isMuted {
                callManager?.toggleMute()
            }
            action.fulfill()
        }
    }

    nonisolated func provider(_ provider: CXProvider, didActivate audioSession: AVAudioSession) {
        // WebRTC handles audio session internally, but ensure it's active
    }

    nonisolated func provider(_ provider: CXProvider, didDeactivate audioSession: AVAudioSession) {
        // WebRTC handles audio session internally
    }
}
