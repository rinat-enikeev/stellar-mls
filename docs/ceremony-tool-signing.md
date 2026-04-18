# Signing & notarizing `ceremony_tool` binaries

## Goal

A participant downloading `ceremony_tool` from the ceremony site should be
able to run it with no platform warnings, no quarantine workaround, and
no manual trust-chain steps. **macOS Gatekeeper** is the main UX blocker
today — an unsigned (or signed-but-not-notarized) Mach-O fails Gatekeeper
with "could not verify is free of malware". This doc specifies how we
sign and notarize, what credentials live where, and how the same
`scripts/.env` drives both local builds and CI.

## Non-goals

- A new code-signing trust root. We use Apple's existing Developer ID
  trust chain on macOS and minisign for everything else.
- Reproducible-build determinism for signed binaries. Signing inserts
  Apple-supplied timestamps; the unsigned pre-image is what reproduces.
  See `docs/ceremony-reproducible-build.md`.
- Windows Authenticode. Phase C of the rollout will add Azure Trusted
  Signing; until then Windows users get a SmartScreen prompt and rely on
  the published SHA-256 + minisign.

## Per-platform approach

| Target                          | What we do                                 | UX after install                            |
|---------------------------------|--------------------------------------------|---------------------------------------------|
| `aarch64-apple-darwin`          | Developer ID sign + notarize               | Brief "verifying" pause, then runs clean    |
| `x86_64-apple-darwin`           | Developer ID sign + notarize               | Same                                        |
| `x86_64-unknown-linux-musl`     | minisign (no platform signing exists)      | `chmod +x && ./bin`                         |
| `aarch64-unknown-linux-musl`    | minisign                                   | Same                                        |
| `x86_64-pc-windows-msvc`        | minisign + SHA-256 only (Phase A)          | SmartScreen prompt; user clicks "Run anyway"|

### Why notarize, not just sign?

Apple's Gatekeeper warning is not silenced by code signing alone — only
*notarization* removes it. The flow is:

1. `codesign --options runtime --timestamp --sign "Developer ID Application: ..."`
   produces a hardened-runtime, secure-timestamped Mach-O.
2. `xcrun notarytool submit` uploads the binary to Apple, which scans it
   and issues a notarization ticket.
3. CLI binaries can't be `xcrun stapler staple`d (only `.app`/`.pkg`/`.dmg`
   accept tickets), so first run does an online ticket fetch. After
   that, the ticket is cached and offline runs are clean.

If we ever want offline-friendly first run, wrap in a stapled `.pkg`. For
a single-binary CLI, the online-ticket path is the standard pattern
(rustup, deno, etc. all do this).

## Credentials contract — `scripts/.env`

This file is **gitignored**. It lives at the same path locally and is
mirrored to GitHub Secrets via `scripts/sync-gh-secrets.sh`. Variable
names are identical in both places.

```bash
# === Apple Developer ID (required for macOS notarization) ===

# Full identity string from `security find-identity -v -p codesigning`,
# including the team-id parens.
APPLE_DEVELOPER_ID="Developer ID Application: Your Name (ABCD123456)"

# Just the team ID portion. Find at developer.apple.com top-right.
APPLE_TEAM_ID=ABCD123456

# Apple ID email (the account that owns the Developer ID cert).
APPLE_ID=you@example.com

# App-specific password from appleid.apple.com → Sign-In and Security
# → App-Specific Passwords. Format: xxxx-xxxx-xxxx-xxxx.
APPLE_APP_PASSWORD=xxxx-xxxx-xxxx-xxxx

# Base64-encoded .p12 export of the Developer ID Application cert
# (private key + cert chain). Required for CI; locally the cert lives in
# the login keychain, so this can stay empty when running sign-macos.sh
# on your own Mac. To produce it:
#   security find-certificate -c "Developer ID Application" -p > cert.pem
#   ... or in Keychain Access: right-click → Export → .p12
#   base64 -i cert.p12 | pbcopy
APPLE_CERT_P12_BASE64=

# Password used when exporting the .p12.
APPLE_CERT_P12_PASSWORD=

# === minisign (optional — Linux + Windows artifact signatures) ===

# Base64-encoded ~/.minisign/minisign.key (the secret key file). Single
# line. Generated once with `minisign -G` then `base64 -i minisign.key`.
MINISIGN_SECRET_KEY_BASE64=

MINISIGN_PASSWORD=
```

### Producing the values, one-time

1. **Cert**: developer.apple.com → Certificates → `+` →
   *Developer ID Application* → follow CSR flow → download → double-click
   to install in Keychain Access.
2. **Verify it landed**:
   ```bash
   security find-identity -v -p codesigning | grep "Developer ID Application"
   ```
3. **Export to .p12**: Keychain Access → search "Developer ID Application"
   → expand to show the private key → select both cert and key →
   right-click → *Export 2 items* → `.p12` → set a password →
   `base64 -i cert.p12 | pbcopy`. Paste into `APPLE_CERT_P12_BASE64`.
4. **App-specific password**: appleid.apple.com → *Sign-In and Security*
   → *App-Specific Passwords* → generate, label `ceremony-notarytool`.
