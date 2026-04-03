import AVFoundation
import SwiftUI

struct VoiceRecordButton: View {
    let hasBlossomServers: Bool
    let onSend: (URL) -> Void

    @State private var isRecording = false
    @State private var isCancelled = false
    @State private var recorder: AVAudioRecorder?
    @State private var recordingURL: URL?
    @State private var dragOffset: CGFloat = 0
    @State private var recordingDuration: TimeInterval = 0
    @State private var durationTimer: Timer?

    private static let cancelThreshold: CGFloat = -60

    var body: some View {
        HStack(spacing: 4) {
            if isRecording {
                Circle()
                    .fill(.red)
                    .frame(width: 8, height: 8)
                Text(formattedDuration)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .monospacedDigit()
                if !isCancelled {
                    Text("Slide to cancel")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                } else {
                    Text("Release to cancel")
                        .font(.caption2)
                        .foregroundStyle(.red)
                }
            }

            Image(systemName: isRecording ? "mic.fill" : "mic")
                .font(.title3)
                .foregroundColor(
                    !hasBlossomServers ? .secondary :
                    isRecording ? .red : .primary
                )
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { value in
                            if !isRecording {
                                startRecording()
                            }
                            dragOffset = value.translation.width
                            isCancelled = dragOffset < Self.cancelThreshold
                        }
                        .onEnded { _ in
                            if isCancelled {
                                cancelRecording()
                            } else if recordingDuration >= 1 {
                                stopAndSend()
                            } else {
                                cancelRecording()
                            }
                        }
                )
                .disabled(!hasBlossomServers)
        }
    }

    private var formattedDuration: String {
        let secs = Int(recordingDuration)
        return String(format: "%d:%02d", secs / 60, secs % 60)
    }

    private func startRecording() {
        let session = AVAudioSession.sharedInstance()

        switch session.recordPermission {
        case .granted:
            break
        case .undetermined:
            session.requestRecordPermission { granted in
                if granted {
                    DispatchQueue.main.async { beginRecording() }
                }
            }
            return
        case .denied:
            return
        @unknown default:
            return
        }

        beginRecording()
    }

    private func beginRecording() {
        do {
            try AVAudioSession.sharedInstance().setCategory(.playAndRecord, mode: .default)
            try AVAudioSession.sharedInstance().setActive(true)
        } catch { return }

        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("m4a")

        let settings: [String: Any] = [
            AVFormatIDKey: Int(kAudioFormatMPEG4AAC),
            AVSampleRateKey: 44100,
            AVNumberOfChannelsKey: 1,
            AVEncoderBitRateKey: 64000,
        ]

        guard let rec = try? AVAudioRecorder(url: url, settings: settings) else { return }
        rec.record()

        recorder = rec
        recordingURL = url
        isRecording = true
        isCancelled = false
        dragOffset = 0
        recordingDuration = 0

        durationTimer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { _ in
            recordingDuration = rec.currentTime
        }
    }

    private func stopAndSend() {
        durationTimer?.invalidate()
        recorder?.stop()
        isRecording = false
        isCancelled = false

        if let url = recordingURL {
            onSend(url)
        }
        recorder = nil
        recordingURL = nil

        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    private func cancelRecording() {
        durationTimer?.invalidate()
        recorder?.stop()
        isRecording = false
        isCancelled = false

        if let url = recordingURL {
            try? FileManager.default.removeItem(at: url)
        }
        recorder = nil
        recordingURL = nil

        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }
}
