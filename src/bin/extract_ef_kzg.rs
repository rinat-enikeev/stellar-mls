//! Extract the n=4096 PoT subset from EF's 2023 KZG ceremony transcript
//! into the byte layout `src/prover/srs.rs` consumes.
//!
//! Pipeline:
//!     scripts/build/fetch-ef-kzg.sh  ──>  /tmp/ef-kzg-2023-transcript.json (253 MB)
//!     cargo run --bin extract-ef-kzg --features plonk --release -- \
//!         /tmp/ef-kzg-2023-transcript.json src/prover/srs/ef-kzg-2023.bin
//!     shasum -a 256 src/prover/srs/ef-kzg-2023.bin > src/prover/srs/expected-hash.txt
//!
//! The transcript JSON shape (per kzg-ceremony-specs):
//!     { "transcripts": [
//!         { "numG1Powers": 4096, "numG2Powers": 65,
//!           "powersOfTau": {
//!             "G1Powers": [ "0x<48-byte hex compressed G1>", … 4096 ],
//!             "G2Powers": [ "0x<96-byte hex compressed G2>", …   65 ] },
//!           "witness": { … signed contribution chain … }
//!         }, { numG1Powers: 8192, … }, { 16384, … }, { 32768, … } ] }
//!
//! We pick `transcripts[0]` (n=4096), decompress points, write them in
//! arkworks-uncompressed encoding (96 B per G1, 192 B per G2) with a small
//! versioned header.
//!
//! Header layout (little-endian):
//!     [ 0..  4]  magic "EFKZ"
//!     [ 4..  8]  u32 version = 1
//!     [ 8.. 12]  u32 g1_count = 4096
//!     [12.. 16]  u32 g2_count = 65
//!     [16..]     g1_count × 96 B G1Affine, then g2_count × 192 B G2Affine

#![cfg(feature = "plonk")]

use std::env;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

// Arkworks 0.5 (Cargo-renamed) — emit bytes that jf-pcs (0.5) deserialises.
use ark_bls12_381_v05::{G1Affine, G2Affine};
use ark_serialize_v05::CanonicalSerialize;

const MAGIC: &[u8; 4] = b"EFKZ";
const VERSION: u32 = 1;
const TARGET_G1: usize = 4096;
const TARGET_G2: usize = 65;

