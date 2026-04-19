import LocalAuthentication
import SwiftUI

struct RecoveryPhraseView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var scenePhase

    @State private var step: Step = .intro
    @State private var authError: String?
    @State private var obscured = false

    private enum Step {
        case intro
        case reveal(phrase: String, revealed: Bool)
        case verify(phrase: String, rounds: [VerifyRound], index: Int, state: VerifyState)
        case done
    }

    var body: some View {
        NavigationStack {
            stepContent
            .navigationTitle(navigationTitle)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    if case .done = step {
                        EmptyView()
                    } else {
                        Button("Cancel") { dismiss() }
                    }
                }
            }
            .alert("Authentication Failed", isPresented: errorBinding) {
                Button("Try Again") { authenticate() }
                Button("Cancel", role: .cancel) { dismiss() }
            } message: {
                Text(authError ?? "")
            }
            .overlay {
                if obscured {
                    Color(UIColor.systemBackground)
                        .ignoresSafeArea()
                        .overlay {
                            Image(systemName: "lock.fill")
                                .font(.largeTitle)
                                .foregroundStyle(.secondary)
                        }
                }
            }
            .onChange(of: scenePhase) { _, phase in
                obscured = phase != .active
            }
        }
    }

    @ViewBuilder
    private var stepContent: some View {
        switch step {
        case .intro:
            IntroScreen(onContinue: authenticate, onCancel: { dismiss() })
        case let .reveal(phrase, revealed):
            RevealScreen(
                phrase: phrase,
                revealed: revealed,
                onReveal: { step = .reveal(phrase: phrase, revealed: true) },
                onCopy: { copyPhrase(phrase) },
                onContinue: { beginVerification(phrase: phrase) }
            )
        case let .verify(phrase, rounds, index, state):
            VerifyScreen(
                round: rounds[index],
                progressIndex: index,
                progressTotal: rounds.count,
                state: state,
                onPick: { word in handlePick(word: word, phrase: phrase, rounds: rounds, index: index) }
            )
        case .done:
            DoneScreen(onDone: { dismiss() })
        }
    }

    private var navigationTitle: String {
        switch step {
        case .intro: return "Back up keys"
        case .reveal: return "Recovery phrase"
        case .verify: return "Verify"
        case .done: return ""
        }
    }

    private var errorBinding: Binding<Bool> {
        Binding(
            get: { authError != nil },
            set: { if !$0 { authError = nil } }
        )
    }

    private func authenticate() {
        let context = LAContext()
        var error: NSError?
        let ok = context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error)
        guard ok else {
            proceedToReveal()
            return
        }
        context.evaluatePolicy(
            .deviceOwnerAuthentication,
            localizedReason: "Authenticate to reveal your recovery phrase"
        ) { success, evalError in
            DispatchQueue.main.async {
                if success {
                    proceedToReveal()
                } else {
                    authError = evalError?.localizedDescription ?? "Authentication failed"
                }
            }
        }
    }

    private func proceedToReveal() {
        guard let phrase = appState.keyManager.recoveryPhrase else {
            authError = "Recovery phrase unavailable. Your identity may not be BIP39-backed."
            return
        }
        step = .reveal(phrase: phrase, revealed: false)
    }

    private func copyPhrase(_ phrase: String) {
        UIPasteboard.general.string = phrase
        DispatchQueue.main.asyncAfter(deadline: .now() + 60) {
            if UIPasteboard.general.string == phrase {
                UIPasteboard.general.string = ""
            }
        }
    }

    private func beginVerification(phrase: String) {
        let words = phrase.split(separator: " ").map(String.init)
        let positions = Array(0..<words.count).shuffled().prefix(3).sorted()
        let rounds: [VerifyRound] = positions.map { pos in
            var opts = Set<String>([words[pos]])
            while opts.count < 4 {
                if let candidate = words.randomElement(), candidate != words[pos] {
                    opts.insert(candidate)
                }
            }
            return VerifyRound(
                wordPosition: pos + 1,
                correct: words[pos],
                options: Array(opts).shuffled()
            )
        }
        step = .verify(phrase: phrase, rounds: rounds, index: 0, state: .idle)
    }

    private func handlePick(word: String, phrase: String, rounds: [VerifyRound], index: Int) {
        if word == rounds[index].correct {
            step = .verify(phrase: phrase, rounds: rounds, index: index, state: .correct)
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.45) {
                if index + 1 >= rounds.count {
                    step = .done
                } else {
                    step = .verify(phrase: phrase, rounds: rounds, index: index + 1, state: .idle)
                }
            }
        } else {
            step = .verify(phrase: phrase, rounds: rounds, index: index, state: .wrong(word))
        }
    }
}

