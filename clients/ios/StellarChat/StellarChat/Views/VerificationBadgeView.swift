import SwiftUI

struct VerificationBadgeView: View {
    let result: OnChainVerificationResult

    var body: some View {
        HStack(spacing: 4) {
            switch result {
            case .verified:
                Image(systemName: "checkmark.seal.fill")
                    .foregroundStyle(.green)
                Text("Verified on-chain")
                    .foregroundStyle(.green)
            case .notPublished:
                Image(systemName: "circle")
                    .foregroundStyle(.secondary)
                Text("Not yet published on-chain")
                    .foregroundStyle(.secondary)
            case .epochMismatch(let local, let onChain):
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                Text("Epoch mismatch: local \(local), on-chain \(onChain)")
                    .foregroundStyle(.orange)
            case .commitmentMismatch:
                Image(systemName: "xmark.seal.fill")
                    .foregroundStyle(.red)
                Text("Commitment mismatch")
                    .foregroundStyle(.red)
            case .inactive:
                Image(systemName: "minus.circle.fill")
                    .foregroundStyle(.red)
                Text("Group deactivated")
                    .foregroundStyle(.red)
            case .error:
                Image(systemName: "questionmark.circle")
                    .foregroundStyle(.secondary)
                Text("Could not verify")
                    .foregroundStyle(.secondary)
            }
        }
        .font(.caption2)
    }
}