const G1_COMPRESSED_BYTES: usize = 48;
const G2_COMPRESSED_BYTES: usize = 96;
const G1_UNCOMPRESSED_BYTES: usize = 96;
const G2_UNCOMPRESSED_BYTES: usize = 192;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if !(args.len() == 3 || args.len() == 4) {
        return Err(format!(
            "usage: {} <transcript.json> <out.bin> [<expected-hash.in>]\n\
             \n\
             If <expected-hash.in> is supplied, the extractor writes a Rust\n\
             array literal of the output's SHA-256 there. Default path is\n\
             derived from <out.bin> by replacing the file with `expected-hash.in`\n\
             in the same directory.",
            args.first().map(|s| s.as_str()).unwrap_or("extract_ef_kzg")
        ));
    }
    let transcript_path = PathBuf::from(&args[1]);
    let out_path = PathBuf::from(&args[2]);
    let hash_out_path = if args.len() == 4 {
        PathBuf::from(&args[3])
    } else {
        // Derive `<dir>/expected-hash.in` from `<dir>/<file>.bin`.
        let parent = out_path
            .parent()
            .ok_or_else(|| format!("cannot derive hash path from {}", out_path.display()))?;
        parent.join("expected-hash.in")
    };

    eprintln!("reading transcript from {} …", transcript_path.display());
    let raw = read_file(&transcript_path)?;
    eprintln!("  {} bytes", raw.len());

    eprintln!("parsing JSON (this may take a few seconds for the 253 MB transcript) …");
    let json: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|e| format!("transcript JSON parse failed: {e}"))?;

    let transcripts = json
        .get("transcripts")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "transcript root has no 'transcripts' array".to_string())?;
    if transcripts.is_empty() {
        return Err("transcripts array is empty".into());
    }

    // The published transcript contains four PoT sets (n=4096, 8192, 16384, 32768).
    // We pick the smallest set, which suffices for our circuits.
    let target = transcripts.iter().find(|t| {
        t.get("numG1Powers").and_then(|v| v.as_u64()) == Some(TARGET_G1 as u64)
    }).ok_or_else(|| format!("no transcript with numG1Powers == {} found", TARGET_G1))?;

    let num_g1 = target.get("numG1Powers").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let num_g2 = target.get("numG2Powers").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    if num_g1 != TARGET_G1 {
        return Err(format!("expected numG1Powers={TARGET_G1}, got {num_g1}"));
    }
    if num_g2 != TARGET_G2 {
        return Err(format!("expected numG2Powers={TARGET_G2}, got {num_g2}"));
    }
    eprintln!("  selected transcript: numG1Powers={num_g1}, numG2Powers={num_g2}");

    let pot = target
        .get("powersOfTau")
        .ok_or_else(|| "transcript has no 'powersOfTau' field".to_string())?;

    let g1_hexes: Vec<&str> = pot
        .get("G1Powers")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "powersOfTau.G1Powers missing or not an array".to_string())?
        .iter()
        .map(|v| v.as_str().ok_or_else(|| "G1Powers entry not a string".to_string()))
        .collect::<Result<_, _>>()?;
    if g1_hexes.len() != num_g1 {
        return Err(format!(
            "G1Powers length {} != numG1Powers {}",
            g1_hexes.len(),
            num_g1
        ));
    }

    let g2_hexes: Vec<&str> = pot
        .get("G2Powers")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "powersOfTau.G2Powers missing or not an array".to_string())?
        .iter()
        .map(|v| v.as_str().ok_or_else(|| "G2Powers entry not a string".to_string()))
        .collect::<Result<_, _>>()?;
    if g2_hexes.len() != num_g2 {
        return Err(format!(
            "G2Powers length {} != numG2Powers {}",
            g2_hexes.len(),
            num_g2
        ));
    }

    eprintln!("decompressing {} G1 points + {} G2 points …", num_g1, num_g2);

    let mut out = Vec::with_capacity(
        16 + num_g1 * G1_UNCOMPRESSED_BYTES + num_g2 * G2_UNCOMPRESSED_BYTES,
    );
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(num_g1 as u32).to_le_bytes());
    out.extend_from_slice(&(num_g2 as u32).to_le_bytes());

    for (i, hex) in g1_hexes.iter().enumerate() {
        let bytes = decode_hex(hex, G1_COMPRESSED_BYTES)
            .map_err(|e| format!("G1Powers[{i}]: {e}"))?;
        let point = decompress_g1_zcash(&bytes)
            .map_err(|e| format!("G1Powers[{i}] decompress: {e}"))?;
        let mut buf = Vec::with_capacity(G1_UNCOMPRESSED_BYTES);
        point.serialize_uncompressed(&mut buf)
            .map_err(|e| format!("G1Powers[{i}] serialize: {e:?}"))?;
        if buf.len() != G1_UNCOMPRESSED_BYTES {
            return Err(format!(
                "G1Powers[{i}] serialise produced {} bytes, expected {}",
                buf.len(), G1_UNCOMPRESSED_BYTES
            ));
        }
        out.extend_from_slice(&buf);
    }

    for (i, hex) in g2_hexes.iter().enumerate() {
        let bytes = decode_hex(hex, G2_COMPRESSED_BYTES)
            .map_err(|e| format!("G2Powers[{i}]: {e}"))?;
        let point = decompress_g2_zcash(&bytes)
            .map_err(|e| format!("G2Powers[{i}] decompress: {e}"))?;
        let mut buf = Vec::with_capacity(G2_UNCOMPRESSED_BYTES);
        point.serialize_uncompressed(&mut buf)
            .map_err(|e| format!("G2Powers[{i}] serialize: {e:?}"))?;
        if buf.len() != G2_UNCOMPRESSED_BYTES {
            return Err(format!(
                "G2Powers[{i}] serialise produced {} bytes, expected {}",
                buf.len(), G2_UNCOMPRESSED_BYTES
            ));
        }
        out.extend_from_slice(&buf);
    }

    eprintln!("writing {} bytes to {} …", out.len(), out_path.display());
    fs::write(&out_path, &out)
        .map_err(|e| format!("write {}: {e}", out_path.display()))?;

    use sha2::{Digest, Sha256};
    let hash: [u8; 32] = Sha256::digest(&out).into();
    eprintln!("sha256 = {}", hex_encode(&hash));

    let hash_literal = format_hash_literal(&hash);
    eprintln!("writing hash literal to {} …", hash_out_path.display());
    fs::write(&hash_out_path, &hash_literal)
        .map_err(|e| format!("write {}: {e}", hash_out_path.display()))?;

    eprintln!("done.");
    Ok(())
}