// MARK: - Types

fileprivate struct VerifyRound: Equatable {
    let wordPosition: Int
    let correct: String
    let options: [String]
}

fileprivate enum VerifyState: Equatable {
    case idle
    case correct
    case wrong(String)
}

// MARK: - Intro

private struct IntroScreen: View {
    let onContinue: () -> Void
    let onCancel: () -> Void

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                heroCard
                    .padding(.horizontal, 16)
                    .padding(.top, 4)
                    .padding(.bottom, 18)

                sectionHeader("Before you start")
                VStack(spacing: 0) {
                    row(
                        icon: RoundedIcon(systemImage: "exclamationmark.triangle.fill", background: .orange),
                        title: "Never share or photograph",
                        separator: true
                    )
                    row(
                        icon: RoundedIcon(systemImage: "checkmark.shield.fill", background: .green),
                        title: "Store offline (paper or metal)",
                        separator: true
                    )
                    row(
                        icon: RoundedIcon(systemImage: "lock.fill", background: Color(UIColor.systemGray)),
                        title: "Anyone with it can read your chats",
                        separator: false
                    )
                }
                .background(
                    Color(UIColor.secondarySystemGroupedBackground),
                    in: RoundedRectangle(cornerRadius: 10, style: .continuous)
                )
                .padding(.horizontal, 16)

                VStack(spacing: 10) {
                    Button(action: onContinue) {
                        HStack(spacing: 8) {
                            Image(systemName: "faceid")
                            Text("Continue with Face ID")
                                .fontWeight(.semibold)
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                        .foregroundStyle(Color.white)
                        .background(Color.accentColor, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    }
                    Button(action: onCancel) {
                        Text("Not now")
                            .fontWeight(.semibold)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 14)
                            .foregroundStyle(Color.accentColor)
                            .background(
                                Color(UIColor.secondarySystemGroupedBackground),
                                in: RoundedRectangle(cornerRadius: 14, style: .continuous)
                            )
                    }
                }
                .padding(.horizontal, 16)
                .padding(.top, 22)
            }
            .padding(.bottom, 28)
        }
        .background(Color(UIColor.systemGroupedBackground))
    }

    private var heroCard: some View {
        VStack(spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 22, style: .continuous)
                    .fill(LinearGradient(
                        colors: [Color.accentColor, Color.purple],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    ))
                    .frame(width: 72, height: 72)
                    .shadow(color: Color.accentColor.opacity(0.25), radius: 10, x: 0, y: 8)
                Image(systemName: "key.fill")
                    .font(.system(size: 30, weight: .semibold))
                    .foregroundStyle(.white)
            }
            Text("Your identity, in 12 words")
                .font(.title2.weight(.bold))
                .multilineTextAlignment(.center)
            Text("Write them down. Keep them offline. This phrase restores your Nostr, Stellar, and BLS keys on any device.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .padding(.vertical, 22)
        .padding(.horizontal, 20)
        .frame(maxWidth: .infinity)
        .background(
            Color(UIColor.secondarySystemGroupedBackground),
            in: RoundedRectangle(cornerRadius: 18, style: .continuous)
        )
    }

    private func sectionHeader(_ text: String) -> some View {
        Text(text.uppercased())
            .font(.footnote)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 32)
            .padding(.bottom, 6)
    }

    private func row(icon: RoundedIcon, title: String, separator: Bool) -> some View {
        HStack(spacing: 12) {
            icon
            Text(title)
                .font(.body)
            Spacer()
        }
        .padding(.horizontal, 16)
        .frame(minHeight: 44)
        .overlay(alignment: .bottom) {
            if separator {
                Rectangle()
                    .fill(Color(UIColor.separator).opacity(0.5))
                    .frame(height: 0.5)
                    .padding(.leading, 58)
            }
        }
    }
}

