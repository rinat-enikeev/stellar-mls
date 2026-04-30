//! Generate a TurboPlonk update proof + public-input bytes for
//! `stellar contract invoke ... -- update_commitment`.
//!
//! ```text
//!     cargo run --bin gen-update-proof --features gen-proof-tool \
//!         --release -- \
//!         --depth 5 \
//!         --secret-keys 0x01,0x02,0x03,0x04,0x05,0x06,0x07,0x08 \
//!         --prover-index 3 \
//!         --epoch-old 0 \
//!         --salt-old 0xEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE \
//!         --salt-new 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF \
//!         --out-dir ./fresh-update-proof
//! ```
//!
//! Optional `--new-keys hex,hex,...` overrides the new-tree roster
//! (defaults to `--secret-keys`). The update circuit doesn't
//! constrain new-tree membership — see `circuit::plonk::update`'s
//! "Security model — new-tree binding is *commitment-only*" note.
//!
//! Writes to `<out-dir>`:
//! - `proof.bin` / `proof.hex` — 1601-byte arkworks-uncompressed PLONK
//!   proof.
//! - `c_old.bin` / `c_old.hex`, `c_new.bin` / `c_new.hex` — 32-byte BE
//!   Fr scalars.
//! - `epoch_old.txt` — decimal `u64`.
//! - `public_inputs.json` — JSON array `[c_old, epoch_old, c_new]` of
//!   hex-encoded 32-byte BE scalars.
//!
//! Same VK-shape and RNG-determinism notes as `gen-membership-proof`.

#![cfg(feature = "plonk")]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use ark_bls12_381_v05::Fr;
use ark_ff_v05::{BigInteger, PrimeField};
use ark_serialize_v05::CanonicalSerialize;
use jf_relation::PlonkCircuit;
use rand_chacha::rand_core::SeedableRng;

use sep_xxxx_circuits::circuit::plonk::update::{synthesize_update, UpdateWitness};
use sep_xxxx_circuits::prover::plonk;

mod common;
use common::{parse_fr_list, parse_salt_hex, write_bin_and_hex, CliError};

struct Args {
    depth: usize,
    secret_keys: Vec<Fr>,
    new_keys: Option<Vec<Fr>>,
    prover_index: usize,
    epoch_old: u64,
    salt_old: [u8; 32],
    salt_new: [u8; 32],
    out_dir: PathBuf,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), CliError> {
    let args = parse_args()?;

    let witness = build_witness(&args)?;
    let c_old = witness.c_old;
    let c_new = witness.c_new;

    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_update(&mut circuit, &witness).map_err(CliError::circuit)?;
    circuit
        .finalize_for_arithmetization()
        .map_err(CliError::circuit)?;
    eprintln!(
        "[gen-update-proof] circuit finalised: {} gates",
        jf_relation::Circuit::num_gates(&circuit)
    );

    let keys = plonk::preprocess(&circuit).map_err(CliError::plonk)?;
    let mut rng = rand_chacha::ChaCha20Rng::from_seed([0u8; 32]);
    let proof = plonk::prove(&mut rng, &keys.pk, &circuit).map_err(CliError::plonk)?;

    let mut proof_bytes = Vec::new();
    proof
        .serialize_uncompressed(&mut proof_bytes)
        .map_err(CliError::serialize)?;
    if proof_bytes.len() != 1601 {
        return Err(CliError::other(format!(
            "unexpected proof length {}; verifier crate expects 1601",
            proof_bytes.len()
        )));
    }

    let public_inputs = vec![c_old, Fr::from(args.epoch_old), c_new];
    plonk::verify(&keys.vk, &public_inputs, &proof).map_err(|e| {
        CliError::other(format!(
            "self-verify rejected proof — witness or circuit shape is wrong: {e:?}"
        ))
    })?;

    fs::create_dir_all(&args.out_dir).map_err(CliError::io)?;
    write_bin_and_hex(&args.out_dir, "proof", &proof_bytes)?;

    let c_old_bytes = fr_to_be32(&c_old);
    let c_new_bytes = fr_to_be32(&c_new);
    write_bin_and_hex(&args.out_dir, "c_old", &c_old_bytes)?;
    write_bin_and_hex(&args.out_dir, "c_new", &c_new_bytes)?;

    let epoch_path = args.out_dir.join("epoch_old.txt");
    fs::write(&epoch_path, args.epoch_old.to_string()).map_err(CliError::io)?;

    let pi_json = format!(
        "[\"{}\",\"{}\",\"{}\"]\n",
        hex(&c_old_bytes),
        hex(&u64_be32(args.epoch_old)),
        hex(&c_new_bytes),
    );
    fs::write(args.out_dir.join("public_inputs.json"), &pi_json).map_err(CliError::io)?;

    eprintln!(
        "[gen-update-proof] depth={} prover_index={} epoch_old={} → {}",
        args.depth,
        args.prover_index,
        args.epoch_old,
        args.out_dir.display()
    );
    Ok(())
}

