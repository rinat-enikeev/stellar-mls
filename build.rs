//! Build-time SRS hash assertion.
//!
//! When the `plonk` feature is active, refuse to compile if the embedded
//! SRS bytes (`src/prover/srs/ef-kzg-2023.bin`) don't match the SHA-256 hash
//! pinned in `src/prover/srs/expected-hash.in`. This makes "we accidentally
//! shipped the wrong SRS" structurally impossible — the build fails on
//! mismatch.
//!
//! Skipped entirely when the `plonk` feature is off; existing Groth16-only
//! builds need none of this and don't require the SRS bundle to be present.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=src/prover/srs/ef-kzg-2023.bin");
    println!("cargo:rerun-if-changed=src/prover/srs/expected-hash.in");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_FEATURE_PLONK").is_err() {
        // Groth16-only / default builds: SRS is not embedded; nothing to verify.
        return;
    }

    let srs_path = Path::new("src/prover/srs/ef-kzg-2023.bin");
    let bytes = std::fs::read(srs_path).unwrap_or_else(|e| {
        panic!(
            "EF KZG SRS missing at {} ({e}). Run \
             `scripts/build/fetch-ef-kzg.sh` then \
             `cargo run --bin extract-ef-kzg --features extract-tool --release -- \
             /tmp/ef-kzg-2023-transcript.json src/prover/srs/ef-kzg-2023.bin`",
            srs_path.display()
        )
    });

    use sha2::Digest;
    let actual: [u8; 32] = sha2::Sha256::digest(&bytes).into();

    let hash_path = Path::new("src/prover/srs/expected-hash.in");
    let raw = std::fs::read_to_string(hash_path).unwrap_or_else(|e| {
        panic!("expected-hash.in missing at {}: {e}", hash_path.display())
    });
    let expected = parse_hash_array(&raw).unwrap_or_else(|e| {
        panic!(
            "could not parse {} (expected `[0x12, 0x34, ...]` literal of 32 bytes): {e}",
            hash_path.display()
        )
    });

    // Bootstrap bypass: an all-zero hash means "placeholder; not yet
    // populated by the extractor". This lets `cargo run --bin
    // extract-ef-kzg` compile on a fresh checkout so the operator can
    // run the extractor in the first place. The extractor itself writes
    // a real hash on success, so subsequent builds enforce the check.
    if expected == [0u8; 32] {
        println!(
            "cargo:warning=src/prover/srs/expected-hash.in is the bootstrap placeholder; \
             SRS hash check is bypassed. Run \
             `cargo run --bin extract-ef-kzg --features extract-tool --release -- \
             /tmp/ef-kzg-2023-transcript.json` to populate it."
        );
        return;
    }

    if actual != expected {
        panic!(
            "SRS hash mismatch — refusing to build.\n\
             expected: {}\n\
             actual:   {}\n\
             Either the SRS bytes were corrupted, the wrong file is checked in, \
             or `expected-hash.in` is stale. Re-extract via the fetch + \
             extract-ef-kzg pipeline; if intentional, update expected-hash.in.",
            hex_encode(&expected),
            hex_encode(&actual),
        );
    }
}

/// Parse a `[u8; 32]` array literal of the form
/// `[0x12, 0x34, ..., 0xab]` (whitespace, line breaks, and trailing commas
/// permitted; any other characters rejected). Strict so a typo can't slip
/// past silently.
fn parse_hash_array(s: &str) -> Result<[u8; 32], String> {
    let trimmed = s.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| "expected literal wrapped in `[ ... ]`".to_string())?;

    let mut out = [0u8; 32];
    let mut count = 0usize;
    for tok in inner.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue; // tolerate trailing comma
        }
        let hex = tok
            .strip_prefix("0x")
            .or_else(|| tok.strip_prefix("0X"))
            .ok_or_else(|| format!("byte literal `{tok}` missing 0x prefix"))?;
        if hex.len() != 2 {
            return Err(format!("byte literal `{tok}` not exactly two hex digits"));
        }
        if count == 32 {
            return Err("more than 32 byte literals".to_string());
        }
        out[count] = u8::from_str_radix(hex, 16)
            .map_err(|e| format!("byte literal `{tok}` not valid hex: {e}"))?;
        count += 1;
    }
    if count != 32 {
        return Err(format!("expected 32 byte literals, got {count}"));
    }
    Ok(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        write!(&mut s, "{:02x}", b).unwrap();
    }
    s
}