/// Format a 32-byte SHA-256 as a Rust `[u8; 32]` array literal that
/// `include!()` can splice into a `const` expression. Matches the form
/// `build.rs::parse_hash_array` accepts.
fn format_hash_literal(hash: &[u8; 32]) -> String {
    let mut s = String::from("[\n");
    for chunk in hash.chunks(8) {
        s.push_str("    ");
        for b in chunk {
            use std::fmt::Write;
            write!(&mut s, "0x{:02x}, ", b).unwrap();
        }
        s.push('\n');
    }
    s.push(']');
    s.push('\n');
    s
}

fn read_file(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let mut f = fs::File::open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(buf)
}

fn decode_hex(hex: &str, expected_len: usize) -> Result<Vec<u8>, String> {
    let s = hex.strip_prefix("0x").unwrap_or(hex);
    if s.len() != expected_len * 2 {
        return Err(format!(
            "expected {} hex chars ({} bytes), got {}",
            expected_len * 2, expected_len, s.len()
        ));
    }
    (0..expected_len)
        .map(|i| {
            u8::from_str_radix(&s[2 * i..2 * i + 2], 16)
                .map_err(|e| format!("invalid hex at byte {i}: {e}"))
        })
        .collect()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        write!(&mut s, "{:02x}", b).unwrap();
    }
    s
}

// ---------------------------------------------------------------------------
// BLS12-381 ZCash-format decompression.
//
// EF KZG transcript points use the ZCash serialisation format (the IETF/EIP-2537
// canonical form). arkworks's `serialize_compressed` emits a different byte
// layout, so we can't just `deserialize_compressed` the transcript bytes
// directly. Instead we decode the ZCash format manually:
//
//   First byte high bits:
//     bit 7 (0x80) = compression flag       (must be 1)
//     bit 6 (0x40) = infinity flag          (point at infinity)
//     bit 5 (0x20) = sort flag              (which y-root: 1 = larger lexicographic)
//   Remaining bits + remaining bytes:       big-endian x coordinate
//
// We recover y from x via the curve equation y² = x³ + 4 (G1) or y² = x³ + 4(u+1) (G2).
//
// Implemented directly here so we don't add a new dep just for this.
// ---------------------------------------------------------------------------

use ark_bls12_381_v05::{Fq, Fq2};
use ark_ec_v05::AffineRepr;
use ark_ff_v05::{Field, PrimeField};