5. **Minisign keypair** (optional, but recommended for Linux):
   ```bash
   minisign -G -p .minisign/ceremony-tool.pub -s .minisign/minisign.key
   git add .minisign/ceremony-tool.pub          # commit pubkey
   base64 -i .minisign/minisign.key | tr -d '\n' > /tmp/sk.b64
   ```
   Paste `/tmp/sk.b64` into `MINISIGN_SECRET_KEY_BASE64`. Wipe `/tmp/sk.b64`.

## Local flow

```bash
# 1. Once: copy the template and fill it in.
cp scripts/.env.example scripts/.env
$EDITOR scripts/.env

# 2. Sign + notarize one already-built binary:
scripts/sign-macos.sh \
  target/release-ceremony/aarch64-apple-darwin/ceremony_tool

# 3. (Once) push the same env to GitHub so CI uses identical credentials:
scripts/sync-gh-secrets.sh
```

`sign-macos.sh` is idempotent: re-running on a notarized binary just
re-stamps the signature and re-submits to notarytool (Apple dedupes
identical submissions). Safe to use as your "fix it" hammer.

## CI flow

`.github/workflows/release-ceremony-tool.yml` (Phase A workflow already
in repo) gets two new conditional steps on macOS runners:

1. **Import cert into temp keychain** — only runs if
   `APPLE_CERT_P12_BASE64` is set. Creates an ephemeral keychain unique
   to the runner, imports the `.p12`, marks `codesign` as a permitted
   tool, then makes the keychain searchable.
2. **Sign + notarize** — invokes `scripts/sign-macos.sh` against the
   built artifact, *before* SHA-256 and minisign steps so those see the
   final bytes.

The workflow reads the secrets via `secrets.APPLE_DEVELOPER_ID` etc. —
identical names to the `.env` keys, so the sync script is a 1:1 mirror.

If the secrets aren't populated, the macOS jobs skip signing and emit an
unsigned binary as before. This keeps the workflow runnable for forks
and PR checks without leaking secrets.

## `scripts/sync-gh-secrets.sh`

Reads `scripts/.env`, iterates every `KEY=VALUE` line (skipping comments
and blanks), and pipes the value to `gh secret set KEY` against the
current repo. Quoted values have surrounding double-quotes stripped.
Empty values are skipped (so a half-filled `.env` doesn't clobber CI
secrets with empty strings).

```bash
gh auth status            # confirm you're logged in to the right account
scripts/sync-gh-secrets.sh
```

The script prints each key it sets but never echoes values. It does not
delete or list existing secrets — only sets/updates the keys present in
`.env`.

## Verification (you, on your Mac, after notarization)

```bash
codesign --verify --strict --verbose=2 ceremony_tool
# expect: "valid on disk" + "satisfies its Designated Requirement"

spctl --assess -vvv --type execute ceremony_tool
# expect: "accepted, source=Notarized Developer ID"

# What a user sees first time (online):
xattr -w com.apple.quarantine "0083;0;Safari;" ceremony_tool  # simulate Safari download
open -a Terminal --args ceremony_tool --version
# expect: brief "verifying" pause, then runs. No Gatekeeper dialog.
```

## Verification (downstream user, no signing toolchain)

The download page (`/download.html`) shows, per asset:

- SHA-256 from `ceremony-downloads.json`
- minisign signature URL + the project pubkey (committed at
  `.minisign/ceremony-tool.pub`)
- Direct GitHub Release link

Users with `minisign` installed can run:

```bash
minisign -Vm ceremony_tool-vX.Y.Z-<target> \
  -P "$(tail -1 .minisign/ceremony-tool.pub)"
```

macOS users don't need this — the platform Gatekeeper check is
authoritative — but having it lets a paranoid user verify out-of-band.

## Acceptance criteria

- [ ] `cp scripts/.env.example scripts/.env`, fill values, then
      `scripts/sign-macos.sh <bin>` produces a notarized binary that
      `spctl --assess` accepts as `Notarized Developer ID`.
- [ ] `scripts/sync-gh-secrets.sh` populates GitHub Secrets idempotently
      from the same file.
- [ ] `gh workflow run "Release ceremony_tool" -f tag=vX.Y.Z` produces
      release assets where the macOS binaries pass
      `spctl --assess --type execute` on a clean Mac with no developer
      tools installed.
- [ ] Linux/Windows assets remain functional (minisign signature when
      keys present, SHA-256 always).
- [ ] Re-running anything is safe: scripts and workflow handle
      partial/empty `.env` gracefully and never leak secret values to
      logs.

## Operational notes

- **Cert rotation**: Developer ID Application certs last 5 years. Set a
  calendar reminder. To rotate: revoke the old one, create a new one,
  re-export `.p12`, update `APPLE_CERT_P12_BASE64` and re-run
  `sync-gh-secrets.sh`.
- **Lost app-specific password**: revoke at appleid.apple.com, generate
  a new one, update `APPLE_APP_PASSWORD`, re-sync.
- **Compromised `.env`**: revoke the app-specific password and the cert
  immediately, then rotate per above. The .p12 password protects the
  exported key but treat it as low-rent — the app-specific password is
  the high-value asset since it talks to App Store Connect.
- **Notarization rejected**: `xcrun notarytool log <submission-id>
  --keychain-profile ceremony-notarytool` returns Apple's structured
  reason (entitlements, hardened runtime, libcrypto linkage, etc.).
  Common cause for Rust binaries: missing `--options runtime` —
  `sign-macos.sh` sets it.
