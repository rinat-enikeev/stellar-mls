import SwiftUI

struct SettingsView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var attestationStatus: String?
    @State private var newRelayURL = ""
    @State private var relayError: String?

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

                Section("Inbox Key (X25519)") {
                    LabeledContent("Inbox Key") {
                        Text(appState.keyManager.keyAgreementPublicKeyHex.prefix(16) + "...")
                            .font(.caption)
                            .monospaced()
                    }

                    Button("Copy Inbox Key") {
                        UIPasteboard.general.string = appState.keyManager.keyAgreementPublicKeyHex
                    }

                    Text("Share this key with others so they can send you group invitations over Nostr.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }

                Section("Stellar Identity (Ed25519)") {
                    LabeledContent("Account ID") {
                        Text(appState.keyManager.stellarAccountID.prefix(16) + "...")
                            .font(.caption)
                            .monospaced()
                    }

                    Button("Copy Stellar Account ID") {
                        UIPasteboard.general.string = appState.keyManager.stellarAccountID
                    }

                    Text("Derived from Nostr key via HKDF-SHA256. StrKey encoded (G...).")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
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

                    Text("Ed25519 signature binding BLS group key to Stellar identity per SEP-XXXX §1.1.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }

                relayManagementSection

                Section("Protocol") {
                    LabeledContent("Invitation Kind") { Text("24113") }
                    LabeledContent("Message Kind") { Text("24114") }
                    LabeledContent("Encryption") { Text("AES-256-GCM") }
                    LabeledContent("Invitation Encryption") { Text("X25519 ECDH + AES-256-GCM") }
                    LabeledContent("Key Derivation") { Text("HKDF-SHA256") }
                    LabeledContent("Topic Derivation") { Text("SHA256(secret)") }
                    LabeledContent("Nostr Signing") { Text("secp256k1 Schnorr") }
                    LabeledContent("Stellar Signing") { Text("Ed25519") }
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

    // MARK: - Relay Management

    @ViewBuilder
    private var relayManagementSection: some View {
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
            .onDelete { offsets in
                appState.removeRelay(at: offsets)
            }
            .onMove { source, destination in
                appState.moveRelay(from: source, to: destination)
            }

            HStack {
                TextField("wss://relay.example.com", text: $newRelayURL)
                    .font(.caption)
                    .monospaced()
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)

                Button("Add") {
                    addRelay()
                }
                .disabled(newRelayURL.trimmingCharacters(in: .whitespaces).isEmpty)
            }

            if let error = relayError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }

            Text("Drag to reorder. Swipe to remove. First relay has highest priority.")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Actions

    private func createAttestation() {
        do {
            let attestation = try appState.keyManager.createAttestation()
            let valid = KeyManager.verifyAttestation(attestation)
            let blsHex = attestation.blsPubkey.prefix(8)
                .map { String(format: "%02x", $0) }.joined()
            let ed25519Hex = attestation.ed25519Pubkey.prefix(8)
                .map { String(format: "%02x", $0) }.joined()
            attestationStatus = "Bound BLS \(blsHex)... to Stellar \(ed25519Hex)... (\(valid ? "verified" : "INVALID"))"
        } catch {
            attestationStatus = "Error: \(error.localizedDescription)"
        }
    }

    private func addRelay() {
        relayError = nil
        let urlString = newRelayURL.trimmingCharacters(in: .whitespacesAndNewlines)
        if appState.addRelay(urlString: urlString) {
            newRelayURL = ""
        } else {
            relayError = "Invalid URL. Must be ws:// or wss:// and not already added."
        }
    }
}
