package uitests

import android.os.Build
import android.util.Log
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Captures the journey of a single instrumented test into a Markdown report
 * written to the app's external files dir. The wrapper script
 * `scripts/run-solo-journey.sh` `adb pull`s these into
 * `clients/android/StellarChat/result_autotest/` after the run.
 *
 * On success: header + chronological stage list with elapsed-time offsets.
 * On failure: above + exception class, message, stack trace, and a tail
 * of `logcat` filtered to test-relevant tags.
 *
 * Reports land in: `/sdcard/Android/data/chat.onym.android/files/autotest-reports/`
 */
class MarkdownReportRule : TestWatcher() {

    private data class StageEntry(val elapsedMs: Long, val text: String)

    private val stages = mutableListOf<StageEntry>()
    private var startMs: Long = 0L
    private var endMs: Long = 0L
    private var fullName: String = ""
    private var failure: Throwable? = null

    /** Called by the test on every `log()` invocation; records elapsed time. */
    fun logStage(stage: String) {
        val elapsed = if (startMs == 0L) 0L else System.currentTimeMillis() - startMs
        stages.add(StageEntry(elapsed, stage))
    }

    override fun starting(description: Description) {
        stages.clear()
        failure = null
        startMs = System.currentTimeMillis()
        fullName = "${description.className}.${description.methodName}"
    }

    override fun succeeded(description: Description) {
        endMs = System.currentTimeMillis()
        writeReport(passed = true)
    }

    override fun failed(e: Throwable, description: Description) {
        endMs = System.currentTimeMillis()
        failure = e
        writeReport(passed = false)
    }

    private fun writeReport(passed: Boolean) {
        val ctx = InstrumentationRegistry.getInstrumentation().targetContext
        val outDir = ctx.getExternalFilesDir(REPORT_DIR_NAME)
        if (outDir == null) {
            Log.w(TAG, "external files dir unavailable — skipping report")
            return
        }
        outDir.mkdirs()

        val fileTimestamp = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.ROOT).format(Date(startMs))
        val statusTag = if (passed) "PASS" else "FAIL"
        val outFile = File(outDir, "solo-journey-$fileTimestamp-$statusTag.md")

        val durationS = (endMs - startMs) / 1000.0

        val md = buildString {
            appendLine("# SoloUserJourneyTest — $statusTag")
            appendLine()
            appendLine("| Field | Value |")
            appendLine("|---|---|")
            appendLine("| Test | `$fullName` |")
            appendLine("| Status | **$statusTag** |")
            appendLine("| Started | ${isoUtc(startMs)} |")
            appendLine("| Finished | ${isoUtc(endMs)} |")
            appendLine("| Duration | ${"%.1f".format(durationS)} s |")
            appendLine("| Device | ${Build.MANUFACTURER} ${Build.MODEL} |")
            appendLine("| Android | ${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT}) |")
            appendLine("| Build fingerprint | `${Build.FINGERPRINT}` |")
            appendLine()
            appendLine("## Stage timeline (${stages.size} entries)")
            appendLine()
            appendLine("```")
            stages.forEach { e ->
                appendLine("[+%7.3fs] %s".format(e.elapsedMs / 1000.0, e.text))
            }
            appendLine("```")

            if (!passed && failure != null) {
                appendLine()
                appendLine("## Failure")
                appendLine()
                val lastStage = stages.lastOrNull()?.text ?: "(no stages logged)"
                appendLine("- **Last stage before failure:** `$lastStage`")
                appendLine("- **Exception:** `${failure!!.javaClass.name}`")
                val msg = failure!!.message ?: ""
                if (msg.isNotEmpty()) {
                    appendLine("- **Message:** $msg")
                }
                appendLine()
                appendLine("### Stack trace")
                appendLine()
                appendLine("```")
                appendLine(failure!!.stackTraceToString().trim())
                appendLine("```")
                appendLine()
                appendLine("### Logcat tail (filtered)")
                appendLine()
                appendLine("```")
                append(captureLogcat())
                appendLine("```")
            }
        }

        runCatching { outFile.writeText(md) }
            .onSuccess { Log.i(TAG, "wrote ${outFile.absolutePath}") }
            .onFailure { Log.e(TAG, "failed to write report: ${it.message}") }
    }

    private fun captureLogcat(): String = runCatching {
        val proc = Runtime.getRuntime().exec(
            arrayOf(
                "logcat", "-d", "-v", "time", "-t", "5000",
                "SoloUserJourneyTest:I", "TestRunner:I", "AndroidRuntime:E",
                "MarkdownReportRule:I", "GroupListVM:I", "*:S"
            )
        )
        proc.inputStream.bufferedReader().use { it.readText() }
    }.getOrElse { "(failed to capture logcat: ${it.message})\n" }

    private fun isoUtc(ms: Long): String {
        val fmt = SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss'Z'", Locale.ROOT)
        fmt.timeZone = java.util.TimeZone.getTimeZone("UTC")
        return fmt.format(Date(ms))
    }

    companion object {
        private const val TAG = "MarkdownReportRule"
        const val REPORT_DIR_NAME = "autotest-reports"
    }
}
