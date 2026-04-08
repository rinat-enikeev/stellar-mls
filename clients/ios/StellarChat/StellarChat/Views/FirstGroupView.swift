import SwiftUI

struct FirstGroupView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var groupName = ""
    @State private var isCreating = false
    @State private var createdGroup: ChatGroup?
    @State private var inviteCode = ""
    @State private var errorMessage: String?
    @State private var showShareInvite = false

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("e.g. Family, Team Alpha, Book Club", text: $groupName)
                } footer: {
                    Text("You can invite people after the group is created.")
                }

                if isCreating {
                    Section {
                        HStack(spacing: 8) {
                            ProgressView()
                                .controlSize(.small)
                            Text("Creating group...")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
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
            .navigationTitle("Name your group")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .disabled(isCreating)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Create") { startCreation() }
                        .disabled(groupName.trimmingCharacters(in: .whitespaces).isEmpty || isCreating)
                }
            }
            .interactiveDismissDisabled(isCreating)
            .sheet(isPresented: $showShareInvite) {
                if let group = createdGroup {
                    ShareInviteView(group: group, inviteCode: inviteCode)
                }
            }
        }
    }

    private func startCreation() {
        let trimmed = groupName.trimmingCharacters(in: .whitespacesAndNewlines)
        let sanitized = trimmed.unicodeScalars.filter {
            !$0.properties.isDefaultIgnorableCodePoint &&
            ($0.value >= 0x20 || $0 == "\n" || $0 == "\t")
        }
        let name = String(String.UnicodeScalarView(sanitized))
        guard !name.isEmpty, name.count <= 100 else {
            errorMessage = "Group name must be 1-100 characters."
            return
        }

        errorMessage = nil
        isCreating = true

        Task {
            do {
                let (group, code) = try appState.createGroup(name: name)
                createdGroup = group
                inviteCode = code
                isCreating = false
                showShareInvite = true
            } catch {
                errorMessage = error.localizedDescription
                isCreating = false
            }
        }
    }
}
