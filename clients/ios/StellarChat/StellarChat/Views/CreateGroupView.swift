import SwiftUI

struct CreateGroupView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var groupName = ""
    @State private var inviteCode = ""
    @State private var copied = false
    @State private var errorMessage: String?

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

                if let errorMessage {
                    Section {
                        Text(errorMessage)
                            .foregroundStyle(.red)
                            .font(.caption)
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
        do {
            let (_, code) = try appState.createGroup(name: groupName)
            inviteCode = code
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
