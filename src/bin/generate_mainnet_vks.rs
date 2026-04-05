//! Generate verification key JSON files for contract deployment.
//!
//! Reads VK files from a keyset directory (produced by generate_keyset)
//! and outputs contract-ready JSON.
//!
//! Usage:
//!   # From a production keyset (recommended)
//!   cargo run --release --bin generate_mainnet_vks -- --keyset-dir keyset-v1 --out-dir /tmp/vks
//!
//!   # From seed (testing/dev only — prints a warning)
//!   cargo run --release --bin generate_mainnet_vks -- --seed 42 --out-dir /tmp/vks

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use ark_bls12_381::{Bls12_381, G1Affine, G2Affine};
use ark_groth16::VerifyingKey;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

use sep_xxxx_circuits::prover;

struct TierConfig {
    name: &'static str,
    depth: usize,
}

const TIERS: &[TierConfig] = &[
    TierConfig { name: "small", depth: 5 },
    TierConfig { name: "medium", depth: 8 },
    TierConfig { name: "large", depth: 11 },
];

enum KeySource {
    Keyset(PathBuf),
    Seed(u64),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (source, out_dir) = parse_args(env::args().skip(1))?;
    fs::create_dir_all(&out_dir)?;

    match &source {
        KeySource::Keyset(dir) => {
            eprintln!("Reading verification keys from keyset: {}", dir.display());
        }
        KeySource::Seed(seed) => {
            eprintln!("WARNING: Generating VKs from seed {} — for testing/dev only!", seed);
            eprintln!("  Production deployments should use --keyset-dir with keys from generate_keyset.");
            eprintln!();
        }
    }

    for tier in TIERS {
        eprintln!("  {} tier (depth {})", tier.name, tier.depth);

        let vk = match &source {
            KeySource::Keyset(dir) => {
                let vk_path = dir.join(tier.name).join("verifying_key.bin");
                let bytes = fs::read(&vk_path)
                    .map_err(|e| format!("failed to read {}: {e}", vk_path.display()))?;
                VerifyingKey::<Bls12_381>::deserialize_compressed(&bytes[..])
                    .map_err(|e| format!("failed to deserialize VK for {}: {e}", tier.name))?
            }
            KeySource::Seed(seed) => {
                let mut rng = ChaCha20Rng::seed_from_u64(*seed);
                let setup = prover::setup(tier.depth, &mut rng)?;
                setup.verifying_key
            }
        };

        let json = verification_key_json(&vk);
        let path = out_dir.join(format!("vk-{}.json", tier.name));
        fs::write(&path, &json)?;
    }

    eprintln!("Done. VK files written to {}", out_dir.display());
    Ok(())
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<(KeySource, PathBuf), Box<dyn std::error::Error>> {
    let mut out_dir: Option<PathBuf> = None;
    let mut keyset_dir: Option<PathBuf> = None;
    let mut seed: Option<u64> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                out_dir = Some(PathBuf::from(args.next().ok_or("--out-dir requires a value")?));
            }
            "--keyset-dir" => {
                keyset_dir = Some(PathBuf::from(args.next().ok_or("--keyset-dir requires a value")?));
            }
            "--seed" => {
                let val = args.next().ok_or("--seed requires a value")?;
                seed = Some(val.parse().map_err(|_| format!("invalid seed: {val}"))?);
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }

    let out_dir = out_dir.ok_or("--out-dir is required")?;
    let source = match (keyset_dir, seed) {
        (Some(dir), None) => KeySource::Keyset(dir),
        (None, Some(s)) => KeySource::Seed(s),
        (Some(_), Some(_)) => return Err("cannot specify both --keyset-dir and --seed".into()),
        (None, None) => return Err("specify --keyset-dir (production) or --seed (testing)".into()),
    };

    Ok((source, out_dir))
}

fn verification_key_json(vk: &VerifyingKey<Bls12_381>) -> String {
    let ic = vk
        .gamma_abc_g1
        .iter()
        .map(|point| format!("\"{}\"", hex_encode(&serialize_g1(point))))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        concat!(
            "{{",
            "\"alpha_g1\":\"{}\",",
            "\"beta_g2\":\"{}\",",
            "\"gamma_g2\":\"{}\",",
            "\"delta_g2\":\"{}\",",
            "\"ic\":[{}]",
            "}}\n"
        ),
        hex_encode(&serialize_g1(&vk.alpha_g1)),
        hex_encode(&serialize_g2(&vk.beta_g2)),
        hex_encode(&serialize_g2(&vk.gamma_g2)),
        hex_encode(&serialize_g2(&vk.delta_g2)),
        ic,
    )
}

fn serialize_g1(point: &G1Affine) -> Vec<u8> {
    let mut bytes = Vec::new();
    point.serialize_uncompressed(&mut bytes).expect("G1 serialization");
    bytes
}

fn serialize_g2(point: &G2Affine) -> Vec<u8> {
    let mut bytes = Vec::new();
    point.serialize_uncompressed(&mut bytes).expect("G2 serialization");
    bytes
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(nibble_to_hex(byte >> 4));
        out.push(nibble_to_hex(byte & 0x0f));
    }
    out
}

fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => unreachable!(),
    }
}
