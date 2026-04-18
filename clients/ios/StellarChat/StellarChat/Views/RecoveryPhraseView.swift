import LocalAuthentication
import SwiftUI

/// Displays the BIP39 recovery phrase behind biometric/passcode authentication.
/// The mnemonic screen is excluded from the app-switcher snapshot via redaction on background.
struct RecoveryPhraseView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var scenePhase

    @State private var authenticated = false
    @State private var authError: String?
    @State private var showCopyConfirm = false
    @State private var obscured = false

    var body: some View {
        NavigationStack {
            Group {
                if authenticated, let phrase = appState.keyManager.recoveryPhrase {
                    phraseContent(phrase)
                } else if let error = authError {
                    ContentUnavailableView {
                        Label("Authentication Failed", systemImage: "lock.slash")
                    } description: {
                        Text(error)
                    } actions: {
                        Button("Try Again") { authenticate() }
                    }
                } else {
                    ContentUnavailableView {
                        Label("Authenticating...", systemImage: "faceid")
                    } description: {
                        Text("Verify your identity to view the recovery phrase.")
                    }
                }
            }
            .navigationTitle("Recovery Phrase")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .task { authenticate() }
            // Hide recovery phrase when app is backgrounded (app-switcher privacy)
            .overlay {
                if obscured {
                    Color(.systemBackground)
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
    private func phraseContent(_ phrase: String) -> some View {
        let words = phrase.split(separator: " ").map(String.init)
        Form {
            Section {
                Label {
                    Text("Write down these words in order. Store them securely offline. Anyone with this phrase can access your identity.")
                        .font(.callout)
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                }
            }

            Section("Recovery Phrase") {
                LazyVGrid(columns: [
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                ], spacing: 12) {
                    ForEach(Array(words.enumerated()), id: \.offset) { index, word in
                        HStack(spacing: 4) {
                            Text("\(index + 1).")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                                .frame(width: 20, alignment: .trailing)
                            Text(word)
                                .font(.body)
                                .fontWeight(.medium)
                                .monospaced()
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(.vertical, 8)
            }

            Section {
                Button {
                    UIPasteboard.general.string = phrase
                    showCopyConfirm = true
                    // Clear clipboard after 60 seconds
                    DispatchQueue.main.asyncAfter(deadline: .now() + 60) {
                        if UIPasteboard.general.string == phrase {
                            UIPasteboard.general.string = ""
                        }
                    }
                } label: {
                    Label("Copy to Clipboard", systemImage: "doc.on.doc")
                }
                .alert("Copied", isPresented: $showCopyConfirm) {
                    Button("OK", role: .cancel) {}
                } message: {
                    Text("Recovery phrase copied. It will be cleared from clipboard in 60 seconds. Store it securely now.")
                }
            } footer: {
                Text("Never share your recovery phrase. Never enter it on a website. Stellar Chat will never ask for it outside of the restore flow.")
            }
        }
    }

    private func authenticate() {
        let context = LAContext()
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error) else {
            // No biometric/passcode — allow access (simulator or no lock set)
            authenticated = true
            return
        }
        context.evaluatePolicy(
            .deviceOwnerAuthentication,
            localizedReason: "Authenticate to view your recovery phrase"
        ) { success, evalError in
            DispatchQueue.main.async {
                if success {
                    authenticated = true
                } else {
                    authError = evalError?.localizedDescription ?? "Authentication failed"
                }
            }
        }
    }
}
