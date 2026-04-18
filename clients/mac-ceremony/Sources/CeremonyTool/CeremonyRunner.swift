import Foundation

struct CeremonyRunnerError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

struct CeremonyRunner {
    /// Orchestrates one contribute invocation:
    /// 1. unzip (or accept a folder) into a temp workspace
    /// 2. spawn the bundled ceremony_tool contribute ...
    /// 3. zip the output into ~/Downloads/contribution-<tier>-<hex8>.zip
    /// Returns the path of the produced zip.
    func contribute(
        sourceURL: URL,
        tier: String,
        participantHex: String,
        log: @escaping (String) -> Void,
        registerProcess: @escaping (Process) -> Void
    ) async throws -> URL {
        let fm = FileManager.default
        let workspace = fm.temporaryDirectory
            .appendingPathComponent("ceremony-\(UUID().uuidString)", isDirectory: true)
        try fm.createDirectory(at: workspace, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: workspace) }

        // Step 1: materialize prev/ inside workspace.
        let prevRoot = workspace.appendingPathComponent("extracted", isDirectory: true)
        try fm.createDirectory(at: prevRoot, withIntermediateDirectories: true)

        try await materialize(sourceURL: sourceURL, into: prevRoot, log: log,
                              registerProcess: registerProcess)

        let inDir = try resolveStateDir(prevRoot, fm: fm)
        let outDir = workspace.appendingPathComponent("mine", isDirectory: true)
        try fm.createDirectory(at: outDir, withIntermediateDirectories: true)

        // Step 2: run ceremony_tool contribute.
        let toolURL = try bundledTool()

        log("Running ceremony_tool contribute --state-dir … --out-dir … --participant …")
        _ = try await run(
            executable: toolURL.path,
            args: [
                "contribute",
                "--state-dir", inDir.path,
                "--out-dir", outDir.path,
                "--participant", participantHex,
            ],
            log: log,
            registerProcess: registerProcess
        )

