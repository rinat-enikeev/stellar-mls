# Reproducibly rebuilding `ceremony_tool`

Every release of `ceremony_tool` is built from a pinned Rust toolchain and
reproducible environment so third parties can rebuild the same commit and
confirm the published binary byte-for-byte.

## What's pinned

- Rust channel: `rust-toolchain.toml` at the repo root (currently
  `channel = "1.88.0"`). CI reads this file via `dtolnay/rust-toolchain@master`
  with `toolchain: stable`, which honors the toolchain file.
- Build profile: `[profile.release-ceremony]` in the root `Cargo.toml`, with
  `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`,
  `panic = "abort"`, `debug = false`.
- Cargo dependency graph: `Cargo.lock`, committed, consulted via `--locked`.
- Reproducible build env: `SOURCE_DATE_EPOCH` is set to the commit's
  committer timestamp; `TZ=UTC`, `LC_ALL=C`; `RUSTFLAGS` includes
  `--remap-path-prefix=<workspace>=/build` so the embedded build path is
  stable across machines.

## Linux (most reproducible)

Linux musl builds are byte-for-byte reproducible because the pinned
`rust:1.88.0-alpine3.20` image has a pinnable digest and the musl target
doesn't pull in system-local glibc.

```bash
./scripts/verify-ceremony-tool.sh v0.1.0 x86_64-unknown-linux-musl
```

The script:

1. Pulls the pinned rust image by digest.
2. `git checkout v0.1.0`.
3. Runs `cargo build --locked --profile release-ceremony --target
   x86_64-unknown-linux-musl --bin ceremony_tool` inside the container,
   with the same environment variables as CI.
4. Computes `sha256sum` of the output.
5. Diffs against the `sha256` field in the published
   `ceremony_tool-v0.1.0-x86_64-unknown-linux-musl.buildinfo.json`.

If the two match, you have a byte-for-byte identical build. If they don't
match, either the published binary is compromised, your toolchain drifted,
or a transitive dependency's proc-macro emitted different output (rare —
this is what the pinned Rust channel is designed to prevent).

## macOS

macOS builds are reproducible up to code signing — the unsigned binary is
reproducible, but Apple's codesign adds per-build nonces that change the
final SHA. The `buildinfo.json` records the *unsigned* hash as the
reproducibility target.

## Windows

Windows MSVC builds are reproducible when the host toolchain matches. The
`v1` release path ships unsigned `.exe` artifacts with minisign + SHA-256
— the unsigned SHA is the reproducibility target. Azure Trusted Signing
(v2) will add per-release code signing on top; the `buildinfo.json` entry
will continue to record the unsigned pre-image.

## Triaging a hash mismatch

- Run the verifier script with `-v` (or set `VERIFY_VERBOSE=1`) to see the
  cargo command line. Confirm `--locked` made it through and `Cargo.lock`
  matches the tagged commit.
- Compare your `rustc --version --verbose` output against the
  `buildinfo.json`'s `rustc` field. If they differ, the pinned toolchain
  rolled forward locally; use the pinned Docker image.
- Run `cargo tree --locked` on both sides — this is how proc-macro
  version skew shows up in practice.

If a clean rebuild against a pinned-digest Docker image still diverges
from the published hash, open an issue and include both
`cargo tree --locked` outputs. Do not run the published binary until the
mismatch is explained.
