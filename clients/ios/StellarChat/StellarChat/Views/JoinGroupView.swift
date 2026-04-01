import SwiftUI

struct JoinGroupView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var codeText = ""
    @State private var errorMessage: String?
    @State private var joined = false
    @State private var isSyncing = false
    @State private var showScanner = false

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
                    } else {
                        Button("Join") { joinGroup() }
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
        }
    }

    private func joinGroup() {
        errorMessage = nil
        do {
            let code = try InviteCode.decode(from: codeText.trimmingCharacters(in: .whitespacesAndNewlines))
            let groupIDHex = code.groupID.map { String(format: "%02x", $0) }.joined()

            // Check if already joined
            if appState.groups.contains(where: { $0.id == groupIDHex }) {
                errorMessage = "You're already in this group."
                return
            }

            let codeRelays = code.relayHints.compactMap(URL.init(string:))
            // Merge invite code relays with user's relays (union) for maximum overlap
            let mergedRelays = Array(Set(appState.relayURLs + codeRelays))

            // Add ourselves to the member list so our own messages pass BLS auth
            var members = code.members
            let myLeaf = try appState.keyManager.memberLeaf
            if !members.contains(where: { $0.publicKeyCompressed == myLeaf.publicKeyCompressed }) {
                members.append(myLeaf)
            }

            let group = ChatGroup(
                id: groupIDHex,
                name: code.name,
                groupSecret: code.groupSecret,
                createdAt: Date(),
                relayHints: mergedRelays.isEmpty ? appState.relayURLs : mergedRelays,
                members: members,
                epoch: code.epoch,
                salt: code.salt,
                commitment: code.commitment
            )
            appState.addGroup(group)
            isSyncing = true

            // Announce ourselves to existing members so they add us
            Task {
                await appState.announceMemberJoined(group: group)
                isSyncing = false
                joined = true
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
