import SwiftUI
import WebRTC

struct CallView: View {
    let callManager: CallManager
    let remoteName: String
    let onDismiss: () -> Void

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            // Remote video (fullscreen background)
            if callManager.isVideoCall, let remoteTrack = callManager.remoteVideoTrack {
                RTCVideoView(track: remoteTrack)
                    .ignoresSafeArea()
            }

            VStack(spacing: 32) {
                // Local video PiP (top-right)
                if callManager.isVideoCall, let localTrack = callManager.localVideoTrack, callManager.isVideoEnabled {
                    HStack {
                        Spacer()
                        RTCVideoView(track: localTrack, mirror: callManager.isUsingFrontCamera)
                            .frame(width: 120, height: 160)
                            .clipShape(RoundedRectangle(cornerRadius: 12))
                            .padding(.top, 60)
                            .padding(.trailing, 16)
                    }
                }

                Spacer()

                // Only show name/status overlay when no remote video, or always for audio calls
                if !callManager.isVideoCall || callManager.remoteVideoTrack == nil {
                    Text(remoteName)
                        .font(.title)
                        .foregroundColor(.white)

                    Text(statusText)
                        .font(.subheadline)
                        .foregroundColor(.gray)
                }

                // Duration (active call only)
                if callManager.state == .active {
                    Text(formatDuration(callManager.callDuration))
                        .font(.title2)
                        .foregroundColor(.white)
                        .monospacedDigit()
                }

                Spacer()

                // Controls
                switch callManager.state {
                case .ringing:
                    if callManager.direction == .incoming {
                        incomingControls
                    } else {
                        outgoingControls
                    }
                case .active:
                    activeControls
                default:
                    EmptyView()
                }

                Spacer().frame(height: 40)
            }
        }
        .onChange(of: callManager.state) { _, newState in
            if newState == .idle {
                onDismiss()
            }
        }
    }

    private var statusText: String {
        switch callManager.state {
        case .idle: return ""
        case .ringing:
            return callManager.direction == .outgoing ? "Calling..." : "Incoming call"
        case .active: return "Connected"
        case .ended: return "Call ended"
        }
    }

    // MARK: - Incoming Call Controls

    private var incomingControls: some View {
        HStack(spacing: 60) {
            // Decline
            Button {
                callManager.rejectCall()
            } label: {
                Image(systemName: "phone.down.fill")
                    .font(.title)
                    .foregroundColor(.white)
                    .frame(width: 64, height: 64)
                    .background(Color.red)
                    .clipShape(Circle())
            }

            // Accept
            Button {
                if let callKit = callManager.callKit {
                    callKit.requestAnswerCall()
                } else {
                    Task { try? await callManager.acceptCall() }
                }
            } label: {
                Image(systemName: "phone.fill")
                    .font(.title)
                    .foregroundColor(.white)
                    .frame(width: 64, height: 64)
                    .background(Color.green)
                    .clipShape(Circle())
            }
        }
    }

    // MARK: - Outgoing Call Controls

    private var outgoingControls: some View {
        Button {
            callManager.hangup()
        } label: {
            Image(systemName: "phone.down.fill")
                .font(.title)
                .foregroundColor(.white)
                .frame(width: 64, height: 64)
                .background(Color.red)
                .clipShape(Circle())
        }
    }

    // MARK: - Active Call Controls

    private var activeControls: some View {
        VStack(spacing: 24) {
            if callManager.isVideoCall {
                HStack(spacing: 40) {
                    // Toggle video
                    Button {
                        callManager.toggleVideo()
                    } label: {
                        Image(systemName: callManager.isVideoEnabled ? "video.fill" : "video.slash.fill")
                            .font(.title2)
                            .foregroundColor(.white)
                            .frame(width: 56, height: 56)
                            .background(callManager.isVideoEnabled ? Color.gray.opacity(0.5) : Color.red)
                            .clipShape(Circle())
                    }

                    // Flip camera
                    Button {
                        callManager.flipCamera()
                    } label: {
                        Image(systemName: "camera.rotate.fill")
                            .font(.title2)
                            .foregroundColor(.white)
                            .frame(width: 56, height: 56)
                            .background(Color.gray.opacity(0.5))
                            .clipShape(Circle())
                    }
                }
            }

            HStack(spacing: 40) {
                // Mute
                Button {
                    callManager.toggleMute()
                } label: {
                    Image(systemName: callManager.isMuted ? "mic.slash.fill" : "mic.fill")
                        .font(.title2)
                        .foregroundColor(.white)
                        .frame(width: 56, height: 56)
                        .background(callManager.isMuted ? Color.red : Color.gray.opacity(0.5))
                        .clipShape(Circle())
                }

                // Speaker
                Button {
                    callManager.toggleSpeaker()
                } label: {
                    Image(systemName: callManager.isSpeaker ? "speaker.wave.3.fill" : "speaker.fill")
                        .font(.title2)
                        .foregroundColor(.white)
                        .frame(width: 56, height: 56)
                        .background(callManager.isSpeaker ? Color.blue : Color.gray.opacity(0.5))
                        .clipShape(Circle())
                }

                // Hang up
                Button {
                    callManager.hangup()
                } label: {
                    Image(systemName: "phone.down.fill")
                        .font(.title2)
                        .foregroundColor(.white)
                        .frame(width: 56, height: 56)
                        .background(Color.red)
                        .clipShape(Circle())
                }
            }
        }
    }

    private func formatDuration(_ seconds: TimeInterval) -> String {
        let mins = Int(seconds) / 60
        let secs = Int(seconds) % 60
        return String(format: "%d:%02d", mins, secs)
    }
}

// MARK: - WebRTC Video View Wrapper

struct RTCVideoView: UIViewRepresentable {
    let track: RTCVideoTrack
    var mirror: Bool = false

    func makeUIView(context: Context) -> RTCMTLVideoView {
        let view = RTCMTLVideoView()
        view.videoContentMode = .scaleAspectFill
        view.clipsToBounds = true
        track.add(view)
        return view
    }

    func updateUIView(_ uiView: RTCMTLVideoView, context: Context) {
        // Mirror is handled by transform on the container if needed
    }

    static func dismantleUIView(_ uiView: RTCMTLVideoView, coordinator: ()) {
        // Track removal handled by CallManager cleanup
    }
}
