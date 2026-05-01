import Foundation

/// Mirror of `StellarChat/TestIDs.swift`. The UI test bundle does not link the
/// app target at runtime, so the constants must be duplicated here. Keep them
/// in sync — if you change a value on one side, change it on the other.
enum TestIDs {
    enum Onboarding {
        // See note in StellarChat/TestIDs.swift — only the primary button
        // is tagged; the sheet container is not.
        static let primary = "onboarding.primary"
    }

    enum GroupList {
        static let screen = "groupList.screen"
        static let plusMenu = "groupList.plusMenu"
        static let pinnedIndicator = "groupList.pinnedIndicator"
    }

    enum Chat {
        static let messageInput = "chat.messageInput"
        static let send = "chat.send"
        static let groupInfo = "chat.groupInfo"
        static let statusSending = "chat.status.sending"
        static let statusSent = "chat.status.sent"
        static let statusDelivered = "chat.status.delivered"
        static let statusFailed = "chat.status.failed"
    }

    enum CreateGroup {
        static let openButton = "createGroup.open"
    }

    enum GroupInfo {
        static let close = "groupInfo.close"
    }
}
