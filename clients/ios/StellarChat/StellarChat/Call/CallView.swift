import SwiftUI

struct CallView: View {
    let callManager: CallManager
    let remoteName: String
    let onDismiss: () -> Void

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            VStack(spacing: 32) {
                Spacer()

                // Remote peer name
                Text(remoteName)
                    .font(.title)
                    .foregroundColor(.white)

                // Status label
                Text(statusText)
                    .font(.subheadline)
                    .foregroundColor(.gray)

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
                Task { try? await callManager.acceptCall() }
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

    private func formatDuration(_ seconds: TimeInterval) -> String {
        let mins = Int(seconds) / 60
        let secs = Int(seconds) % 60
        return String(format: "%d:%02d", mins, secs)
    }
}