fn build_witness(args: &Args) -> Result<UpdateWitness, CliError> {
    use sep_xxxx_circuits::circuit::plonk::poseidon::{
        poseidon_hash_one_v05, poseidon_hash_two_v05,
    };

    if args.prover_index >= args.secret_keys.len() {
        return Err(CliError::other(format!(
            "prover_index {} out of range for {} secret keys",
            args.prover_index,
            args.secret_keys.len()
        )));
    }
    let depth = args.depth;
    if depth >= 32 {
        return Err(CliError::other(format!(
            "depth {depth} out of supported range (5/8/11)"
        )));
    }
    let num_leaves = 1usize << depth;
    if args.secret_keys.len() > num_leaves {
        return Err(CliError::other(format!(
            "{} old secret keys exceed depth-{} tree capacity {}",
            args.secret_keys.len(),
            depth,
            num_leaves
        )));
    }

    let new_keys = args.new_keys.as_ref().unwrap_or(&args.secret_keys);
    if new_keys.len() > num_leaves {
        return Err(CliError::other(format!(
            "{} new secret keys exceed depth-{} tree capacity {}",
            new_keys.len(),
            depth,
            num_leaves
        )));
    }

    let (root_old, paths_old) = build_tree(&args.secret_keys, depth);
    let (root_new, _) = build_tree(new_keys, depth);

    let salt_old_fr = Fr::from_le_bytes_mod_order(&args.salt_old);
    let salt_new_fr = Fr::from_le_bytes_mod_order(&args.salt_new);
    let c_old = poseidon_hash_two_v05(
        &poseidon_hash_two_v05(&root_old, &Fr::from(args.epoch_old)),
        &salt_old_fr,
    );
    let c_new = poseidon_hash_two_v05(
        &poseidon_hash_two_v05(&root_new, &Fr::from(args.epoch_old + 1)),
        &salt_new_fr,
    );
    let _ = poseidon_hash_one_v05; // keep helper imported for symmetry with build_tree

    Ok(UpdateWitness {
        c_old,
        epoch_old: args.epoch_old,
        c_new,
        secret_key: args.secret_keys[args.prover_index],
        poseidon_root_old: root_old,
        salt_old: args.salt_old,
        merkle_path_old: paths_old[args.prover_index].clone(),
        leaf_index_old: args.prover_index,
        poseidon_root_new: root_new,
        salt_new: args.salt_new,
        depth,
    })
}

fn build_tree(secret_keys: &[Fr], depth: usize) -> (Fr, Vec<Vec<Fr>>) {
    use sep_xxxx_circuits::circuit::plonk::poseidon::{
        poseidon_hash_one_v05, poseidon_hash_two_v05,
    };

    let leaves: Vec<Fr> = secret_keys.iter().map(poseidon_hash_one_v05).collect();
    let num_leaves = 1usize << depth;
    let mut nodes = vec![Fr::from(0u64); 2 * num_leaves];
    for (i, leaf) in leaves.iter().enumerate() {
        nodes[num_leaves + i] = *leaf;
    }
    for i in (1..num_leaves).rev() {
        nodes[i] = poseidon_hash_two_v05(&nodes[2 * i], &nodes[2 * i + 1]);
    }
    let root = nodes[1];

    let mut paths: Vec<Vec<Fr>> = Vec::with_capacity(secret_keys.len());
    for prover_index in 0..secret_keys.len() {
        let mut path = Vec::with_capacity(depth);
        let mut cur = num_leaves + prover_index;
        for _ in 0..depth {
            let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
            path.push(nodes[sib]);
            cur /= 2;
        }
        paths.push(path);
    }
    (root, paths)
}

fn fr_to_be32(fr: &Fr) -> [u8; 32] {
    let bytes = fr.into_bigint().to_bytes_be();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    out
}

