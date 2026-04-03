import SwiftMLS
import SwiftUI

struct JoinGroupView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var codeText = ""
    @State private var errorMessage: String?
    @State private var joined = false
    @State private var isSyncing = false
    @State private var showScanner = false
    @State private var clipboardDetected = false
    @State private var decodedGroup: ChatGroup?
    @State private var verificationResult: OnChainVerificationResult?
    @State private var isVerifying = false

    var body: some View {
        NavigationStack {
            Form {
                Section("Invite Code") {
                    TextEditor(text: $codeText)
                        .font(.caption)
                        .monospaced()
                        .frame(minHeight: 80)

                    Button("Paste from Clipboard") {
                        if let text = UIPasteboard.general.string {
                            codeText = text
                        }
                    }

                    Button {
                        showScanner = true
                    } label: {
                        Label("Scan QR Code", systemImage: "qrcode.viewfinder")
                    }
                }

                if let group = decodedGroup {
                    Section("Group Preview") {
                        LabeledContent("Name", value: group.name)
                        LabeledContent("Members", value: "\(group.members.count)")
                        LabeledContent("Epoch", value: "\(group.epoch)")

                        if let result = verificationResult {
                            VerificationBadgeView(result: result)
                            if result == .inactive {
                                Text("This group has been deactivated on-chain and cannot be joined.")
                                    .font(.caption)
                                    .foregroundStyle(.red)
                            }
                        } else if isVerifying {
                            HStack(spacing: 4) {
                                ProgressView()
                                    .controlSize(.mini)
                                Text("Verifying on-chain...")
                                    .font(.caption2)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }

                if let error = errorMessage {
                    Section {
                        Label(error, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.red)
                    }
                }

                if isSyncing {
                    Section {
                        HStack {
                            ProgressView()
                            Text("Syncing with group members…")
                                .foregroundStyle(.secondary)
                        }
                    }
                } else if joined {
                    Section {
                        Label("Joined successfully!", systemImage: "checkmark.circle")
                            .foregroundStyle(.green)
                    }
                }
            }
            .navigationTitle("Join Group")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    if !isSyncing {
                        Button("Cancel") { dismiss() }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if joined {
                        Button("Done") { dismiss() }
                    } else if decodedGroup != nil {
                        Button("Join") { confirmJoin() }
                            .disabled(isSyncing || verificationResult == .inactive)
                    } else {
                        Button("Preview") { decodeInvite() }
                            .disabled(isSyncing || codeText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                }
            }
            .interactiveDismissDisabled(isSyncing)
            .sheet(isPresented: $showScanner) {
                QRScannerView { scannedCode in
                    codeText = scannedCode
                    showScanner = false
                }
            }
            .onChange(of: codeText) { _, _ in
                decodedGroup = nil
                verificationResult = nil
                errorMessage = nil
            }
            .onAppear {
                guard codeText.isEmpty, !clipboardDetected,
                      let text = UIPasteboard.general.string?.trimmingCharacters(in: .whitespacesAndNewlines),
                      text.count > 100,
                      Data(base64Encoded: text) != nil
                else { return }
                clipboardDetected = true
                codeText = text
            }
        }
    }

    private func decodeInvite() {
        errorMessage = nil
        do {
            let code = try InviteCode.decode(from: codeText.trimmingCharacters(in: .whitespacesAndNewlines))
            let groupIDHex = code.groupID.map { String(format: "%02x", $0) }.joined()

            if appState.groups.contains(where: { $0.id == groupIDHex }) {
                errorMessage = "You're already in this group."
                return
            }

            let codeRelays = code.relayHints.compactMap(URL.init(string:))
            let mergedRelays = Array(Set(appState.relayURLs + codeRelays))

            let group = ChatGroup(
                id: groupIDHex,
                name: code.name,
                groupSecret: code.groupSecret,
                createdAt: Date(),
                relayHints: mergedRelays.isEmpty ? appState.relayURLs : mergedRelays,
                members: code.members,
                epoch: code.epoch,
                salt: code.salt,
                commitment: code.commitment,
                tier: SEPTier(rawValue: code.tierRawValue) ?? .large
            )
            decodedGroup = group

            Task { await verifyGroup() }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func verifyGroup() async {
        guard let group = decodedGroup else { return }
        guard appState.isContractConfigured else { return }
        isVerifying = true
        let result = await appState.verifyGroupOnChain(group)
        verificationResult = result
        isVerifying = false
    }

    private func confirmJoin() {
        guard var group = decodedGroup else { return }
        if verificationResult == .verified {
            group.isPublishedOnChain = true
        }
        // Add ourselves to the member list so our own messages pass BLS auth
        if let myLeaf = try? appState.keyManager.memberLeaf,
           !group.members.contains(where: { $0.publicKeyCompressed == myLeaf.publicKeyCompressed }) {
            group.members.append(myLeaf)
        }
        appState.addGroup(group)
        joined = true

        Task {
            await appState.announceMemberJoined(group: group)
        }
    }
}
