import SwiftUI
import SwiftMLS

struct GroupInfoView: View {
    let group: ChatGroup
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var memberToRemove: SEPGroupMemberLeaf?
    @State private var showRemoveConfirmation = false
    @State private var removalStatus: String?

    var body: some View {
        NavigationStack {
            List {
                Section("Group") {
                    LabeledContent("Name") { Text(group.name) }
                    LabeledContent("Epoch") { Text("\(group.epoch)") }
                    LabeledContent("Members") { Text("\(group.members.count)") }
                    LabeledContent("Tier") { Text(group.tier.displayName) }
                    if group.isPublishedOnChain {
                        LabeledContent("On-Chain") {
                            Image(systemName: "checkmark.circle.fill")
                                .foregroundStyle(.green)
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
                            }
                        }
                    }
                }

                Section {
                    Button {
                        appState.rotateGroupKey(groupID: group.id)
                        removalStatus = "Key rotated to epoch \(appState.groups.first(where: { $0.id == group.id })?.epoch ?? 0)."
                    } label: {
                        Text("Rotate Group Key")
                    }
                    Text("Generate a new encryption key without changing membership. Provides forward secrecy.")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }

                if let status = removalStatus {
                    Section {
                        Text(status)
                            .font(.caption)
                            .foregroundStyle(status.hasPrefix("Error") ? .red : .green)
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
        }
    }

    private func isMyKey(_ member: SEPGroupMemberLeaf) -> Bool {
        guard let myLeaf = try? appState.keyManager.memberLeaf else { return false }
        return member.publicKeyCompressed == myLeaf.publicKeyCompressed
    }

    private func removeMember(_ member: SEPGroupMemberLeaf) {
        appState.removeMember(blsPubkey: member.publicKeyCompressed, from: group.id)
        removalStatus = "Member removed. Key rotated to epoch \(appState.groups.first(where: { $0.id == group.id })?.epoch ?? 0)."
    }
}

private extension SEPTier {
    var displayName: String {
        switch self {
        case .small: return "Small (up to 8)"
        case .medium: return "Medium (up to 64)"
        case .large: return "Large (up to 2048)"
        @unknown default: return "Unknown"
        }
    }
}
