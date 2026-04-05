//! Generate a production keyset (proving keys + verification keys) for all tiers.
//!
//! Uses OsRng for real randomness — the resulting keys are NOT reproducible.
//! Destroy this binary's process memory after running (the toxic waste is gone
//! once the RNG state is dropped, but defense-in-depth is good practice).
//!
//! Usage:
//!   cargo run --release --bin generate_keyset -- --out-dir keyset-v1
//!
//! Output:
//!   keyset-v1/
//!     small/proving_key.bin
//!     small/verifying_key.bin
//!     medium/proving_key.bin
//!     medium/verifying_key.bin
//!     large/proving_key.bin
//!     large/verifying_key.bin
//!     vk-small.json          (contract-ready VK)
//!     vk-medium.json
//!     vk-large.json
//!     metadata.json           (version, hashes, tier info)

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use ark_bls12_381::{Bls12_381, G1Affine, G2Affine};
use ark_groth16::{ProvingKey, VerifyingKey};
use ark_serialize::CanonicalSerialize;
use rand::rngs::OsRng;
use sha2::{Sha256, Digest};

use sep_xxxx_circuits::prover;

struct TierConfig {
    name: &'static str,
    depth: usize,
    max_members: usize,
}

const TIERS: &[TierConfig] = &[
    TierConfig { name: "small", depth: 5, max_members: 32 },
    TierConfig { name: "medium", depth: 8, max_members: 256 },
    TierConfig { name: "large", depth: 11, max_members: 2048 },
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (out_dir, version) = parse_args(env::args().skip(1))?;
    fs::create_dir_all(&out_dir)?;

    eprintln!("Generating production keyset (version: {version})");
    eprintln!("Using OsRng — keys are NOT reproducible from seed.");
    eprintln!();

    let mut tier_metadata = Vec::new();

    for tier in TIERS {
        eprintln!("  {} tier (depth {}, max {} members)", tier.name, tier.depth, tier.max_members);

        let tier_dir = out_dir.join(tier.name);
        fs::create_dir_all(&tier_dir)?;

        // Generate with real randomness
        let mut rng = OsRng;
        let setup = prover::setup(tier.depth, &mut rng)?;

        // Serialize proving key (compressed)
        let pk_bytes = serialize_proving_key(&setup.proving_key)?;
        let pk_path = tier_dir.join("proving_key.bin");
        fs::write(&pk_path, &pk_bytes)?;
        let pk_hash = sha256_hex(&pk_bytes);
        eprintln!("    proving_key.bin: {} bytes, sha256:{}", pk_bytes.len(), &pk_hash);

        // Serialize verifying key (compressed)
        let vk_bytes = serialize_verifying_key(&setup.verifying_key)?;
        let vk_path = tier_dir.join("verifying_key.bin");
        fs::write(&vk_path, &vk_bytes)?;
        let vk_hash = sha256_hex(&vk_bytes);
        eprintln!("    verifying_key.bin: {} bytes, sha256:{}", vk_bytes.len(), &vk_hash);

        // Write contract-ready VK JSON
        let vk_json = verification_key_json(&setup.verifying_key);
        let vk_json_path = out_dir.join(format!("vk-{}.json", tier.name));
        fs::write(&vk_json_path, &vk_json)?;

        tier_metadata.push(format!(
            concat!(
                "    {{\n",
                "      \"tier\": \"{}\",\n",
                "      \"depth\": {},\n",
                "      \"max_members\": {},\n",
                "      \"proving_key_sha256\": \"{}\",\n",
                "      \"proving_key_bytes\": {},\n",
                "      \"verifying_key_sha256\": \"{}\",\n",
                "      \"verifying_key_bytes\": {}\n",
                "    }}"
            ),
            tier.name, tier.depth, tier.max_members,
            pk_hash, pk_bytes.len(),
            vk_hash, vk_bytes.len(),
        ));
    }

    // Write metadata.json
    let metadata = format!(
        concat!(
            "{{\n",
            "  \"version\": \"{}\",\n",
            "  \"generated\": \"{}\",\n",
            "  \"rng\": \"OsRng\",\n",
            "  \"tiers\": [\n",
            "{}\n",
            "  ]\n",
            "}}\n"
        ),
        version,
        chrono_now(),
        tier_metadata.join(",\n"),
    );
    fs::write(out_dir.join("metadata.json"), &metadata)?;

    eprintln!();
    eprintln!("Keyset written to {}", out_dir.display());
    eprintln!();
    eprintln!("IMPORTANT: The toxic waste (RNG state) is destroyed when this process exits.");
    eprintln!("For production use with MPC ceremony, use a multi-party setup tool instead.");

    Ok(())
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<(PathBuf, String), Box<dyn std::error::Error>> {
    let mut out_dir: Option<PathBuf> = None;
    let mut version = String::from("1");
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out-dir" => {
                let value = args.next().ok_or("--out-dir requires a value")?;
                out_dir = Some(PathBuf::from(value));
            }
            "--version" => {
                version = args.next().ok_or("--version requires a value")?;
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    out_dir.ok_or_else(|| "--out-dir is required".into())
        .map(|d| (d, version))
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

/// Simple ISO 8601 timestamp without external dependency.
fn chrono_now() -> String {
    // Falls back to empty string if system time unavailable
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => format!("unix:{}", d.as_secs()),
        Err(_) => "unknown".to_string(),
    }
}
