import SwiftUI

struct InviteMemberView: View {
    let group: ChatGroup
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var recipientInboxKey = ""
    @State private var status: String?
    @State private var isSending = false
    @State private var showScanner = false

    var body: some View {
        NavigationStack {
            Form {
                Section("Group") {
                    LabeledContent("Name") { Text(group.name) }
                    LabeledContent("Members") { Text("\(group.members.count)") }
                }

                Section("Recipient's Inbox Key") {
                    TextField("X25519 public key (hex)", text: $recipientInboxKey)
                        .font(.caption)
                        .monospaced()
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)

                    Button("Paste from Clipboard") {
                        if let text = UIPasteboard.general.string {
                            recipientInboxKey = text.trimmingCharacters(in: .whitespacesAndNewlines)
                        }
                    }

                    Button {
                        showScanner = true
                    } label: {
                        Label("Scan Inbox Key QR", systemImage: "qrcode.viewfinder")
                    }

                    Text("The recipient can find their inbox key QR code in Settings → Advanced.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }

                if let status {
                    Section {
                        Text(status)
                            .font(.caption)
                            .foregroundStyle(status.hasPrefix("Error") ? .red : .green)
                    }
                }
            }
            .navigationTitle("Invite via Nostr")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if status?.hasPrefix("Sent") == true {
                        Button("Done") { dismiss() }
                    } else {
                        Button("Send") { sendInvitation() }
                            .disabled(recipientInboxKey.count != 64 || isSending)
                    }
                }
            }
            .sheet(isPresented: $showScanner) {
                QRScannerView { scannedCode in
                    let trimmed = scannedCode.trimmingCharacters(in: .whitespacesAndNewlines)
                    if trimmed.count == 64, trimmed.allSatisfy({ $0.isHexDigit }) {
                        recipientInboxKey = trimmed
                    }
                    showScanner = false
                }
            }
        }
    }

    private func sendInvitation() {
        guard let recipientPubkeyData = hexToData(recipientInboxKey), recipientPubkeyData.count == 32 else {
            status = "Error: Invalid inbox key. Must be 64 hex characters (32 bytes)."
            return
        }

        isSending = true
        status = nil

        Task {
            do {
                let payload = BootstrapPayload.from(
                    group: group,
                    senderPubkey: appState.keyManager.publicKeyHex
                )

                await appState.invitationTransport.connect(to: appState.relayURLs)

                try await appState.invitationTransport.sendInvitation(
                    payload: payload,
                    recipientKeyAgreementPubkey: recipientPubkeyData,
                    keyManager: appState.keyManager
                )

                status = "Sent! The recipient will see this in their pending invitations."
            } catch {
                status = "Error: \(error.localizedDescription)"
            }
            isSending = false
        }
    }

    private func hexToData(_ hex: String) -> Data? {
        guard hex.count % 2 == 0 else { return nil }
        let bytes = stride(from: 0, to: hex.count, by: 2).compactMap { i -> UInt8? in
            let start = hex.index(hex.startIndex, offsetBy: i)
            let end = hex.index(start, offsetBy: 2)
            return UInt8(hex[start..<end], radix: 16)
        }
        guard bytes.count == hex.count / 2 else { return nil }
        return Data(bytes)
    }
}
