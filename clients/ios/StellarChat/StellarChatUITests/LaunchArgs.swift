import XCTest

/// Configures the app to behave like a fresh install for the journey — without
/// wiping the Room-equivalent CoreData/PersistenceStore. We only override the
/// onboarding-seen flag so the welcome sheet renders; everything else (real
/// relays, real Soroban, real keys) stays intact. The device may be the user's
/// daily driver and we do not want to clobber real groups.
///
/// The Android counterpart clears `has_seen_onboarding` and
/// `has_seen_first_group_welcome` from SharedPreferences. On iOS the same keys
/// live in `UserDefaults.standard` (see `@AppStorage("hasSeenOnboarding")` in
/// ContentView and `@AppStorage("hasSeenFirstGroupWelcome")` in ChatView).
///
/// **Why launchEnvironment, not launchArguments.** iOS UserDefaults has a
/// search-order in which `NSArgumentDomain` outranks the application's own
/// persistent domain. Passing `-hasSeenOnboarding NO` would set the value
/// only for the duration of the launch — and would *also* shadow the app's
/// own write of `true` once the user finishes onboarding, leaving the sheet
/// stuck open. Instead we set an environment variable that the AppDelegate
/// reads on `didFinishLaunchingWithOptions`, where it `removeObject`s the
/// onboarding keys *before* `@AppStorage` reads them on first SwiftUI render.
/// After that the keys behave normally — when the app later writes `true`,
/// the read returns `true` and the sheet dismisses.
enum LaunchArgs {
    static func configure(_ app: XCUIApplication) {
        app.launchEnvironment["RESET_ONBOARDING"] = "1"
        // Marker so the app code can detect "running under XCUITest" if it
        // ever needs to (e.g. to suppress non-deterministic UI like
        // promo banners). Currently no production code reads this — the
        // marker is here as a stable hook for future use.
        app.launchArguments.append("--xcuitest")
    }
}
