import XCTest

/// Captures the journey of a single XCUITest into a Markdown report. On a
/// pass, the report contains a header + chronological stage list with
/// elapsed-time offsets. On a failure, it adds the exception message and
/// description of the last stage we logged.
///
/// The report is delivered to the host two ways:
///  1. As an `XCTAttachment` with `.keepAlways` lifetime — visible in the
///     `.xcresult` bundle (`xcrun xcresulttool` can extract it).
///  2. As stdout with a `=== MARKDOWN REPORT BEGIN/END ===` delimiter — the
///     `scripts/run-solo-journey.sh` wrapper greps the xcodebuild log for the
///     block and saves it to `result_autotest/`. This is the simpler path and
///     the one the script defaults to; the XCAttachment is a fallback.
///
/// Mirrors the Android `MarkdownReportRule` (TestWatcher → MD on /sdcard).
final class MarkdownReporter {

    private struct StageEntry {
        let elapsedMs: Int64
        let text: String
    }

    private var stages: [StageEntry] = []
    private var startMs: Int64 = 0
    private var endMs: Int64 = 0
    private var fullName: String = ""
    private var failureMessage: String?

    init(testName: String) {
        self.fullName = testName
        self.startMs = Int64(Date().timeIntervalSince1970 * 1000)
    }

    func logStage(_ stage: String) {
        let elapsed = Int64(Date().timeIntervalSince1970 * 1000) - startMs
        stages.append(StageEntry(elapsedMs: elapsed, text: stage))
        // Echo to stdout as well — useful when watching `xcodebuild test`
        // output live without waiting for the final report.
        print("▶︎ \(stage)")
    }

    func recordFailure(_ message: String) {
        failureMessage = message
    }

    /// Finalise and emit the report. Call from `tearDown`. `passed` is true if
    /// the test method itself didn't throw — `failureMessage` is set
    /// independently when an XCTest failure was recorded.
    func emit(passed: Bool, testCase: XCTestCase) {
        endMs = Int64(Date().timeIntervalSince1970 * 1000)
        let durationS = Double(endMs - startMs) / 1000.0
        let isPass = passed && failureMessage == nil
        let statusTag = isPass ? "PASS" : "FAIL"

        var md = ""
        md += "# SoloUserJourneyUITest — \(statusTag)\n\n"
        md += "| Field | Value |\n"
        md += "|---|---|\n"
        md += "| Test | `\(fullName)` |\n"
        md += "| Status | **\(statusTag)** |\n"
        md += "| Started | \(isoUtc(startMs)) |\n"
        md += "| Finished | \(isoUtc(endMs)) |\n"
        md += "| Duration | \(String(format: "%.1f", durationS)) s |\n"
        md += "| Device | \(deviceDescription()) |\n"
        md += "| OS | \(osDescription()) |\n\n"

        md += "## Stage timeline (\(stages.count) entries)\n\n"
        md += "```\n"
        for entry in stages {
            md += String(
                format: "[+%7.3fs] %@\n",
                Double(entry.elapsedMs) / 1000.0,
                entry.text
            )
        }
        md += "```\n"

        if !isPass {
            md += "\n## Failure\n\n"
            let lastStage = stages.last?.text ?? "(no stages logged)"
            md += "- **Last stage before failure:** `\(lastStage)`\n"
            if let failureMessage {
                md += "- **Message:** \(failureMessage)\n"
            }
        }

        // Stdout delivery — bracketed for easy grep in the xcodebuild log.
        let timestamp = isoUtcCompact(startMs)
        let fileName = "solo-journey-\(timestamp)-\(statusTag).md"
        print("=== MARKDOWN REPORT BEGIN \(fileName) ===")
        print(md)
        print("=== MARKDOWN REPORT END \(fileName) ===")

        // XCTAttachment delivery — kept always so the .xcresult retains it
        // even on pass.
        let attachment = XCTAttachment(string: md)
        attachment.name = fileName
        attachment.lifetime = .keepAlways
        testCase.add(attachment)
    }

    private func isoUtc(_ ms: Int64) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss'Z'"
        formatter.timeZone = TimeZone(identifier: "UTC")
        formatter.locale = Locale(identifier: "en_US_POSIX")
        return formatter.string(from: Date(timeIntervalSince1970: TimeInterval(ms) / 1000.0))
    }

    private func isoUtcCompact(_ ms: Int64) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyyMMdd-HHmmss"
        formatter.timeZone = TimeZone(identifier: "UTC")
        formatter.locale = Locale(identifier: "en_US_POSIX")
        return formatter.string(from: Date(timeIntervalSince1970: TimeInterval(ms) / 1000.0))
    }

    private func deviceDescription() -> String {
        // ProcessInfo.hostName is a reasonable fallback when running on a
        // simulator; on a real device it returns the device name.
        ProcessInfo.processInfo.hostName
    }

    private func osDescription() -> String {
        let v = ProcessInfo.processInfo.operatingSystemVersion
        return "iOS \(v.majorVersion).\(v.minorVersion).\(v.patchVersion)"
    }
}
