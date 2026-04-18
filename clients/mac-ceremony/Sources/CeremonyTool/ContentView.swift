import SwiftUI
import AppKit
import UniformTypeIdentifiers

enum AppMode: String, CaseIterable, Identifiable {
    case contribute, verify
    var id: String { rawValue }
    var title: String {
        switch self {
        case .contribute: return "Contribute"
        case .verify:     return "Verify"
        }
    }
}

struct ContentView: View {
    @AppStorage("mode") private var modeRaw: String = AppMode.contribute.rawValue
    @AppStorage("tier") private var tier: String = "small"
    // Key name preserved from pre-1.5.8 builds (value may now be hex or npub1).
    @AppStorage("participantHex") private var participantKey: String = ""

    @State private var log: String = "Ready."
    @State private var isRunning = false
    @State private var outputURL: URL?
    @State private var currentProcess: Process?

    // Verify-mode inputs.
    @State private var beforeURL: URL?
    @State private var afterURL: URL?
    @State private var verifyOK: Bool?

    private let runner = CeremonyRunner()

    private var mode: AppMode {
        get { AppMode(rawValue: modeRaw) ?? .contribute }
    }

    var body: some View {
        VStack(spacing: 14) {
            header
            modePicker
            switch mode {
            case .contribute: contributeView
            case .verify:     verifyView
            }
            logView
            footer
        }
        .padding(16)
        .onChange(of: modeRaw) { _ in resetTransient() }
    }

    // MARK: - Shared

    private var header: some View {
        VStack(spacing: 2) {
            Text("Onym Trusted Setup Ceremony")
                .font(.title2).bold()
            Text(mode == .contribute
                 ? "Drop prev.zip — get contribution.zip back, upload via the web."
                 : "Verify any round locally against the bundled ceremony_tool.")
                .font(.caption).foregroundStyle(.secondary)
        }
    }