fn decompress_g1_zcash(bytes: &[u8]) -> Result<G1Affine, String> {
    if bytes.len() != G1_COMPRESSED_BYTES {
        return Err(format!("expected {} bytes, got {}", G1_COMPRESSED_BYTES, bytes.len()));
    }
    let flags = bytes[0];
    let compressed = (flags & 0x80) != 0;
    let infinity = (flags & 0x40) != 0;
    let sort = (flags & 0x20) != 0;
    if !compressed {
        return Err("compressed flag (bit 7) not set".into());
    }
    if infinity {
        return Ok(G1Affine::identity());
    }

    let mut x_bytes = bytes.to_vec();
    x_bytes[0] &= 0x1f; // strip the three flag bits

    let x = Fq::from_be_bytes_mod_order(&x_bytes);
    if x.into_bigint() >= Fq::MODULUS {
        return Err("x ≥ field modulus after flag strip".into());
    }
    // y² = x³ + 4
    let b = Fq::from(4u64);
    let rhs = x * x * x + b;
    let y = rhs.sqrt().ok_or_else(|| "y² has no square root: not on curve".to_string())?;
    let neg_y = -y;
    // Sort flag: 1 means pick the larger y (lexicographically). Compare by big-endian repr.
    let chosen = if larger_lex(&y, &neg_y) == sort { y } else { neg_y };

    let p = G1Affine::new_unchecked(x, chosen);
    if !p.is_on_curve() {
        return Err("decompressed point not on curve".into());
    }
    if !p.is_in_correct_subgroup_assuming_on_curve() {
        return Err("decompressed point not in r-torsion subgroup".into());
    }
    Ok(p)
}

fn decompress_g2_zcash(bytes: &[u8]) -> Result<G2Affine, String> {
    if bytes.len() != G2_COMPRESSED_BYTES {
        return Err(format!("expected {} bytes, got {}", G2_COMPRESSED_BYTES, bytes.len()));
    }
    let flags = bytes[0];
    let compressed = (flags & 0x80) != 0;
    let infinity = (flags & 0x40) != 0;
    let sort = (flags & 0x20) != 0;
    if !compressed {
        return Err("compressed flag (bit 7) not set".into());
    }
    if infinity {
        return Ok(G2Affine::identity());
    }
    let mut x_bytes = bytes.to_vec();
    x_bytes[0] &= 0x1f;

    // ZCash G2 layout: x = (c1, c0) where c1 is the higher-order half (first 48 B
    // after flag strip) and c0 is the lower-order half (last 48 B). I.e. x = c0 + c1·u.
    let (c1_be, c0_be) = x_bytes.split_at(48);
    let c0 = Fq::from_be_bytes_mod_order(c0_be);
    let c1 = Fq::from_be_bytes_mod_order(c1_be);
    if c0.into_bigint() >= Fq::MODULUS || c1.into_bigint() >= Fq::MODULUS {
        return Err("x coordinate components ≥ field modulus".into());
    }
    let x = Fq2::new(c0, c1);
    // y² = x³ + 4(u+1)
    let b = Fq2::new(Fq::from(4u64), Fq::from(4u64));
    let rhs = x * x * x + b;
    let y = rhs.sqrt().ok_or_else(|| "y² has no square root: not on curve".to_string())?;
    let neg_y = -y;
    let chosen = if larger_lex_fq2(&y, &neg_y) == sort { y } else { neg_y };

    let p = G2Affine::new_unchecked(x, chosen);
    if !p.is_on_curve() {
        return Err("decompressed G2 point not on curve".into());
    }
    if !p.is_in_correct_subgroup_assuming_on_curve() {
        return Err("decompressed G2 point not in r-torsion subgroup".into());
    }
    Ok(p)
}

/// Returns true iff `a` is lexicographically larger than `b` (big-endian Fq comparison).
fn larger_lex(a: &Fq, b: &Fq) -> bool {
    a.into_bigint() > b.into_bigint()
}

/// For Fq2, comparison is on (c1, c0) lex order — the convention ZCash uses.
fn larger_lex_fq2(a: &Fq2, b: &Fq2) -> bool {
    if a.c1 != b.c1 {
        a.c1.into_bigint() > b.c1.into_bigint()
    } else {
        a.c0.into_bigint() > b.c0.into_bigint()
    }
}

