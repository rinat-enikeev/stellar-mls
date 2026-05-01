import Foundation

/// Stable identifiers for XCUITest. Used by `accessibilityIdentifier(...)` in
/// production views and by the matching constants in `StellarChatUITests`.
///
/// Keep this file in sync with `StellarChatUITests/TestIDs.swift` — we
/// duplicate the constants there because the UI test bundle does not link the
/// app target at runtime, so values must be hard-coded on both sides. If you
/// change a string here, change it there too.
enum TestIDs {
    enum Onboarding {
        // Note: a `sheet` identifier on the OnboardingView's content
        // cascaded down to all its descendant Buttons under SwiftUI iOS 26,
        // overriding `primary` and breaking XCUITest lookups. The fix is to
        // tag only the leaf control we actually need — the primary button.
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
        // The GroupInfo sheet's toolbar Close button. Without an explicit
        // identifier, label-matching `app.buttons["Close"]` collides with
        // SF-Symbol `xmark` dismiss buttons in ChatView's welcome and push
        // banners (iOS auto-labels those as "Close" for VoiceOver).
        static let close = "groupInfo.close"
    }
}
