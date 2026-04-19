import SwiftUI

/// Shown after the app handles a `https://onym.chat/onboard?inviter=…&nonce=…`
/// deeplink. Creates a solo 1:1 group and publishes an invitation back to the
/// inviter so they can join. The invitation carries the original nonce so the
/// inviter's client can reconcile its InvitedContact row to "joined".
struct OnboardLandingView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    let inviterX25519Hex: String
    let nonceHex: String

    @State private var status: LoadStatus = .idle
    @State private var createdGroupID: String?

    enum LoadStatus: Equatable {
        case idle
        case working
        case done
        case failed(String)
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 24) {
                Spacer()

                Image(systemName: "person.2.wave.2.fill")
                    .font(.system(size: 64))
                    .foregroundStyle(.tint)

                VStack(spacing: 8) {
                    Text("Someone invited you to Onym")
                        .font(.title2)
                        .fontWeight(.bold)
                        .multilineTextAlignment(.center)
                    Text("Tap Accept to start an encrypted 1:1 chat with them. We'll send them your public key so they can join.")
                        .font(.body)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 32)
                }

                Spacer()

                switch status {
                case .idle:
                    Button {
                        Task { await accept() }
                    } label: {
                        Label("Accept", systemImage: "checkmark.circle.fill")
                            .fontWeight(.semibold)
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.large)
                    .padding(.horizontal, 32)

                case .working:
                    ProgressView("Setting up chat…")
                        .controlSize(.large)

                case .done:
                    Label("Invitation sent. Waiting for them to join…", systemImage: "checkmark.seal.fill")
                        .foregroundStyle(.green)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 32)

                case .failed(let message):
                    VStack(spacing: 12) {
                        Label(message, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.red)
                            .multilineTextAlignment(.center)
                            .padding(.horizontal, 32)
                        Button("Try again") { Task { await accept() } }
                            .buttonStyle(.bordered)
                    }
                }

                Spacer()
            }
            .navigationTitle("New Chat")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    if status != .working {
                        Button("Cancel") { dismiss() }
                    }
                }
                if status == .done {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") { dismiss() }
                    }
                }
            }
        }
    }

    private func accept() async {
        status = .working
        do {
            guard let recipientKeyData = Data(hexString: inviterX25519Hex),
                  recipientKeyData.count == 32
            else {
                throw NSError(domain: "Onboard", code: 1, userInfo: [
                    NSLocalizedDescriptionKey: "Invalid inviter key in link."
                ])
            }

            // Create a solo 1:1 group. The inviter will receive our invitation
            // via their Nostr inbox, decode the payload, and add themselves.
            let label = "Chat with \(inviterX25519Hex.prefix(8))"
            let (group, _) = try appState.createGroup(name: String(label))
            createdGroupID = group.id

            let payload = BootstrapPayload.from(
                group: group,
                senderPubkey: appState.keyManager.publicKeyHex,
                transportBundle: try? appState.keyManager.createTransportBundle(),
                onboardReplyNonce: nonceHex
            )

            await appState.invitationTransport.connect(to: appState.relayURLs)
            try await appState.invitationTransport.sendInvitation(
                payload: payload,
                recipientKeyAgreementPubkey: recipientKeyData,
                keyManager: appState.keyManager
            )
            status = .done
        } catch {
            status = .failed(error.localizedDescription)
        }
    }
}

private extension Data {
    init?(hexString: String) {
        var bytes: [UInt8] = []
        var i = hexString.startIndex
        while i < hexString.endIndex {
            let next = hexString.index(i, offsetBy: 2, limitedBy: hexString.endIndex) ?? hexString.endIndex
            let pair = String(hexString[i..<next])
            guard pair.count == 2, let byte = UInt8(pair, radix: 16) else { return nil }
            bytes.append(byte)
            i = next
        }
        self.init(bytes)
    }
}
