# Ceremony Tool — macOS GUI app

## Why

Participants hit two friction points with the CLI:

1. Gatekeeper warnings on first run (solved for the CLI in
   `ceremony-tool-signing.md`, but still a cognitive hurdle).
2. Opening Terminal, pasting a command with a 64-char hex participant
   key, and keeping the window alive for several minutes.

Most ceremony participants are on a Mac. A small drag-and-drop .app removes
both — users download one signed+notarized bundle, drop `prev.zip` onto it,
and get a `contribution.zip` back in `~/Downloads/` they can upload via the
web page. No terminal, no commands, no separate CLI install.

## Non-goals

- **No in-app Nostr signing / upload.** The web page
  (`contribute.html`) keeps the auth + upload role. The .app is a pure
  local computer: `prev.zip → contribution.zip`. Keys never leave the
  machine. This keeps the app audit surface trivial.
- **No iOS/iPadOS.** This is desktop-only tooling.
- **No Xcode project committed.** The sources build with a single
  `swiftc` invocation driven by a shell script, so the repo stays free
  of `project.pbxproj` churn.

## UX (one window)

```
┌─ Onym Trusted Setup Ceremony ──────────────────┐
│                                                │
│  Tier: ( Small ) ( Medium ) ( Large )          │
│  Your Nostr hex pubkey: [________________]     │
│                                                │
│  ┌──────────────────────────────────────────┐  │
│  │                                          │  │
│  │           Drop prev.zip here             │  │
│  │                                          │  │
│  └──────────────────────────────────────────┘  │
│                                                │
│  Log …                                         │
│                                                │
│  Ready: contribution-small-a1b2c3d4.zip        │
│          [Reveal in Finder] [Upload in browser]│
└────────────────────────────────────────────────┘
```

Tier and pubkey are stored in `UserDefaults` so repeat contributors only
fill them once.

## Pipeline

On drop the app:

1. Extracts the dropped zip (or folder) into a temp dir via `/usr/bin/unzip`.
2. Shells out to the **bundled** `ceremony_tool`:
   ```
   contribute --tier <T> --in-dir <extracted>/prev --out-dir <temp>/mine --participant <hex>
   ```
3. Streams stdout/stderr into the log view.
4. Packs `mine/` into `~/Downloads/contribution-<tier>-<hex8>.zip` via
   `/usr/bin/ditto`.
5. Offers *Reveal in Finder* and *Upload in browser* (opens
   `ceremony.onym.chat/contribute.html`).

Temp dir cleaned on exit.

## Distribution

One universal .app, delivered as a signed+notarized+stapled **`.dmg`** on
the GitHub Release. Stapling is the real win over the CLI path: the
Gatekeeper check is fully offline on first run.

## Build + CI

Pure `swiftc` build, no Xcode project:

- Sources at `clients/mac-ceremony/Sources/CeremonyTool/*.swift`.
- `clients/mac-ceremony/build.sh` compiles Swift for both arches, `lipo`s
  them into a universal Mach-O, assembles the `.app` bundle, embeds a
  universal `ceremony_tool` (itself lipo'd from the two CLI matrix
  outputs), signs inner binary → outer binary → bundle → dmg, runs
  `notarytool submit --wait`, then `xcrun stapler staple`.
- Release workflow gets a new `build-macos-gui` job that `needs:` both
  `apple-darwin` CLI jobs, downloads their artifacts, calls `build.sh`,
  and uploads `CeremonyTool-<tag>.dmg{,.sha256}` as a release asset.
- `release` job now depends on `build-macos-gui`, so the dmg is bundled
  into the same Release and surfaces in `ceremony-downloads.json`.

## Manifest and website

`scripts/update-ceremony-downloads.py` gains a `kind` field on each asset
(`cli` default; `gui` for the dmg). The GUI asset has `target:
"macos-universal-gui"` so the Alpine code on `/download` and `/contribute`
can detect macOS and prefer it.

- `/download` shows the .dmg first on Mac UAs, with a *“Use the CLI
  instead”* disclosure for power users.
- `/contribute` step 4 replaces the CLI filename block with “Download the
  app → drop prev.zip → upload contribution.zip” when on macOS.

## Signing reuse

No new secrets. The same `APPLE_DEVELOPER_ID` / `APPLE_CERT_P12_BASE64` /
team-id / Apple ID / app-specific password that sign the CLI sign the
.app. Minisign still covers Linux + Windows CLI; the .app is notarized
only (Gatekeeper is the verifier Mac users actually see).

## Verification

- `codesign --verify --deep --strict --verbose=2 CeremonyTool.app`
- `spctl --assess -vv --type exec CeremonyTool.app` → *Notarized
  Developer ID*
- First-run test on a throwaway Mac user account: double-click after
  download, expect a quick "verifying…" dimmed spinner with no
  "unverified" warning.

## Risks

| Risk                                               | Mitigation                                                          |
|----------------------------------------------------|---------------------------------------------------------------------|
| Large log output from ceremony_tool stalls UI      | Log writes are batched onto MainActor; no sync writes on hot path.  |
| User drops a folder instead of a zip               | Detect; if dropped item is a dir containing `state.srs`, use directly. |
| User types a non-hex pubkey                        | Validate length=64 + charset before spawning the subprocess.        |
| Subprocess hangs                                   | Cancel button sends `Process.terminate()`; user can redrop.          |
| Bundled ceremony_tool and on-disk CLI diverge      | .app prints the embedded binary's `--version` into the log on first use. |