// MARK: - Reveal

private struct RevealScreen: View {
    let phrase: String
    let revealed: Bool
    let onReveal: () -> Void
    let onCopy: () -> Void
    let onContinue: () -> Void

    @State private var copyConfirmShown = false

    private var words: [String] {
        phrase.split(separator: " ").map(String.init)
    }

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                Text("Write down these \(words.count) words in order. You'll confirm three of them on the next screen.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 32)
                    .padding(.top, 12)
                    .padding(.bottom, 8)

                phraseCard
                    .padding(.horizontal, 16)
                    .padding(.bottom, 12)

                HStack(spacing: 8) {
                    Button(action: {
                        onCopy()
                        copyConfirmShown = true
                    }) {
                        Label("Copy", systemImage: "doc.on.doc")
                            .font(.body.weight(.semibold))
                            .foregroundStyle(Color.accentColor)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 12)
                            .background(
                                Color.accentColor.opacity(0.12),
                                in: RoundedRectangle(cornerRadius: 14, style: .continuous)
                            )
                    }
                    .disabled(!revealed)
                    .opacity(revealed ? 1 : 0.4)
                }
                .padding(.horizontal, 16)

                Button(action: onContinue) {
                    Text("I've written it down")
                        .fontWeight(.semibold)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 14)
                        .foregroundStyle(Color.white)
                        .background(Color.accentColor, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                }
                .disabled(!revealed)
                .opacity(revealed ? 1 : 0.5)
                .padding(.horizontal, 16)
                .padding(.top, 14)

                Text("Screenshots are blocked on this screen. The phrase is generated on-device and never sent to StellarChat.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 32)
                    .padding(.top, 18)
            }
            .padding(.bottom, 28)
        }
        .background(Color(UIColor.systemGroupedBackground))
        .alert("Copied", isPresented: $copyConfirmShown) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("Recovery phrase copied. It will be cleared from clipboard in 60 seconds. Store it securely now.")
        }
    }

    private var phraseCard: some View {
        ZStack {
            LazyVGrid(
                columns: [GridItem(.flexible(), spacing: 14), GridItem(.flexible())],
                spacing: 10
            ) {
                ForEach(Array(words.enumerated()), id: \.offset) { index, word in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("\(index + 1)")
                            .font(.caption2.monospaced())
                            .foregroundStyle(.tertiary)
                            .frame(width: 18, alignment: .trailing)
                            .monospacedDigit()
                        Text(word)
                            .font(.callout.weight(.medium))
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 7)
                    .background(
                        Color.primary.opacity(0.03),
                        in: RoundedRectangle(cornerRadius: 8, style: .continuous)
                    )
                }
            }
            .padding(16)
            .blur(radius: revealed ? 0 : 10)
            .allowsHitTesting(revealed)

            if !revealed {
                Button(action: onReveal) {
                    VStack(spacing: 10) {
                        ZStack {
                            Circle()
                                .fill(Color.primary.opacity(0.08))
                                .frame(width: 44, height: 44)
                            Image(systemName: "eye.slash")
                                .font(.system(size: 18, weight: .semibold))
                                .foregroundStyle(Color.primary)
                        }
                        Text("Tap to reveal")
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(Color.primary)
                    }
                }
                .contentShape(Rectangle())
            }
        }
        .frame(maxWidth: .infinity)
        .background(
            Color(UIColor.secondarySystemGroupedBackground),
            in: RoundedRectangle(cornerRadius: 18, style: .continuous)
        )
    }
}

// MARK: - Verify

private struct VerifyScreen: View {
    let round: VerifyRound
    let progressIndex: Int
    let progressTotal: Int
    let state: VerifyState
    let onPick: (String) -> Void

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                ProgressDots(current: progressIndex, total: progressTotal)
                    .padding(.top, 6)
                    .padding(.bottom, 22)

                VStack(spacing: 6) {
                    Text("Select word number")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    Text("\(round.wordPosition)")
                        .font(.system(size: 64, weight: .bold))
                        .foregroundStyle(Color.accentColor)
                        .monospacedDigit()
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 20)
                .padding(.horizontal, 16)
                .background(
                    Color(UIColor.secondarySystemGroupedBackground),
                    in: RoundedRectangle(cornerRadius: 18, style: .continuous)
                )
                .padding(.horizontal, 16)
                .padding(.bottom, 20)

