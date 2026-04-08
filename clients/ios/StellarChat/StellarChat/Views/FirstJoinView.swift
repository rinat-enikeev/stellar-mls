import SwiftMLS
import SwiftUI

struct FirstJoinView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var codeText = ""
    @State private var showScanner = false
    @State private var errorMessage: String?
    @State private var decodedGroup: ChatGroup?
    @State private var verificationResult: OnChainVerificationResult?
    @State private var isVerifying = false
    @State private var isSyncing = false
    @State private var joined = false

    var body: some View {
        NavigationStack {
            if let group = decodedGroup {
                // Screen K: Preview & Join
                previewView(group: group)
            } else {
                // Screen J: Scan or Paste
                scanPasteView
            }
        }
    }

    // MARK: - Scan / Paste View

    private var scanPasteView: some View {
        VStack(spacing: 24) {
            Spacer()

            // QR scan (primary action)
            Button { showScanner = true } label: {
                VStack(spacing: 12) {
                    Image(systemName: "qrcode.viewfinder")
                        .font(.system(size: 56))
                        .foregroundStyle(.tint)
                    Text("Scan QR code")
                        .font(.headline)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 32)
                .background(Color.accentColor.opacity(0.05), in: RoundedRectangle(cornerRadius: 16))
            }
            .buttonStyle(.plain)
            .padding(.horizontal, 32)

            // Divider
            HStack {
                Rectangle().fill(.secondary.opacity(0.3)).frame(height: 1)
                Text("or paste invite code")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Rectangle().fill(.secondary.opacity(0.3)).frame(height: 1)
            }
            .padding(.horizontal, 32)

            // Paste section
            VStack(spacing: 8) {
                TextField("Paste invite code here", text: $codeText)
                    .font(.caption)
                    .monospaced()
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                    .padding(12)
                    .background(Color(.systemGray6), in: RoundedRectangle(cornerRadius: 12))
                Button("Paste from Clipboard") {
                    if let text = UIPasteboard.general.string {
                        codeText = text.trimmingCharacters(in: .whitespacesAndNewlines)
                    }
                }
                .font(.caption)
            }
            .padding(.horizontal, 32)

            if let errorMessage {
                Text(errorMessage)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .padding(.horizontal, 32)
            }

            Spacer()

            Text("Ask a friend to share their group's invite link or QR code")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
                .padding(.bottom, 16)
        }
        .navigationTitle("Join a group")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") { dismiss() }
            }
        }
        .sheet(isPresented: $showScanner) {
            QRScannerView { scannedCode in
                codeText = scannedCode
                showScanner = false
            }
        }
        .onChange(of: codeText) { _, newValue in
            errorMessage = nil
            let trimmed = newValue.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { return }
            decodeInvite(trimmed)
        }
    }

    // MARK: - Preview & Join View

    private func previewView(group: ChatGroup) -> some View {
        VStack(spacing: 20) {
            Spacer()

            Image(systemName: "person.3.fill")
                .font(.system(size: 40))
                .foregroundStyle(.tint)
            Text(group.name)
                .font(.title2)
                .fontWeight(.bold)
            Text("\(group.members.count) members · Epoch \(group.epoch)")
                .font(.caption)
                .foregroundStyle(.secondary)

            if let result = verificationResult {
                VerificationBadgeView(result: result)
            } else if isVerifying {
                HStack(spacing: 4) {
                    ProgressView().controlSize(.mini)
                    Text("Verifying on-chain...")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }

            Spacer()

            if isSyncing {
                ProgressView("Joining...")
            } else if joined {
                Label("You're in!", systemImage: "checkmark.circle.fill")
                    .font(.headline)
                    .foregroundStyle(.green)
            } else {
                Button { confirmJoin() } label: {
                    Text("Join this group")
                        .fontWeight(.semibold)
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .padding(.horizontal, 32)
                .disabled(verificationResult == .inactive)
            }

            Spacer().frame(height: 32)
        }
        .navigationTitle("Join a group")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                if !isSyncing {
                    Button("Back") {
                        decodedGroup = nil
                        codeText = ""
                    }
                }
            }
            if joined {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    // MARK: - Logic

    private func decodeInvite(_ code: String) {
        do {
            let invite = try InviteCode.decode(from: code)
            let groupIDHex = invite.groupID.map { String(format: "%02x", $0) }.joined()

            if appState.groups.contains(where: { $0.id == groupIDHex }) {
                errorMessage = "You're already in this group."
                return
            }

            let codeRelays = invite.relayHints.compactMap(URL.init(string:))
            let mergedRelays = Array(Set(appState.relayURLs + codeRelays))

            let group = ChatGroup(
                id: groupIDHex,
                name: invite.name,
                groupSecret: invite.groupSecret,
                createdAt: Date(),
                relayHints: mergedRelays.isEmpty ? appState.relayURLs : mergedRelays,
                members: invite.members,
                epoch: invite.epoch,
                salt: invite.salt,
                commitment: invite.commitment,
                tier: SEPTier(rawValue: invite.tierRawValue) ?? .large
            )
            decodedGroup = group

            Task { await verifyGroup(group) }
        } catch {
            // Not a valid code yet — user may still be typing/pasting
        }
    }

    private func verifyGroup(_ group: ChatGroup) async {
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
        if let myLeaf = try? appState.keyManager.memberLeaf,
           !group.members.contains(where: { $0.publicKeyCompressed == myLeaf.publicKeyCompressed }) {
            group.members.append(myLeaf)
        }
        appState.addGroup(group)
        joined = true
        appState.navigateToGroupID = group.id

        Task {
            await appState.announceMemberJoined(group: group)
        }
    }
}
