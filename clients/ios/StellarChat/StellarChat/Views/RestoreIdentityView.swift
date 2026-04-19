import SwiftUI

struct RestoreIdentityView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var words: [String] = Array(repeating: "", count: 12)
    @State private var activeSlot: Int = 0
    @State private var currentInput: String = ""
    @State private var errorMessage: String?
    @State private var isRestoring = false
    @State private var restored = false
    @State private var showConfirmDialog = false

    @FocusState private var inputFocused: Bool

    var onRestoreComplete: (() -> Void)?

    private var suggestions: [String] {
        Bip39.suggestions(prefix: currentInput, limit: 4)
    }

    private var allFilled: Bool {
        words.allSatisfy { !$0.isEmpty }
    }

    private var joinedPhrase: String {
        words.map { $0.lowercased() }.joined(separator: " ")
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(spacing: 0) {
                    Text("Type the 12 words from your recovery phrase. We'll suggest matches as you go.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 32)
                        .padding(.top, 12)
                        .padding(.bottom, 10)

                    slotGrid
                        .padding(.horizontal, 16)

                    if !suggestions.isEmpty {
                        suggestionChips
                            .padding(.horizontal, 16)
                            .padding(.top, 10)
                    }

                    if let error = errorMessage {
                        HStack(spacing: 8) {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .foregroundStyle(.red)
                            Text(error)
                                .font(.footnote)
                                .foregroundStyle(.red)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 32)
                        .padding(.top, 14)
                    }

                    if restored {
                        HStack(spacing: 8) {
                            Image(systemName: "checkmark.circle.fill")
                                .foregroundStyle(.green)
                            Text("Identity restored successfully")
                                .font(.footnote)
                                .foregroundStyle(.green)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 32)
                        .padding(.top, 14)
                    }

                    Button {
                        if let text = UIPasteboard.general.string {
                            applyPaste(text)
                        }
                    } label: {
                        Label("Paste phrase", systemImage: "doc.on.clipboard")
                            .font(.body.weight(.semibold))
                            .foregroundStyle(Color.accentColor)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 12)
                            .background(
                                Color.accentColor.opacity(0.12),
                                in: RoundedRectangle(cornerRadius: 14, style: .continuous)
                            )
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 18)
                    .disabled(isRestoring || restored)

                    Button {
                        showConfirmDialog = true
                    } label: {
                        if isRestoring {
                            ProgressView()
                                .progressViewStyle(.circular)
                                .tint(.white)
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 14)
                                .background(Color.accentColor, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                        } else {
                            Text("Restore Identity")
                                .fontWeight(.semibold)
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 14)
                                .foregroundStyle(.white)
                                .background(Color.accentColor, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                        }
                    }
                    .disabled(!allFilled || isRestoring || restored)
                    .opacity((!allFilled || isRestoring || restored) ? 0.5 : 1)
                    .padding(.horizontal, 16)
                    .padding(.top, 10)
                }
                .padding(.bottom, 40)
            }
            .background(Color(UIColor.systemGroupedBackground))
            .safeAreaInset(edge: .bottom) { inputBar }
            .navigationTitle("Restore")
            .navigationBarTitleDisplayMode(.large)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .disabled(isRestoring)
                }
                if restored {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") {
                            onRestoreComplete?()
                            dismiss()
                        }
                    }
                }
            }
            .interactiveDismissDisabled(isRestoring)
            .alert("Replace Current Identity?", isPresented: $showConfirmDialog) {
                Button("Cancel", role: .cancel) {}
                Button("Replace", role: .destructive) { restore() }
            } message: {
                Text("This will replace your current identity keys. If your current identity is not backed up, it will be permanently lost.")
            }
            .onAppear {
                inputFocused = true
            }
        }
    }

    private var slotGrid: some View {
        VStack(spacing: 0) {
            LazyVGrid(
                columns: [GridItem(.flexible(), spacing: 10), GridItem(.flexible())],
                spacing: 8
            ) {
                ForEach(0..<words.count, id: \.self) { i in
                    slotChip(i)
                }
            }
            .padding(14)
        }
        .background(
            Color(UIColor.secondarySystemGroupedBackground),
            in: RoundedRectangle(cornerRadius: 16, style: .continuous)
        )
    }

    private func slotChip(_ i: Int) -> some View {
        let active = activeSlot == i
        let filled = !words[i].isEmpty
        return Button {
            activeSlot = i
            currentInput = ""
            inputFocused = true
        } label: {
            HStack(spacing: 8) {
                Text("\(i + 1)")
                    .font(.caption2.monospaced())
                    .foregroundStyle(.tertiary)
                    .frame(width: 18, alignment: .trailing)
                    .monospacedDigit()
                Group {
                    if active && !filled {
                        HStack(spacing: 1) {
                            Text(currentInput)
                            Rectangle()
                                .fill(Color.accentColor)
                                .frame(width: 1.5, height: 14)
                                .opacity(0.9)
                        }
                    } else if filled {
                        Text(words[i])
                            .foregroundStyle(Color.primary)
                    } else {
                        Text("")
                    }
                }
                .font(.callout.weight(.medium))
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(
                active ? Color.accentColor.opacity(0.12) : Color.primary.opacity(0.04),
                in: RoundedRectangle(cornerRadius: 8, style: .continuous)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .strokeBorder(active ? Color.accentColor : Color.clear, lineWidth: 1.2)
            )
        }
        .buttonStyle(.plain)
    }

    private var suggestionChips: some View {
        HStack(spacing: 6) {
            ForEach(Array(suggestions.enumerated()), id: \.offset) { idx, word in
                Button {
                    acceptSuggestion(word)
                } label: {
                    Text(word)
                        .font(.callout.weight(.medium))
                        .foregroundStyle(idx == 0 ? Color.white : Color.primary)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .background(
                            idx == 0 ? Color.accentColor : Color(UIColor.systemGray5),
                            in: Capsule()
                        )
                }
                .buttonStyle(.plain)
            }
            Spacer(minLength: 0)
        }
    }

    private var inputBar: some View {
        HStack {
            TextField("Word \(activeSlot + 1)", text: $currentInput)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .font(.body.monospaced())
                .focused($inputFocused)
                .onSubmit { commitTopSuggestion() }
                .onChange(of: currentInput) { _, newValue in
                    if newValue.hasSuffix(" ") {
                        commitTopSuggestion()
                    }
                }
            if !currentInput.isEmpty {
                Button {
                    currentInput = ""
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.tertiary)
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(.thinMaterial)
    }

    private func acceptSuggestion(_ word: String) {
        guard activeSlot < words.count else { return }
        words[activeSlot] = word
        currentInput = ""
        advanceFocus()
    }

    private func commitTopSuggestion() {
        let trimmed = currentInput.trimmingCharacters(in: .whitespaces).lowercased()
        guard !trimmed.isEmpty else { return }
        if Bip39.isKnownWord(trimmed) {
            acceptSuggestion(trimmed)
        } else if let top = suggestions.first {
            acceptSuggestion(top)
        } else {
            errorMessage = "\"\(trimmed)\" is not in the BIP39 wordlist."
        }
    }

    private func advanceFocus() {
        if let next = (activeSlot + 1..<words.count).first(where: { words[$0].isEmpty }) {
            activeSlot = next
        } else if let firstEmpty = words.firstIndex(where: { $0.isEmpty }) {
            activeSlot = firstEmpty
        } else {
            activeSlot = words.count - 1
            inputFocused = false
        }
        errorMessage = nil
    }

    private func applyPaste(_ text: String) {
        let tokens = text
            .lowercased()
            .split(whereSeparator: { $0.isWhitespace || $0.isNewline })
            .map(String.init)
        guard tokens.count == words.count else {
            errorMessage = "Expected \(words.count) words, got \(tokens.count)."
            return
        }
        words = tokens
        currentInput = ""
        errorMessage = nil
        activeSlot = words.count - 1
        inputFocused = false
    }

    private func restore() {
        errorMessage = nil
        isRestoring = true
        let mnemonic = joinedPhrase

        Task.detached(priority: .userInitiated) {
            do {
                let newKeyManager = try KeyManager.restoreFromMnemonic(mnemonic)
                await MainActor.run {
                    appState.replaceKeyManager(newKeyManager)
                    restored = true
                    isRestoring = false
                }
            } catch {
                await MainActor.run {
                    errorMessage = error.localizedDescription
                    isRestoring = false
                }
            }
        }
    }
}
