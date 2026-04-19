import SwiftUI
import UIKit

/// Sheet shown when the user taps "Invite" on a phonebook row.
/// Lets them pick between inviting to an existing group or starting a
/// personal 1:1 chat (onboarding link). Emits the Telegram deeplink
/// and records the InvitedContact locally.
struct TelegramInvitePickerView: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    let contact: PhoneContact

    @State private var selectedGroupID: String?
    @State private var mode: Mode = .onboard
    @State private var error: String?

    enum Mode: Hashable {
        case group
        case onboard
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Contact") {
                    LabeledContent("Name", value: contact.displayName)
                    LabeledContent("Phone", value: contact.phone)
                }

                Section("Invite type") {
                    Picker("Kind", selection: $mode) {
                        Text("Start personal chat").tag(Mode.onboard)
                        Text("Invite to existing group").tag(Mode.group)
                    }
                    .pickerStyle(.segmented)

                    if mode == .onboard {
                        Text("Share a 1:1 chat link. Your friend installs Onym and lands straight in a new encrypted chat with you.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    } else {
                        Text("Share an existing group's invite code. Your friend joins that group after install.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                if mode == .group {
                    Section("Group") {
                        if appState.groups.isEmpty {
                            Text("You have no groups yet. Create one first.")
                                .foregroundStyle(.secondary)
                        } else {
                            Picker("Select group", selection: $selectedGroupID) {
                                Text("Choose…").tag(String?.none)
                                ForEach(appState.groups, id: \.id) { group in
                                    Text(group.name).tag(Optional(group.id))
                                }
                            }
                        }
                    }
                }

                if let error {
                    Section {
                        Label(error, systemImage: "exclamationmark.triangle")
                            .foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle("Invite via Telegram")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Open Telegram") { openTelegram() }
                        .disabled(!canSubmit)
                }
            }
        }
    }

    private var canSubmit: Bool {
        switch mode {
        case .onboard: return true
        case .group: return selectedGroupID != nil
        }
    }

    private func openTelegram() {
        do {
            let (inviteURL, kind, nonce, groupID) = try buildInviteURL()
            let contactRecord = InvitedContact(
                phone: contact.phone,
                displayName: contact.displayName,
                inviteKind: kind,
                inviteURL: inviteURL,
                handshakeNonce: nonce,
                groupID: groupID,
                invitedAt: Date(),
                status: .pending,
                resolvedPubkey: nil
            )
            appState.invitedContactStore.upsert(contactRecord)

            TelegramInviteLauncher.open(
                url: inviteURL,
                phone: contact.phone,
                store: appState.invitedContactStore,
                onSuccess: { dismiss() },
                onFailure: { msg in error = msg }
            )
        } catch let err {
            error = err.localizedDescription
        }
    }

    private func buildInviteURL() throws -> (url: String, kind: InvitedContactKind, nonce: String?, groupID: String?) {
        switch mode {
        case .group:
            guard let groupID = selectedGroupID,
                  let group = appState.groups.first(where: { $0.id == groupID })
            else {
                throw NSError(domain: "TelegramInvite", code: 1, userInfo: [
                    NSLocalizedDescriptionKey: "Pick a group first."
                ])
            }
            let code = InviteCode(
                groupID: Data(hexString: group.id),
                groupSecret: group.groupSecret,
                name: group.name,
                relayHints: group.relayHints.map(\.absoluteString),
                members: group.members,
                epoch: group.epoch,
                salt: group.salt,
                commitment: group.commitment,
                tierRawValue: group.tier.rawValue
            ).encode()
            return ("https://onym.chat/join?code=\(code)", .group, nil, group.id)

        case .onboard:
            let nonce = Self.randomNonceHex()
            let inviterHex = appState.keyManager.keyAgreementPublicKeyHex
            return (
                "https://onym.chat/onboard?inviter=\(inviterHex)&nonce=\(nonce)",
                .onboard,
                nonce,
                nil
            )
        }
    }

    private static func randomNonceHex() -> String {
        var bytes = [UInt8](repeating: 0, count: 8)
        _ = SecRandomCopyBytes(kSecRandomDefault, 8, &bytes)
        return bytes.map { String(format: "%02x", $0) }.joined()
    }
}

/// Shared helper that hands off an Onym invite URL to Telegram via the share
/// flow (`tg://msg_url`) with an `https://t.me/share/url` web fallback.
/// On success, marks the matching InvitedContact as `.sent`.
@MainActor
enum TelegramInviteLauncher {
    static func open(
        url inviteURL: String,
        phone: String,
        store: InvitedContactStore,
        onSuccess: @MainActor @escaping () -> Void = {},
        onFailure: @MainActor @escaping (String) -> Void = { _ in }
    ) {
        // Telegram silently drops `text` in tg://resolve?phone=… links, so we use
        // the share flow (`tg://msg_url`) with `url=<link>&text=<caption>`.
        // `url` renders a clickable preview card; `text` becomes the caption.
        // Do NOT repeat the URL inside `text` — Telegram iOS renders both,
        // producing two visible links in the message.
        //
        // Use `.alphanumerics` (not `.urlQueryAllowed`) so that `&`, `=`, and `?`
        // inside the inner invite URL get percent-encoded. Otherwise the outer
        // tg:// parser splits on `&` and truncates our query params (nonce is
        // lost), breaking the onboard deeplink on the receiving side.
        let caption = "Join me on Onym — end-to-end encrypted chat."
        let encodedCaption = caption.addingPercentEncoding(
            withAllowedCharacters: .alphanumerics
        ) ?? ""
        let encodedURL = inviteURL.addingPercentEncoding(
            withAllowedCharacters: .alphanumerics
        ) ?? ""

        let tgURL = URL(string: "tg://msg_url?url=\(encodedURL)&text=\(encodedCaption)")!
        let webURL = URL(string: "https://t.me/share/url?url=\(encodedURL)&text=\(encodedCaption)")!

        UIApplication.shared.open(tgURL, options: [:]) { opened in
            Task { @MainActor in
                if opened {
                    store.setStatus(phone: phone, status: .sent)
                    onSuccess()
                } else {
                    UIApplication.shared.open(webURL, options: [:]) { webOpened in
                        Task { @MainActor in
                            if webOpened {
                                store.setStatus(phone: phone, status: .sent)
                                onSuccess()
                            } else {
                                onFailure("Couldn't open Telegram. Install the app or try another contact.")
                            }
                        }
                    }
                }
            }
        }
    }
}

private extension Data {
    init(hexString: String) {
        let bytes = stride(from: 0, to: hexString.count, by: 2).compactMap { i -> UInt8? in
            let start = hexString.index(hexString.startIndex, offsetBy: i)
            let end = hexString.index(start, offsetBy: 2)
            return UInt8(hexString[start..<end], radix: 16)
        }
        self.init(bytes)
    }
}