                VStack(spacing: 10) {
                    ForEach(round.options, id: \.self) { option in
                        VerifyOption(
                            word: option,
                            isCorrect: state == .correct && option == round.correct,
                            isWrong: state == .wrong(option),
                            onTap: { onPick(option) }
                        )
                    }
                }
                .padding(.horizontal, 16)

                if case .wrong = state {
                    Text("Not the right word. Check your phrase and try again.")
                        .font(.footnote)
                        .foregroundStyle(.red)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: .infinity)
                        .padding(.horizontal, 32)
                        .padding(.top, 18)
                }
            }
            .padding(.bottom, 28)
        }
        .background(Color(UIColor.systemGroupedBackground))
    }
}

private struct VerifyOption: View {
    let word: String
    let isCorrect: Bool
    let isWrong: Bool
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack {
                Text(word)
                    .font(.body.weight(.medium))
                Spacer()
                if isCorrect {
                    ZStack {
                        Circle()
                            .fill(Color.green)
                            .frame(width: 22, height: 22)
                        Image(systemName: "checkmark")
                            .font(.system(size: 12, weight: .bold))
                            .foregroundStyle(.white)
                    }
                }
            }
            .foregroundStyle(foreground)
            .padding(.horizontal, 18)
            .padding(.vertical, 16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(background, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(border, lineWidth: 1.5)
            )
        }
        .buttonStyle(.plain)
    }

    private var background: Color {
        if isCorrect { return Color.green.opacity(0.14) }
        if isWrong { return Color.red.opacity(0.12) }
        return Color(UIColor.secondarySystemGroupedBackground)
    }

    private var border: Color {
        if isCorrect { return Color.green }
        if isWrong { return Color.red }
        return Color.clear
    }

    private var foreground: Color {
        if isCorrect { return Color.green }
        if isWrong { return Color.red }
        return Color.primary
    }
}

private struct ProgressDots: View {
    let current: Int
    let total: Int

    var body: some View {
        HStack(spacing: 8) {
            ForEach(0..<total, id: \.self) { i in
                Capsule()
                    .fill(i == current ? Color.accentColor : Color(UIColor.systemGray4))
                    .frame(width: i == current ? 24 : 8, height: 8)
                    .animation(.easeInOut(duration: 0.2), value: current)
            }
        }
    }
}

// MARK: - Done

private struct DoneScreen: View {
    let onDone: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            Spacer()
            ZStack {
                Circle()
                    .fill(LinearGradient(
                        colors: [Color.green, Color(red: 0.2, green: 0.78, blue: 0.35)],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    ))
                    .frame(width: 96, height: 96)
                    .shadow(color: Color.green.opacity(0.4), radius: 16, x: 0, y: 12)
                Image(systemName: "checkmark")
                    .font(.system(size: 44, weight: .bold))
                    .foregroundStyle(.white)
            }
            .padding(.bottom, 24)

            Text("Backup verified")
                .font(.title.weight(.bold))
                .padding(.bottom, 10)
            Text("Your recovery phrase is confirmed. Store it somewhere safe — you'll only need it if you lose this device.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 28)
                .padding(.bottom, 36)

            Button(action: onDone) {
                Text("Done")
                    .fontWeight(.semibold)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 14)
                    .foregroundStyle(.white)
                    .background(Color.accentColor, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            }
            .padding(.horizontal, 16)

            Spacer()

            Text(footer)
                .font(.caption2)
                .foregroundStyle(.tertiary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
                .padding(.bottom, 32)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(UIColor.systemGroupedBackground))
    }

    private var footer: String {
        let df = DateFormatter()
        df.dateStyle = .medium
        return "Backed up \(df.string(from: Date())) · BIP-39 English"
    }
}

// MARK: - Shared atoms

private struct RoundedIcon: View {
    let systemImage: String
    let background: Color

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 7, style: .continuous)
                .fill(background)
                .frame(width: 30, height: 30)
            Image(systemName: systemImage)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(.white)
        }
    }
}
