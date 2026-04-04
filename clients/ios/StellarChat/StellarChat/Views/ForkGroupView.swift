import SwiftUI
import SwiftMLS

struct ForkGroupView: View {
    let originalGroup: ChatGroup
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var forkName: String = ""
    @State private var excludedMembers: Set<Data> = []
    @State private var isCreating = false
    @State private var progressText: String?
    @State private var errorMessage: String?
    @State private var createdGroup: ChatGroup?

    private var forkSnapshot: EpochSnapshot? {
        guard let myLeaf = try? appState.keyManager.memberLeaf else { return nil }
        let myKey = myLeaf.publicKeyCompressed
        return appState.epochSnapshots[originalGroup.id]?.values
            .filter { snapshot in snapshot.members.contains { $0.publicKeyCompressed == myKey } }
            .max(by: { $0.epoch < $1.epoch })
    }

    var body: some View {
        NavigationStack {
            Form {
                if let snapshot = forkSnapshot {
                    Section {
                        LabeledContent("Fork from") { Text("Epoch \(snapshot.epoch)") }
                        LabeledContent("Members at epoch") { Text("\(snapshot.members.count)") }
                    } header: {
                        Text("Source")
                    } footer: {
                        Text("The fork starts from the last epoch where you were a member.")
                    }

                    Section("Fork Name") {
                        TextField("Group name", text: $forkName)
                    }

                    Section {
                        ForEach(snapshot.members, id: \.publicKeyCompressed) { member in
                            let isMe = isMyKey(member)
                            let isExcluded = excludedMembers.contains(member.publicKeyCompressed)
                            let pubkeyHex = member.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
                            let isRemover = pubkeyHex == originalGroup.removedByPubkeyHex
                            Button {
                                if !isMe {
                                    if isExcluded {
                                        excludedMembers.remove(member.publicKeyCompressed)
                                    } else {
                                        excludedMembers.insert(member.publicKeyCompressed)
                                    }
                                }
                            } label: {
                                HStack {
                                    VStack(alignment: .leading) {
                                        Text(pubkeyHex.prefix(16) + "...")
                                            .font(.caption)
                                            .monospaced()
                                        if isMe {
                                            Text("You")
                                                .font(.caption2)
                                                .foregroundStyle(.blue)
                                        }
                                        if isRemover {
                                            Text("Removed you")
                                                .font(.caption2)
                                                .foregroundStyle(.red)
                                        }
                                    }
                                    Spacer()
                                    if isMe {
                                        Image(systemName: "checkmark.circle.fill")
                                            .foregroundStyle(.blue)
                                    } else if isExcluded {
                                        Image(systemName: "xmark.circle.fill")
                                            .foregroundStyle(.red)
                                    } else {
                                        Image(systemName: "checkmark.circle.fill")
                                            .foregroundStyle(.green)
                                    }
                                }
                            }
                            .foregroundStyle(.primary)
                            .disabled(isMe)
                        }
                    } header: {
                        Text("Members")
                    } footer: {
                        let included = snapshot.members.count - excludedMembers.count
                        Text("Tap to exclude members. \(included) member\(included == 1 ? "" : "s") will be in the fork.")
                    }

                    if let errorMessage {
                        Section {
                            Text(errorMessage)
                                .font(.caption)
                                .foregroundStyle(.red)
                        }
                    }
                } else {
                    ContentUnavailableView(
                        "Cannot Fork",
                        systemImage: "exclamationmark.triangle",
                        description: Text("No epoch snapshot found where you were a member.")
                    )
                }
            }
            .navigationTitle("Fork Group")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if createdGroup != nil {
                        Button("Go to Fork") {
                            if let group = createdGroup {
                                appState.navigateToGroupID = group.id
                            }
                            dismiss()
                        }
                    } else {
                        Button {
                            createFork()
                        } label: {
                            if isCreating {
                                HStack(spacing: 4) {
                                    ProgressView()
                                    if let progressText {
                                        Text(progressText)
                                            .font(.caption2)
                                    }
                                }
                            } else {
                                Text("Create Fork")
                            }
                        }
                        .disabled(isCreating || forkSnapshot == nil)
                    }
                }
            }
            .onAppear {
                if forkName.isEmpty {
                    forkName = "Fork of \(originalGroup.name)"
                }
                // Pre-exclude the member who removed us
                if let removerHex = originalGroup.removedByPubkeyHex,
                   let snapshot = forkSnapshot,
                   let removerMember = snapshot.members.first(where: {
                       $0.publicKeyCompressed.map { String(format: "%02x", $0) }.joined() == removerHex
                   }) {
                    excludedMembers.insert(removerMember.publicKeyCompressed)
                }
            }
        }
    }

    private func isMyKey(_ member: SEPGroupMemberLeaf) -> Bool {
        guard let myLeaf = try? appState.keyManager.memberLeaf else { return false }
        return member.publicKeyCompressed == myLeaf.publicKeyCompressed
    }

    private func createFork() {
        isCreating = true
        errorMessage = nil
        progressText = nil
        Task {
            do {
                let group = try await appState.forkGroup(
                    originalGroupID: originalGroup.id,
                    excludingMembers: Array(excludedMembers),
                    forkName: forkName.trimmingCharacters(in: .whitespaces).isEmpty ? nil : forkName,
                    onProgress: { text in progressText = text }
                )
                createdGroup = group
            } catch {
                errorMessage = error.localizedDescription
            }
            isCreating = false
            progressText = nil
        }
    }
}
