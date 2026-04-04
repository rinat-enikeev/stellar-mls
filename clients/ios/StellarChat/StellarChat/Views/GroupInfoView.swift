import SwiftUI
import SwiftMLS

struct GroupInfoView: View {
    let group: ChatGroup
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var memberToRemove: SEPGroupMemberLeaf?
    @State private var showRemoveConfirmation = false
    @State private var removalStatus: String?
    @State private var removalStatusIsError = false
    @State private var isRemovingMember = false
    @State private var showRenameAlert = false
    @State private var newGroupName = ""

    var body: some View {
        NavigationStack {
            List {
                Section("Group") {
                    Button {
                        newGroupName = group.name
                        showRenameAlert = true
                    } label: {
                        LabeledContent("Name") {
                            HStack(spacing: 4) {
                                Text(group.name)
                                Image(systemName: "pencil")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                    .foregroundStyle(.primary)
                    LabeledContent("Epoch") { Text("\(group.epoch)") }
                    LabeledContent("Members") { Text("\(group.members.count)") }
                    LabeledContent("Tier") { Text(group.tier.displayName) }
                    if group.isPublishedOnChain {
                        let removed = !appState.isMember(of: group)
                        LabeledContent("On-Chain") {
                            if removed {
                                HStack(spacing: 4) {
                                    Image(systemName: "exclamationmark.triangle.fill")
                                        .foregroundStyle(.orange)
                                    Text("Diverged")
                                        .font(.caption)
                                        .foregroundStyle(.orange)
                                }
                            } else {
                                Image(systemName: "checkmark.circle.fill")
                                    .foregroundStyle(.green)
                            }
                        }
                    }
                }

                Section("Members") {
                    ForEach(group.members, id: \.publicKeyCompressed) { member in
                        HStack {
                            VStack(alignment: .leading) {
                                let pubkeyHex = member.publicKeyCompressed.map { String(format: "%02x", $0) }.joined()
                                Text(pubkeyHex.prefix(16) + "...")
                                    .font(.caption)
                                    .monospaced()

                                if isMyKey(member) {
                                    Text("You")
                                        .font(.caption2)
                                        .foregroundStyle(.blue)
                                }
                            }

                            Spacer()

                            if !isMyKey(member) {
                                Button(role: .destructive) {
                                    memberToRemove = member
                                    showRemoveConfirmation = true
                                } label: {
                                    Image(systemName: "person.badge.minus")
                                }
                                .disabled(isRemovingMember)
                            }
                        }
                    }
                }

                // Epoch History (epoch branching)
                if let snapshots = appState.epochSnapshots[group.id], !snapshots.isEmpty {
                    Section {
                        let currentGroup = appState.groups.first(where: { $0.id == group.id })
                        let pinnedEpoch = currentGroup?.pinnedEpoch
                        let currentSecret = currentGroup?.groupSecret
                        let sorted = snapshots.values.sorted { $0.epoch > $1.epoch }

                        if pinnedEpoch != nil {
                            Button {
                                appState.unpinEpoch(groupID: group.id)
                            } label: {
                                Label("Follow Latest Epoch", systemImage: "arrow.uturn.forward")
                            }
                        }

                        ForEach(sorted, id: \.epoch) { snapshot in
                            let isRekeyBoundary = currentSecret != nil && snapshot.groupSecret != currentSecret!
                            Button {
                                if pinnedEpoch == snapshot.epoch {
                                    appState.unpinEpoch(groupID: group.id)
                                } else {
                                    appState.pinEpoch(groupID: group.id, epoch: snapshot.epoch)
                                }
                            } label: {
                                HStack {
                                    Image(systemName: isRekeyBoundary ? "lock.shield" : "circle")
                                        .font(.caption)
                                        .foregroundStyle(isRekeyBoundary ? .green : .secondary)
                                        .frame(width: 20)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text("Epoch \(snapshot.epoch)")
                                            .font(.subheadline)
                                            .fontWeight(.medium)
                                        Text(snapshot.changeDescription)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                        HStack(spacing: 4) {
                                            Text("\(snapshot.members.count) members")
                                            if isRekeyBoundary {
                                                Text("- private branch")
                                                    .foregroundStyle(.green)
                                            }
                                        }
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                    }
                                    Spacer()
                                    if pinnedEpoch == snapshot.epoch {
                                        Image(systemName: "checkmark.circle.fill")
                                            .foregroundStyle(.blue)
                                    }
                                }
                            }
                            .foregroundStyle(.primary)
                        }
                    } header: {
                        Text("Epoch History")
                    } footer: {
                        Text("Switch to a previous epoch to communicate with members who were present at that point. Epochs marked with \(Image(systemName: "lock.shield")) use a different encryption key — only members from that epoch can read messages there.")
                    }
                }

                Section {
                    Button {
                        Task {
                            await appState.rotateGroupKey(groupID: group.id)
                            removalStatus = "Key rotated to epoch \(appState.groups.first(where: { $0.id == group.id })?.epoch ?? 0)."
                            removalStatusIsError = false
                        }
                    } label: {
                        Text("Rotate Group Key")
                    }
                    Text("Generate a new encryption key without changing membership. Provides forward secrecy.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }

                if isRemovingMember {
                    Section {
                        HStack(spacing: 8) {
                            ProgressView()
                            Text("Removing member and updating on-chain state...")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }

                if let status = removalStatus {
                    Section {
                        Text(status)
                            .font(.caption)
                            .foregroundStyle(removalStatusIsError ? .red : .green)
                    }
                }
            }
            .navigationTitle("Group Info")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .confirmationDialog(
                "Remove this member?",
                isPresented: $showRemoveConfirmation,
                titleVisibility: .visible
            ) {
                Button("Remove", role: .destructive) {
                    guard let member = memberToRemove else { return }
                    removeMember(member)
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("The member will be removed and the group key will be rotated. They will not be able to decrypt future messages.")
            }
            .alert("Rename Group", isPresented: $showRenameAlert) {
                TextField("Group name", text: $newGroupName)
                Button("Rename") {
                    appState.renameGroup(groupID: group.id, newName: newGroupName)
                    removalStatus = "Group renamed."
                    removalStatusIsError = false
                }
                Button("Cancel", role: .cancel) {}
            }
        }
    }

    private func isMyKey(_ member: SEPGroupMemberLeaf) -> Bool {
        guard let myLeaf = try? appState.keyManager.memberLeaf else { return false }
        return member.publicKeyCompressed == myLeaf.publicKeyCompressed
    }

    private func removeMember(_ member: SEPGroupMemberLeaf) {
        isRemovingMember = true
        removalStatus = nil
        Task {
            do {
                try await appState.removeMember(blsPubkey: member.publicKeyCompressed, from: group.id)
                let epoch = appState.groups.first(where: { $0.id == group.id })?.epoch ?? 0
                removalStatus = "Member removed. Key rotated to epoch \(epoch)."
                removalStatusIsError = false
            } catch {
                removalStatus = error.localizedDescription
                removalStatusIsError = true
            }
            isRemovingMember = false
        }
    }
}

private extension SEPTier {
    var displayName: String {
        switch self {
        case .small: return "Small (up to 32)"
        case .medium: return "Medium (up to 256)"
        case .large: return "Large (up to 2,048)"
        @unknown default: return "Unknown"
        }
    }
}
