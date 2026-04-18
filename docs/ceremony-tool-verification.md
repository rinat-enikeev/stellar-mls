# Verifying a `ceremony_tool` download

The coordinator's web UI at `ceremony.onym.chat/download` links to published
binaries on GitHub Releases. For each target we publish four files:

```
ceremony_tool-<tag>-<target>[.exe]                    # the binary itself
ceremony_tool-<tag>-<target>[.exe].sha256             # SHA-256 digest
ceremony_tool-<tag>-<target>[.exe].minisig            # minisign (optional)
ceremony_tool-<tag>-<target>[.exe].buildinfo.json     # reproducibility metadata
```

You should always verify at least the SHA-256. If a minisign signature is
published for your download, verify that too.

## 1. Verify the SHA-256

```bash
# Linux / macOS
sha256sum  --check ceremony_tool-v0.1.0-x86_64-unknown-linux-musl.sha256
# or on macOS
shasum -a 256 -c ceremony_tool-v0.1.0-aarch64-apple-darwin.sha256
```

On Windows (PowerShell):

```powershell
$expected = (Get-Content .\ceremony_tool-v0.1.0-x86_64-pc-windows-msvc.exe.sha256).Split(' ')[0]
$actual   = (Get-FileHash .\ceremony_tool-v0.1.0-x86_64-pc-windows-msvc.exe -Algorithm SHA256).Hash.ToLower()
if ($actual -eq $expected) { "OK" } else { "MISMATCH" }
```

## 2. Verify the minisign signature (optional)

```bash
minisign -Vm ceremony_tool-v0.1.0-x86_64-unknown-linux-musl \
  -P <pinned-pubkey-from-ceremony.onym.chat/download>
```

The coordinator's minisign public key is pinned in three places that would
all have to be compromised together for a signing-key switch to go
undetected:

- `ceremony.onym.chat/download` (this page)
- The GitHub Release's release notes
- A kind-30078 Nostr event on `wss://nostr.onym.chat` with
  `d="sepceremony1:releasekeys"`

If any two disagree, do not run the binary.

## 3. Cross-check `buildinfo.json`

The `buildinfo.json` sibling records `commit`, `rustc`, `target`, and the
unsigned SHA-256 — which lets you reproduce the build locally (see
`docs/ceremony-reproducible-build.md`) and confirm that the published
binary was built from the tagged commit, not a backdoor-injected fork.

## 4. Run the binary's self-check

Once downloaded and verified, run:

```bash
ceremony_tool --version
ceremony_tool verify-state --state-dir <downloaded-round>
```

`verify-state` re-runs the six pairing equations against any round's
state.srs, state.txt, and receipt.txt — the same check the coordinator
runs on upload and the same check the browser WASM verifier runs at
`ceremony.onym.chat/verify`.
