import SwiftUI

struct OnymShareKeyView: View {
    let identity: OnymIdentity
    @Environment(\.dismiss) private var dismiss

    private var url: String { onymInviteURL(for: identity) }

    var body: some View {
        OnymPage {
            VStack(alignment: .leading, spacing: 0) {
                OnymNavBar(title: "Invite Key", subtitle: identity.name, onBack: { dismiss() })

                VStack(spacing: 14) {
                    OnymQRCode(value: url, size: 260)
                        .padding(14)
                        .background(.white, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
                        .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous)
                            .stroke(.black.opacity(0.04), lineWidth: 1))
                        .shadow(color: .black.opacity(0.06), radius: 8, y: 2)

                    HStack(spacing: 8) {
                        OnymIdentityTile(active: identity.active, size: 28)
                        Text(identity.name)
                            .font(.system(size: 17, weight: .semibold))
                            .foregroundStyle(OnymTokens.text)
                    }
                    Text(identity.npub.prefix(22) + "…")
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundStyle(OnymTokens.text2)

                    Text(url)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundStyle(OnymTokens.text2)
                        .lineLimit(1).truncationMode(.middle)
                        .padding(.horizontal, 12).padding(.vertical, 8)
                        .background(OnymTokens.card2, in: RoundedRectangle(cornerRadius: 10))
                }
                .frame(maxWidth: .infinity)
                .padding(24)
                .background(OnymTokens.card, in: RoundedRectangle(cornerRadius: 22, style: .continuous))
                .padding(.horizontal, 16)
                .padding(.top, 8)

                HStack(spacing: 10) {
                    Button { UIPasteboard.general.string = url } label: {
                        HStack(spacing: 6) {
                            Image(systemName: "doc.on.doc")
                            Text("Copy link").font(.system(size: 15, weight: .semibold))
                        }
                        .foregroundStyle(OnymTokens.blue)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .background(Color(red: 0.863, green: 0.918, blue: 0.988),
                                    in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    }
                    .buttonStyle(.plain)
                    ShareLink(item: url) {
                        HStack(spacing: 6) {
                            Image(systemName: "square.and.arrow.up")
                            Text("Share").font(.system(size: 15, weight: .semibold))
                        }
                        .foregroundStyle(.white)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .background(OnymTokens.blue,
                                    in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, 16).padding(.top, 14)

                OnymFootnote(text: "Anyone who scans this code with Onym can start a private, end-to-end encrypted chat with \(identity.name). The invite key contains your Nostr public key (npub1…) only — no contact info.")
            }
        }
    }
}
