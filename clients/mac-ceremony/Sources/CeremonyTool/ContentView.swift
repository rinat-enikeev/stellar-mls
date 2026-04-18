import SwiftUI
import AppKit
import UniformTypeIdentifiers

struct ContentView: View {
    @AppStorage("tier") private var tier: String = "small"
    @AppStorage("participantHex") private var participantHex: String = ""

    @State private var log: String = "Ready. Fill in your Nostr hex pubkey, then drop prev.zip below."
    @State private var isDragTarget = false
    @State private var isRunning = false
    @State private var outputURL: URL?
    @State private var currentProcess: Process?

    private let runner = CeremonyRunner()

    var body: some View {
        VStack(spacing: 14) {
            header
            config
            dropZone
            logView
            footer
        }
        .padding(16)
    }

    private var header: some View {
        VStack(spacing: 2) {
            Text("Onym Trusted Setup Ceremony")
                .font(.title2).bold()
            Text("Drop prev.zip — get contribution.zip back, upload via the web.")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    private var config: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Tier").frame(width: 80, alignment: .leading)
                Picker("", selection: $tier) {
                    Text("Small").tag("small")
                    Text("Medium").tag("medium")
                    Text("Large").tag("large")
                }
                .labelsHidden().pickerStyle(.segmented)
            }
            HStack {
                Text("Pubkey").frame(width: 80, alignment: .leading)
                TextField("64-char Nostr hex pubkey", text: $participantHex)
                    .textFieldStyle(.roundedBorder)
                    .disableAutocorrection(true)
            }
        }
    }

    private var dropZone: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 12)
                .strokeBorder(isDragTarget ? Color.accentColor : Color.gray.opacity(0.5),
                              style: StrokeStyle(lineWidth: 2, dash: [8, 4]))
                .background(RoundedRectangle(cornerRadius: 12)
                    .fill(isDragTarget ? Color.accentColor.opacity(0.08) : Color.gray.opacity(0.04)))
            VStack(spacing: 8) {
                if isRunning {
                    ProgressView().controlSize(.large)
                    Text("Computing contribution…").foregroundStyle(.secondary)
                } else {
                    Image(systemName: "arrow.down.doc.fill")
                        .font(.system(size: 44)).foregroundStyle(.secondary)
                    Text("Drop prev.zip here").foregroundStyle(.secondary)
                }
            }
        }
        .frame(minHeight: 130)
        .onDrop(of: [.fileURL], isTargeted: $isDragTarget) { providers in
            guard !isRunning, let provider = providers.first else { return false }
            _ = provider.loadObject(ofClass: URL.self) { url, _ in
                guard let url = url else { return }
                Task { @MainActor in await handleDrop(url: url) }
            }
            return true
        }
    }

    private var logView: some View {
        ScrollViewReader { proxy in
            ScrollView {
                Text(log)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
                    .id("logBottom")
            }
            .background(Color(nsColor: .textBackgroundColor))
            .overlay(RoundedRectangle(cornerRadius: 6)
                .stroke(Color.gray.opacity(0.25)))
            .frame(minHeight: 140)
            .onChange(of: log) { _ in
                proxy.scrollTo("logBottom", anchor: .bottom)
            }
        }
    }

    @ViewBuilder
    private var footer: some View {
        if let url = outputURL {
            HStack {
                Text("Ready: \(url.lastPathComponent)")
                    .lineLimit(1).truncationMode(.middle)
                Spacer()
                Button("Reveal in Finder") {
                    NSWorkspace.shared.activateFileViewerSelecting([url])
                }
                Button("Upload in browser") {
                    if let u = URL(string: "https://ceremony.onym.chat/contribute.html") {
                        NSWorkspace.shared.open(u)
                    }
                }
                .keyboardShortcut(.defaultAction)
            }
        } else if isRunning {
            HStack {
                Spacer()
                Button("Cancel") {
                    currentProcess?.terminate()
                }
            }
        }
    }

    @MainActor
    private func handleDrop(url: URL) async {
        guard validatePubkey(participantHex) else {
            append("— Pubkey must be 64 hex characters. Fix and drop again.")
            return
        }
        isRunning = true
        outputURL = nil
        log = "Starting contribution for tier=\(tier), pubkey=\(participantHex.prefix(8))…\n"
        do {
            let out = try await runner.contribute(
                sourceURL: url,
                tier: tier,
                participantHex: participantHex,
                log: { line in Task { @MainActor in self.append(line) } },
                registerProcess: { p in Task { @MainActor in self.currentProcess = p } }
            )
            outputURL = out
            append("\nDone. Output: \(out.path)\nUpload via ceremony.onym.chat/contribute.html.")
        } catch {
            append("\nFAILED: \(error.localizedDescription)")
        }
        currentProcess = nil
        isRunning = false
    }

    @MainActor
    private func append(_ line: String) {
        log += line + "\n"
    }

    private func validatePubkey(_ s: String) -> Bool {
        guard s.count == 64 else { return false }
        return s.allSatisfy { $0.isHexDigit }
    }
}

private extension Character {
    var isHexDigit: Bool {
        switch self {
        case "0"..."9", "a"..."f", "A"..."F": return true
        default: return false
        }
    }
}
