import XCTest

/// Default timeout for ordinary UI assertions — comfortable for any
/// frame-driven SwiftUI state change on a real device.
let DEFAULT_TIMEOUT: TimeInterval = 10

/// Long timeout for assertions that wait on real network / on-chain settlement.
/// Soroban testnet ledger close ranges from ~5s to ~30s in practice; allow
/// 2 minutes so transient relay backoff or testnet congestion doesn't ruin
/// a full-journey run.
let LONG_TIMEOUT: TimeInterval = 120

/// XCTest mirror of the Compose `awaitTag` / `awaitText` helpers used in the
/// Android suite. Each waiter blocks until the predicate is satisfied (or the
/// timeout elapses) and returns the matched element so the caller can chain
/// further actions.
extension XCUIApplication {

    @discardableResult
    func awaitElement(
        _ element: XCUIElement,
        timeout: TimeInterval = DEFAULT_TIMEOUT,
        file: StaticString = #file,
        line: UInt = #line
    ) -> XCUIElement {
        if !element.waitForExistence(timeout: timeout) {
            XCTFail(
                "element did not appear in \(timeout)s: \(element.debugDescription)",
                file: file,
                line: line
            )
        }
        return element
    }

    func awaitGone(
        _ element: XCUIElement,
        timeout: TimeInterval = DEFAULT_TIMEOUT,
        file: StaticString = #file,
        line: UInt = #line
    ) {
        let predicate = NSPredicate(format: "exists == false")
        let expectation = XCTNSPredicateExpectation(predicate: predicate, object: element)
        let result = XCTWaiter().wait(for: [expectation], timeout: timeout)
        if result != .completed {
            XCTFail(
                "element still present after \(timeout)s: \(element.debugDescription)",
                file: file,
                line: line
            )
        }
    }

    /// Wait for ANY of the given elements to exist. Returns the first one that
    /// becomes visible. Used for status-icon checks where a sent message may
    /// land on Sending / Sent / Delivered / Failed depending on relay state.
    @discardableResult
    func awaitAny(
        _ elements: [XCUIElement],
        timeout: TimeInterval = DEFAULT_TIMEOUT,
        file: StaticString = #file,
        line: UInt = #line
    ) -> XCUIElement? {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            for element in elements where element.exists {
                return element
            }
            Thread.sleep(forTimeInterval: 0.1)
        }
        XCTFail(
            "none of the candidate elements appeared in \(timeout)s",
            file: file,
            line: line
        )
        return nil
    }

    /// Tap the leading nav-bar back button (the standard SwiftUI back chevron
    /// uses the previous screen's title as its identifier; matching by
    /// `boundBy: 0` avoids hard-coding screen names).
    func tapBackArrow() {
        let backButton = navigationBars.buttons.element(boundBy: 0)
        if backButton.exists {
            backButton.tap()
        }
    }

    /// Locate a static text by case-insensitive label match. Useful for
    /// SwiftUI Section headers under `.listStyle(.insetGrouped)`, which the
    /// system renders in UPPERCASE visually but the underlying source string
    /// is mixed-case (e.g. `Text("Network")` displays as "NETWORK"). XCUITest
    /// has reported either case across iOS versions, so a case-insensitive
    /// predicate is the only stable lookup.
    func staticTextCaseInsensitive(_ label: String) -> XCUIElement {
        return staticTexts
            .matching(NSPredicate(format: "label ==[c] %@", label))
            .firstMatch
    }
}
