import SwiftUI

/// Allows the user to restore identity from a BIP39 recovery phrase during onboarding.
struct RestoreIdentityView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var phraseInput = ""
    @State private var errorMessage: String?
    @State private var isRestoring = false
    @State private var restored = false
    @State private var showConfirmDialog = false
    var onRestoreComplete: (() -> Void)?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text("Enter your 12-word recovery phrase to restore your identity. All keys will be re-derived from this phrase.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }

                Section("Recovery Phrase") {
                    TextEditor(text: $phraseInput)
                        .font(.body)
                        .monospaced()
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .frame(minHeight: 100)

                    Button("Paste from Clipboard") {
                        if let text = UIPasteboard.general.string {
                            phraseInput = text.trimmingCharacters(in: .whitespacesAndNewlines)
                        }
                    }
                }

                if let error = errorMessage {
                    Section {
                        Label(error, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.red)
                    }
                }

                if restored {
                    Section {
                        Label("Identity restored successfully!", systemImage: "checkmark.circle")
                            .foregroundStyle(.green)
                    }
                }

                Section {
                    Button {
                        showConfirmDialog = true
                    } label: {
                        if isRestoring {
                            ProgressView()
                                .frame(maxWidth: .infinity)
                        } else {
                            Text("Restore Identity")
                                .fontWeight(.semibold)
                                .frame(maxWidth: .infinity)
                        }
                    }
                    .disabled(phraseInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isRestoring || restored)
                    .alert("Replace Current Identity?", isPresented: $showConfirmDialog) {
                        Button("Cancel", role: .cancel) {}
                        Button("Replace", role: .destructive) { restore() }
                    } message: {
                        Text("This will replace your current identity keys. If your current identity is not backed up, it will be permanently lost.")
                    }
                }
            }
            .navigationTitle("Restore Identity")
            .navigationBarTitleDisplayMode(.inline)
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
        }
    }

    private func restore() {
        errorMessage = nil
        isRestoring = true
        let mnemonic = phraseInput
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
            .split(separator: /\s+/)
            .joined(separator: " ")

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
