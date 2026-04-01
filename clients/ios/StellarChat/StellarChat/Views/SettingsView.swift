import SwiftUI

struct SettingsView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var attestationStatus: String?

    var body: some View {
        NavigationStack {
            Form {
                Section("Nostr Identity (secp256k1)") {
                    LabeledContent("Public Key") {
                        Text(appState.keyManager.publicKeyHex.prefix(16) + "...")
                            .font(.caption)
                            .monospaced()
                    }

                    Button("Copy Nostr Public Key") {
                        UIPasteboard.general.string = appState.keyManager.publicKeyHex
                    }
                }

                Section("Group Membership (BLS12-381)") {
                    if let blsHex = try? appState.keyManager.blsPublicKey
                        .map({ String(format: "%02x", $0) }).joined()
                    {
                        LabeledContent("BLS Public Key") {
                            Text(blsHex.prefix(16) + "...")
                                .font(.caption)
                                .monospaced()
                        }
                    }

                    Button("Create Key Attestation") {
                        createAttestation()
                    }

                    if let status = attestationStatus {
                        Text(status)
                            .font(.caption)
                            .foregroundStyle(status.hasPrefix("Error") ? .red : .green)
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
                    LabeledContent("Signing") { Text("secp256k1 Schnorr") }
                    LabeledContent("ZK Backend") { Text("Groth16 / BLS12-381") }
                    LabeledContent("Commitment") { Text("Poseidon Merkle + SHA256") }
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

    private func createAttestation() {
        do {
            let attestation = try appState.keyManager.createAttestation()
            let blsHex = attestation.blsPubkey.prefix(8)
                .map { String(format: "%02x", $0) }.joined()
            let nostrHex = attestation.nostrPubkey.prefix(8)
                .map { String(format: "%02x", $0) }.joined()
            attestationStatus = "Bound BLS \(blsHex)... to Nostr \(nostrHex)..."
        } catch {
            attestationStatus = "Error: \(error.localizedDescription)"
        }
    }
}
