import SwiftUI

struct JoinGroupView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var codeText = ""
    @State private var errorMessage: String?
    @State private var joined = false

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
                }

                if let error = errorMessage {
                    Section {
                        Label(error, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.red)
                    }
                }

                if joined {
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
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if joined {
                        Button("Done") { dismiss() }
                    } else {
                        Button("Join") { joinGroup() }
                            .disabled(codeText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
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

            let relayURLs = code.relayHints.compactMap(URL.init(string:))

            let group = ChatGroup(
                id: groupIDHex,
                name: code.name,
                groupSecret: code.groupSecret,
                createdAt: Date(),
                relayHints: relayURLs.isEmpty ? appState.relayURLs : relayURLs
            )
            appState.addGroup(group)
            joined = true
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
