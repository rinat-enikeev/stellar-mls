import SwiftUI

struct ShareInviteView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    let group: ChatGroup
    let inviteCode: String
    @State private var copied = false

    private var deepLink: String {
        "https://onym.chat/join?code=\(inviteCode)"
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 24) {
                Spacer()

                // Header
                VStack(spacing: 8) {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.system(size: 48))
                        .foregroundStyle(.green)
                    Text("Your group is ready")
                        .font(.title2)
                        .fontWeight(.bold)
                    Text("Invite someone so you can start chatting")
                        .font(.body)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                }

                // Share button
                ShareLink(item: deepLink) {
                    Label("Share invite link", systemImage: "square.and.arrow.up")
                        .fontWeight(.semibold)
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .padding(.horizontal, 32)

                // QR code
                VStack(spacing: 8) {
                    QRCodeView(inviteCode, size: 200)
                    Text("or scan this QR code in person")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                // Copy link
                Button {
                    UIPasteboard.general.string = deepLink
                    copied = true
                } label: {
                    Label(copied ? "Copied!" : "Copy invite link", systemImage: copied ? "checkmark" : "doc.on.doc")
                }
                .buttonStyle(.bordered)

                Spacer()

                // Skip
                Button("I'll do this later") {
                    appState.navigateToGroupID = group.id
                    dismiss()
                }
                .font(.caption)
                .foregroundStyle(.secondary)
                .padding(.bottom, 16)
            }
            .navigationTitle("Invite")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") {
                        appState.navigateToGroupID = group.id
                        dismiss()
                    }
                }
            }
        }
    }
}
