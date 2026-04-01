import SwiftUI

struct SettingsView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section("Your Identity") {
                    LabeledContent("Public Key") {
                        Text(appState.keyManager.publicKeyHex.prefix(16) + "...")
                            .font(.caption)
                            .monospaced()
                    }

                    Button("Copy Full Public Key") {
                        UIPasteboard.general.string = appState.keyManager.publicKeyHex
                    }
                }

                Section("Relays") {
                    ForEach(appState.relayURLs, id: \.absoluteString) { url in
                        HStack {
                            Image(systemName: "antenna.radiowaves.left.and.right")
                                .foregroundStyle(.green)
                            Text(url.absoluteString)
                                .font(.caption)
                                .monospaced()
                        }
                    }
                }

                Section("Protocol") {
                    LabeledContent("Invitation Kind") { Text("24113") }
                    LabeledContent("Message Kind") { Text("24114") }
                    LabeledContent("Encryption") { Text("AES-256-GCM") }
                    LabeledContent("Key Derivation") { Text("HKDF-SHA256") }
                    LabeledContent("Topic Derivation") { Text("SHA256(secret)") }
                }

                Section("About") {
                    LabeledContent("Version") { Text("1.0.0") }
                    Text("Messages are encrypted end-to-end. Relays see only opaque ciphertext and hidden topic tags. Group IDs never appear in cleartext on the wire.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}