    private var modePicker: some View {
        Picker("", selection: $modeRaw) {
            ForEach(AppMode.allCases) { m in
                Text(m.title).tag(m.rawValue)
            }
        }
        .labelsHidden()
        .pickerStyle(.segmented)
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

    // MARK: - Contribute

    private var contributeView: some View {
        VStack(spacing: 10) {
            contributeConfig
            contributeDropZone
        }
    }

    private var contributeConfig: some View {
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
                TextField("npub1… or 64-char hex", text: $participantKey)
                    .textFieldStyle(.roundedBorder)
                    .disableAutocorrection(true)
            }
        }
    }

    private var contributeDropZone: some View {
        DropZone(
            placeholder: "Drop prev.zip here",
            systemImage: "arrow.down.doc.fill",
            isRunning: isRunning,
            runningLabel: "Computing contribution…"
        ) { url in
            Task { @MainActor in await handleContribute(url: url) }
        }
    }

    @MainActor
    private func handleContribute(url: URL) async {
        let hex: String
        do {
            hex = try normalizePubkey(participantKey)
        } catch {
            append("— Pubkey: \(error.localizedDescription)")
            return
        }
        isRunning = true
        outputURL = nil
        log = "Starting contribution for tier=\(tier), pubkey=\(hex.prefix(8))…\n"
        do {
            let out = try await runner.contribute(
                sourceURL: url,
                tier: tier,
                participantHex: hex,
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

    // MARK: - Verify

    private var verifyView: some View {
        VStack(spacing: 10) {
            Text("Drop one round to self-check (round 0 / genesis), or two rounds — "
                 + "previous and yours — for the full six-pairing contribution proof.")
                .font(.caption).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, alignment: .leading)
            HStack(spacing: 10) {
                DropZone(
                    placeholder: beforeURL?.lastPathComponent ?? "Drop previous round",
                    systemImage: beforeURL == nil ? "1.circle" : "1.circle.fill",
                    isRunning: false,
                    runningLabel: ""
                ) { url in
                    beforeURL = url
                    append("Before: \(url.lastPathComponent)")
                }
                DropZone(
                    placeholder: afterURL?.lastPathComponent ?? "Drop your round (optional)",
                    systemImage: afterURL == nil ? "2.circle" : "2.circle.fill",
                    isRunning: false,
                    runningLabel: ""
                ) { url in
                    afterURL = url
                    append("After: \(url.lastPathComponent)")
                }
            }
            .frame(minHeight: 120)
            HStack {
                Button("Reset") {
                    beforeURL = nil; afterURL = nil; verifyOK = nil; log = "Ready."
                }
                .disabled(isRunning || (beforeURL == nil && afterURL == nil))
                Spacer()
                Button(afterURL == nil ? "Verify round (self-check)"
                                       : "Verify contribution") {
                    Task { @MainActor in await handleVerify() }
                }
                .disabled(isRunning || beforeURL == nil)
                .keyboardShortcut(.defaultAction)
            }
        }
    }

    @MainActor
    private func handleVerify() async {
        guard let beforeURL = beforeURL else { return }
        isRunning = true
        verifyOK = nil
        log = afterURL == nil
            ? "Verifying \(beforeURL.lastPathComponent) (self-check)…\n"
            : "Verifying \(beforeURL.lastPathComponent) → \(afterURL!.lastPathComponent)…\n"
        do {
            try await runner.verify(
                beforeURL: beforeURL,
                afterURL: afterURL,
                log: { line in Task { @MainActor in self.append(line) } },
                registerProcess: { p in Task { @MainActor in self.currentProcess = p } }
            )
            verifyOK = true
            append("\nVERIFIED ✓")
        } catch {
            verifyOK = false
            append("\nFAILED ✗  \(error.localizedDescription)")
        }
        currentProcess = nil
        isRunning = false
    }

    // MARK: - Footer

    @ViewBuilder
    private var footer: some View {
        switch mode {
        case .contribute:
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
                HStack { Spacer(); Button("Cancel") { currentProcess?.terminate() } }
            }
        case .verify:
            if let ok = verifyOK {
                HStack {
                    Image(systemName: ok ? "checkmark.seal.fill" : "xmark.seal.fill")
                        .foregroundStyle(ok ? .green : .red)
                    Text(ok ? "Verified locally." : "Verification failed.")
                    Spacer()
                    Button("Open /verify in browser") {
                        if let u = URL(string: "https://ceremony.onym.chat/verify.html") {
                            NSWorkspace.shared.open(u)
                        }
                    }
                }
            } else if isRunning {
                HStack { Spacer(); Button("Cancel") { currentProcess?.terminate() } }
            }
        }
    }

    // MARK: - helpers

    private func resetTransient() {
        log = "Ready."
        outputURL = nil
        beforeURL = nil
        afterURL = nil
        verifyOK = nil
    }

    @MainActor
    private func append(_ line: String) {
        log += line + "\n"
    }
}

/// A single drop target with a dashed border, icon, and label.
private struct DropZone: View {
    let placeholder: String
    let systemImage: String
    let isRunning: Bool
    let runningLabel: String
    let onDrop: (URL) -> Void

    @State private var isTargeted = false

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 12)
                .strokeBorder(isTargeted ? Color.accentColor : Color.gray.opacity(0.5),
                              style: StrokeStyle(lineWidth: 2, dash: [8, 4]))
                .background(RoundedRectangle(cornerRadius: 12)
                    .fill(isTargeted ? Color.accentColor.opacity(0.08)
                                     : Color.gray.opacity(0.04)))
            VStack(spacing: 8) {
                if isRunning {
                    ProgressView().controlSize(.large)
                    Text(runningLabel).foregroundStyle(.secondary)
                } else {
                    Image(systemName: systemImage)
                        .font(.system(size: 36)).foregroundStyle(.secondary)
                    Text(placeholder)
                        .foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                        .padding(.horizontal, 8)
                }
            }
        }
        .frame(minHeight: 120)
        .onDrop(of: [.fileURL], isTargeted: $isTargeted) { providers in
            guard !isRunning, let provider = providers.first else { return false }
            _ = provider.loadObject(ofClass: URL.self) { url, _ in
                guard let url = url else { return }
                Task { @MainActor in onDrop(url) }
            }
            return true
        }
    }
}