fn u64_be32(v: u64) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[24..32].copy_from_slice(&v.to_be_bytes());
    out
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn parse_args() -> Result<Args, CliError> {
    let raw: Vec<String> = env::args().collect();
    let mut depth: Option<usize> = None;
    let mut secret_keys: Option<Vec<Fr>> = None;
    let mut new_keys: Option<Vec<Fr>> = None;
    let mut prover_index: Option<usize> = None;
    let mut epoch_old: Option<u64> = None;
    let mut salt_old: Option<[u8; 32]> = None;
    let mut salt_new: Option<[u8; 32]> = None;
    let mut out_dir: Option<PathBuf> = None;

    let mut i = 1;
    while i < raw.len() {
        let arg = &raw[i];
        match arg.as_str() {
            "--depth" => {
                let s = raw.get(i + 1).ok_or_else(|| CliError::usage("--depth needs a value"))?;
                depth = Some(s.parse().map_err(|_| CliError::usage(format!(
                    "--depth must be an integer, got {s:?}"
                )))?);
                i += 2;
            }
            "--secret-keys" => {
                let s = raw.get(i + 1).ok_or_else(|| CliError::usage("--secret-keys needs a value"))?;
                secret_keys = Some(parse_fr_list(s)?);
                i += 2;
            }
            "--new-keys" => {
                let s = raw.get(i + 1).ok_or_else(|| CliError::usage("--new-keys needs a value"))?;
                new_keys = Some(parse_fr_list(s)?);
                i += 2;
            }
            "--prover-index" => {
                let s = raw.get(i + 1).ok_or_else(|| CliError::usage("--prover-index needs a value"))?;
                prover_index = Some(s.parse().map_err(|_| CliError::usage(format!(
                    "--prover-index must be an integer, got {s:?}"
                )))?);
                i += 2;
            }
            "--epoch-old" => {
                let s = raw.get(i + 1).ok_or_else(|| CliError::usage("--epoch-old needs a value"))?;
                epoch_old = Some(s.parse().map_err(|_| CliError::usage(format!(
                    "--epoch-old must be a u64, got {s:?}"
                )))?);
                i += 2;
            }
            "--salt-old" => {
                let s = raw.get(i + 1).ok_or_else(|| CliError::usage("--salt-old needs a value"))?;
                salt_old = Some(parse_salt_hex(s)?);
                i += 2;
            }
            "--salt-new" => {
                let s = raw.get(i + 1).ok_or_else(|| CliError::usage("--salt-new needs a value"))?;
                salt_new = Some(parse_salt_hex(s)?);
                i += 2;
            }
            "--out-dir" => {
                let s = raw.get(i + 1).ok_or_else(|| CliError::usage("--out-dir needs a value"))?;
                out_dir = Some(PathBuf::from(s));
                i += 2;
            }
            "--help" | "-h" => return Err(CliError::usage(USAGE)),
            other => return Err(CliError::usage(format!("unknown arg {other:?}"))),
        }
    }

    Ok(Args {
        depth: depth.ok_or_else(|| CliError::usage("missing --depth"))?,
        secret_keys: secret_keys.ok_or_else(|| CliError::usage("missing --secret-keys"))?,
        new_keys,
        prover_index: prover_index.ok_or_else(|| CliError::usage("missing --prover-index"))?,
        epoch_old: epoch_old.ok_or_else(|| CliError::usage("missing --epoch-old"))?,
        salt_old: salt_old.ok_or_else(|| CliError::usage("missing --salt-old"))?,
        salt_new: salt_new.ok_or_else(|| CliError::usage("missing --salt-new"))?,
        out_dir: out_dir.ok_or_else(|| CliError::usage("missing --out-dir"))?,
    })
}

const USAGE: &str = "\
usage: gen-update-proof
    --depth <5|8|11>
    --secret-keys <hex,hex,...>          (old roster, comma-separated 32-byte BE Fr scalars)
    [--new-keys <hex,hex,...>]           (defaults to --secret-keys; new tree's roster)
    --prover-index <N>                   (the prover's position in the OLD roster)
    --epoch-old <u64>                    (current group epoch; new epoch = old + 1)
    --salt-old <hex>                     (32-byte salt the old commitment was bound to)
    --salt-new <hex>                     (32-byte salt for the new commitment)
    --out-dir <path>                     (writes proof.bin / c_old.bin / c_new.bin / etc.)";
