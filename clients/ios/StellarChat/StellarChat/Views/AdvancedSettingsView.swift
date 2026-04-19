import SwiftMLS
import SwiftUI

struct AdvancedSettingsView: View {
    @Environment(AppState.self) private var appState
    @State private var attestationStatus: String?

    var body: some View {
        List {
            Section {
                keyRow(label: "Public key (secp256k1)", value: appState.keyManager.publicKeyHex)
                Button("Copy Nostr public key") {
                    UIPasteboard.general.string = appState.keyManager.publicKeyHex
                }
            } header: {
                Text("Nostr identity")
            } footer: {
                Text("Your Nostr identity key used to sign events on relays.")
            }

            Section {
                keyRow(label: "Account ID (Ed25519)", value: appState.keyManager.stellarAccountID)
                Button("Copy Stellar Account ID") {
                    UIPasteboard.general.string = appState.keyManager.stellarAccountID
                }
            } header: {
                Text("Stellar identity")
            } footer: {
                Text("Derived from Nostr key via HKDF-SHA256. StrKey encoded (G…).")
            }

            Section {
                if let blsHex = try? appState.keyManager.blsPublicKey
                    .map({ String(format: "%02x", $0) }).joined()
                {
                    keyRow(label: "BLS public key (BLS12-381)", value: blsHex)
                }
                Button("Create key attestation") {
                    createAttestation()
                }
                if let status = attestationStatus {
                    Text(status)
                        .font(.caption)
                        .foregroundStyle(status.hasPrefix("Error") ? .red : .green)
                }
            } header: {
                Text("BLS / attestation")
            } footer: {
                Text("Ed25519 signature binding BLS group key to Stellar identity per SEP-XXXX §1.1.")
            }

            Section {
                protocolRow(label: "Invitation kind", value: "34113")
                protocolRow(label: "Message kind", value: "24114")
                protocolRow(label: "Encryption", value: "AES-256-GCM")
                protocolRow(label: "Invitation encryption", value: "X25519 ECDH + AES-GCM")
                protocolRow(label: "Key derivation", value: "HKDF-SHA256")
                protocolRow(label: "Topic derivation", value: "SHA256(secret)")
                protocolRow(label: "Nostr signing", value: "secp256k1 Schnorr")
                protocolRow(label: "Stellar signing", value: "Ed25519")
                protocolRow(label: "ZK backend", value: "Groth16 / BLS12-381")
                protocolRow(label: "Commitment", value: "Poseidon-only")
            } header: {
                Text("Protocol")
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Advanced")
        .navigationBarTitleDisplayMode(.large)
    }

    private func keyRow(label: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(label)
                .font(.callout)
            Spacer()
            Text(truncate(value))
                .font(.footnote)
                .monospaced()
                .foregroundStyle(.secondary)
        }
    }

    private func protocolRow(label: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline) {
            Text(label)
                .font(.callout)
            Spacer()
            Text(value)
                .font(.footnote)
                .monospaced()
                .foregroundStyle(.secondary)
        }
    }

    private func truncate(_ s: String, keep: Int = 14) -> String {
        guard s.count > keep else { return s }
        return String(s.prefix(keep)) + "…"
    }

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
}
