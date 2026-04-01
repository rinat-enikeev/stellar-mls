import SwiftUI

struct CreateGroupView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var groupName = ""
    @State private var inviteCode = ""
    @State private var copied = false

    var body: some View {
        NavigationStack {
            Form {
                Section("Group Name") {
                    TextField("e.g. Team Alpha", text: $groupName)
                }

                if !inviteCode.isEmpty {
                    Section("Invite Code") {
                        Text(inviteCode)
                            .font(.caption)
                            .monospaced()
                            .textSelection(.enabled)

                        Button {
                            UIPasteboard.general.string = inviteCode
                            copied = true
                        } label: {
                            Label(
                                copied ? "Copied!" : "Copy to Clipboard",
                                systemImage: copied ? "checkmark" : "doc.on.doc"
                            )
                        }
                    }

                    Section {
                        Text("Share this invite code with group members. Anyone with this code can join and read messages.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("Create Group")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if inviteCode.isEmpty {
                        Button("Create") { createGroup() }
                            .disabled(groupName.trimmingCharacters(in: .whitespaces).isEmpty)
                    } else {
                        Button("Done") { dismiss() }
                    }
                }
            }
        }
    }

    private func createGroup() {
        var groupIDBytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, 32, &groupIDBytes)
        let groupID = Data(groupIDBytes)

        var secretBytes = [UInt8](repeating: 0, count: 32)
        _ = SecRandomCopyBytes(kSecRandomDefault, 32, &secretBytes)
        let groupSecret = Data(secretBytes)

        let groupIDHex = groupID.map { String(format: "%02x", $0) }.joined()

        let group = ChatGroup(
            id: groupIDHex,
            name: groupName,
            groupSecret: groupSecret,
            createdAt: Date(),
            relayHints: appState.relayURLs
        )

        appState.addGroup(group)

        let code = InviteCode(
            groupID: groupID,
            groupSecret: groupSecret,
            name: groupName,
            relayHints: appState.relayURLs.map(\.absoluteString)
        )
        inviteCode = code.encode()
    }
}