        // Step 3: zip mine/ → ~/Downloads/contribution-<tier>-<hex8>.zip
        let downloads = try fm.url(for: .downloadsDirectory, in: .userDomainMask,
                                   appropriateFor: nil, create: true)
        let hex8 = String(participantHex.prefix(8))
        let outZip = downloads.appendingPathComponent("contribution-\(tier)-\(hex8).zip")
        if fm.fileExists(atPath: outZip.path) {
            try fm.removeItem(at: outZip)
        }
        log("Packing \(outZip.lastPathComponent)…")
        _ = try await run(
            executable: "/usr/bin/ditto",
            args: ["-c", "-k", "--sequesterRsrc", outDir.path, outZip.path],
            log: log,
            registerProcess: registerProcess
        )
        return outZip
    }

    /// Verify one or two state directories with the bundled CLI.
    ///
    /// - If only `before` is provided → `verify-state --state-dir <before>`
    ///   (use this to check a single round's self-consistency; for round 0
    ///   this also verifies the initial-contribution proof).
    /// - If both are provided → `verify-contribution --before-state-dir <before>
    ///   --after-state-dir <after>` (the full six-pairing chained check).
    func verify(
        beforeURL: URL,
        afterURL: URL?,
        log: @escaping (String) -> Void,
        registerProcess: @escaping (Process) -> Void
    ) async throws {
        let fm = FileManager.default
        let workspace = fm.temporaryDirectory
            .appendingPathComponent("verify-\(UUID().uuidString)", isDirectory: true)
        try fm.createDirectory(at: workspace, withIntermediateDirectories: true)
        defer { try? fm.removeItem(at: workspace) }

        let beforeRoot = workspace.appendingPathComponent("before", isDirectory: true)
        try fm.createDirectory(at: beforeRoot, withIntermediateDirectories: true)
        try await materialize(sourceURL: beforeURL, into: beforeRoot, log: log,
                              registerProcess: registerProcess)
        let beforeDir = try resolveStateDir(beforeRoot, fm: fm)

        let toolURL = try bundledTool()

        if let afterURL = afterURL {
            let afterRoot = workspace.appendingPathComponent("after", isDirectory: true)
            try fm.createDirectory(at: afterRoot, withIntermediateDirectories: true)
            try await materialize(sourceURL: afterURL, into: afterRoot, log: log,
                                  registerProcess: registerProcess)
            let afterDir = try resolveStateDir(afterRoot, fm: fm)

            log("Running ceremony_tool verify-contribution …")
            _ = try await run(
                executable: toolURL.path,
                args: [
                    "verify-contribution",
                    "--before-state-dir", beforeDir.path,
                    "--after-state-dir", afterDir.path,
                ],
                log: log,
                registerProcess: registerProcess
            )
        } else {
            log("Running ceremony_tool verify-state …")
            _ = try await run(
                executable: toolURL.path,
                args: ["verify-state", "--state-dir", beforeDir.path],
                log: log,
                registerProcess: registerProcess
            )
        }
    }

    // MARK: - internals

    private func bundledTool() throws -> URL {
        guard let toolURL = Bundle.main.url(forResource: "ceremony_tool", withExtension: nil) else {
            throw CeremonyRunnerError(message: "Bundled ceremony_tool not found in app resources.")
        }
        return toolURL
    }

    /// Copy a folder or unzip an archive into `dest`.
    private func materialize(
        sourceURL: URL,
        into dest: URL,
        log: @escaping (String) -> Void,
        registerProcess: @escaping (Process) -> Void
    ) async throws {
        let fm = FileManager.default
        var isDir: ObjCBool = false
        guard fm.fileExists(atPath: sourceURL.path, isDirectory: &isDir) else {
            throw CeremonyRunnerError(message: "Item no longer exists: \(sourceURL.path)")
        }
        if isDir.boolValue {
            log("Using \(sourceURL.lastPathComponent)/ directly.")
            for item in try fm.contentsOfDirectory(at: sourceURL, includingPropertiesForKeys: nil) {
                let dst = dest.appendingPathComponent(item.lastPathComponent)
                try fm.copyItem(at: item, to: dst)
            }
        } else {
            log("Extracting \(sourceURL.lastPathComponent)…")
            _ = try await run(
                executable: "/usr/bin/unzip",
                args: ["-oqq", sourceURL.path, "-d", dest.path],
                log: log,
                registerProcess: registerProcess
            )
        }
    }

    /// Pick the right state-dir from an extracted tree. The coordinator emits
    /// zips that unpack to `prev/state.srs`, but users sometimes drop a zip
    /// without the wrapping folder.
    private func resolveStateDir(_ extracted: URL, fm: FileManager) throws -> URL {
        // Look for state.srs at common locations.
        let candidates: [URL] = [
            extracted.appendingPathComponent("prev", isDirectory: true),
            extracted.appendingPathComponent("mine", isDirectory: true),
            extracted,
        ]
        for c in candidates {
            if fm.fileExists(atPath: c.appendingPathComponent("state.srs").path) {
                return c
            }
        }
        // Single wrapper dir? Use it if it contains state.srs.
        let kids = (try? fm.contentsOfDirectory(at: extracted, includingPropertiesForKeys: nil)) ?? []
        var isDir: ObjCBool = false
        if kids.count == 1,
           fm.fileExists(atPath: kids[0].path, isDirectory: &isDir), isDir.boolValue,
           fm.fileExists(atPath: kids[0].appendingPathComponent("state.srs").path) {
            return kids[0]
        }
        throw CeremonyRunnerError(message: "Could not locate state.srs in \(extracted.lastPathComponent).")
    }

    /// Spawns a subprocess, streams stdout+stderr to `log` line-by-line, and
    /// throws on non-zero exit. `registerProcess` is called once with the live
    /// `Process` so the UI can wire up a Cancel button.
    private func run(
        executable: String,
        args: [String],
        log: @escaping (String) -> Void,
        registerProcess: @escaping (Process) -> Void
    ) async throws -> Int32 {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: executable)
        p.arguments = args

        let stdout = Pipe()
        let stderr = Pipe()
        p.standardOutput = stdout
        p.standardError = stderr

        @Sendable func forward(_ handle: FileHandle) {
            let data = handle.availableData
            guard !data.isEmpty, let s = String(data: data, encoding: .utf8) else { return }
            for line in s.split(whereSeparator: { $0 == "\n" || $0 == "\r" }) where !line.isEmpty {
                log(String(line))
            }
        }
        stdout.fileHandleForReading.readabilityHandler = forward
        stderr.fileHandleForReading.readabilityHandler = forward

        try p.run()
        registerProcess(p)

        return try await withCheckedThrowingContinuation { cont in
            p.terminationHandler = { proc in
                stdout.fileHandleForReading.readabilityHandler = nil
                stderr.fileHandleForReading.readabilityHandler = nil
                if proc.terminationStatus == 0 {
                    cont.resume(returning: proc.terminationStatus)
                } else {
                    let name = (executable as NSString).lastPathComponent
                    cont.resume(throwing: CeremonyRunnerError(
                        message: "\(name) exited with status \(proc.terminationStatus)"
                    ))
                }
            }
        }
    }
}
