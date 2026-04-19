//! Dev-only Democracy VK generator — **NOT FOR PRODUCTION**.
//!
//! Runs a single-party Groth16 setup over `DemocracyUpdateCircuit` with `OsRng`
//! and dumps the resulting verifying key as a contract-ready JSON. The toxic
//! waste (the trapdoor) is held in this single process and never securely
//! destroyed — anyone with the proving key can forge proofs.
//!
//! Use this only for local testnet iteration and client development while the
//! multi-party Phase 2 ceremony for `DemocracyUpdateCircuit` is pending
//! (tracked in `docs/democracy-circuit-ceremony.md`). The VK file names
//! include a `DEV-` prefix so they cannot be confused with production keys.
//!
//! Usage:
//!   cargo run --release --bin generate_democracy_vk_dev -- \
//!       --out-dir keyset-democracy-dev
//!
//! Outputs:
//!   keyset-democracy-dev/
//!     tier0-k32/ proving_key.bin, verifying_key.bin
//!     tier1-k256/ proving_key.bin, verifying_key.bin
//!     vk-democracy-DEV-0-32.json    (contract-ready)
//!     vk-democracy-DEV-1-256.json
//!     README.txt                    (refuses to be mistaken for a real VK)

use std::env;
use std::fs;
use std::path::PathBuf;

use ark_bls12_381::{Bls12_381, Fr, G1Affine, G2Affine};
use ark_groth16::{Groth16, ProvingKey, VerifyingKey};
use ark_serialize::CanonicalSerialize;
use ark_snark::SNARK;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

use sep_xxxx_circuits::circuit::democracy::DemocracyUpdateCircuit;

struct Pair {
    label: &'static str,
    tier: u32,
    depth: usize,
    k_max: usize,
}

// v1 matrix from docs/democracy-circuit-ceremony.md §1.
const PAIRS: &[Pair] = &[
    Pair { label: "tier0-k32",  tier: 0, depth: 5, k_max: 32 },
    Pair { label: "tier1-k256", tier: 1, depth: 8, k_max: 256 },
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = parse_args(env::args().skip(1))?;
    fs::create_dir_all(&out_dir)?;

    eprintln!("==============================================================");
    eprintln!("  DEV-ONLY Democracy VK generator — NOT FOR PRODUCTION USE.");
    eprintln!("  The trapdoor is held in this process only; anyone with the");
    eprintln!("  proving key can forge Democracy update proofs. Safe for");
    eprintln!("  local/testnet iteration only.");
    eprintln!("==============================================================");
    eprintln!();

    for pair in PAIRS {
        eprintln!(
            "  {} (tier={}, depth={}, k_max={})",
            pair.label, pair.tier, pair.depth, pair.k_max,
        );

        let tier_dir = out_dir.join(pair.label);
        fs::create_dir_all(&tier_dir)?;

        let mut rng = OsRng;
        let empty = DemocracyUpdateCircuit::<Fr>::empty(pair.depth, pair.k_max);
        let (pk, vk) =
            Groth16::<Bls12_381>::circuit_specific_setup(empty, &mut rng)?;

        let pk_bytes = serialize_proving_key(&pk)?;
        fs::write(tier_dir.join("proving_key.bin"), &pk_bytes)?;
        eprintln!(
            "    proving_key.bin: {} bytes, sha256:{}",
            pk_bytes.len(),
            sha256_hex(&pk_bytes),
        );

        let vk_bytes = serialize_verifying_key(&vk)?;
        fs::write(tier_dir.join("verifying_key.bin"), &vk_bytes)?;
        eprintln!(
            "    verifying_key.bin: {} bytes, sha256:{}",
            vk_bytes.len(),
            sha256_hex(&vk_bytes),
        );

        // Assert IC length matches the contract dispatcher expectation
        // (6 points = 5 public inputs + 1 constant). A mismatch here means
        // the circuit's public-input schedule drifted away from
        // `contracts/sep-xxxx/src/lib.rs` and must be fixed before any
        // attempt to wire the VK in.
        assert_eq!(
            vk.gamma_abc_g1.len(),
            6,
            "DemocracyUpdateCircuit must expose exactly 5 public inputs (→ 6 IC points); \
             contract dispatcher enforces this. Got {}.",
            vk.gamma_abc_g1.len(),
        );

        let vk_json = verification_key_json(&vk);
        let json_name = format!("vk-democracy-DEV-{}-{}.json", pair.tier, pair.k_max);
        fs::write(out_dir.join(&json_name), &vk_json)?;
        eprintln!("    {}: {} bytes", json_name, vk_json.len());
    }

    fs::write(out_dir.join("README.txt"), README_WARNING)?;

    eprintln!();
    eprintln!("Dev VKs written to {}", out_dir.display());
    eprintln!();
    eprintln!("Install into a local testnet contract with:");
    eprintln!("  stellar contract invoke ... update_vk \\");
    eprintln!("      --kind '{{\"UpdateByType\":2}}' --tier 0 \\");
    eprintln!("      --new-vk \"$(cat {}/vk-democracy-DEV-0-32.json)\"",
             out_dir.display());
    eprintln!();
    eprintln!("Do NOT ship these to mainnet. Real Democracy VKs must come");
    eprintln!("from the Phase 2 ceremony (see docs/democracy-circuit-ceremony.md).");

    Ok(())
}

const README_WARNING: &str = "\
DEV-ONLY DEMOCRACY VERIFYING KEYS.

These files were produced by `generate_democracy_vk_dev` using OsRng in a
single process. The Groth16 trapdoor is not securely destroyed and anyone
holding the proving key can forge Democracy update proofs.

DO NOT publish these files to a production contract. DO NOT commit them
to a release branch or bundle them with a mobile binary.

The real Democracy VK must come from the Phase 2 trusted-setup ceremony
documented in docs/democracy-circuit-ceremony.md.
";

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut out_dir: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                let value = args.next().ok_or("--out-dir requires a value")?;
                out_dir = Some(PathBuf::from(value));
            }
            "-h" | "--help" => {
                eprintln!("Usage: generate_democracy_vk_dev --out-dir <PATH>");
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    out_dir.ok_or_else(|| "--out-dir is required".into())
}

fn serialize_proving_key(pk: &ProvingKey<Bls12_381>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    pk.serialize_compressed(&mut bytes)?;
    Ok(bytes)
}

fn serialize_verifying_key(vk: &VerifyingKey<Bls12_381>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    vk.serialize_compressed(&mut bytes)?;
    Ok(bytes)
}

fn sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hex_encode(&hash)
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
