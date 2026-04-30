//! Generate a TurboPlonk membership proof + public-input bytes ready
//! for `stellar contract invoke`.
//!
//! ```text
//!     cargo run --bin gen-membership-proof --features gen-proof-tool \
//!         --release -- \
//!         --depth 5 \
//!         --secret-keys 0x01,0x02,0x03,0x04,0x05,0x06,0x07,0x08 \
//!         --prover-index 3 \
//!         --epoch 0 \
//!         --salt 0xEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE \
//!         --out-dir ./fresh-create-proof
//! ```
//!
//! Writes to `<out-dir>`:
//! - `proof.bin` — 1601-byte arkworks-uncompressed PLONK proof.
//! - `proof.hex` — same bytes, hex-encoded (no `0x` prefix).
//! - `commitment.bin` / `commitment.hex` — 32-byte BE Fr scalar.
//! - `epoch.txt` — decimal `u64` (matches the in-circuit
//!   `Fr::from(u64)` encoding the verifier expects).
//! - `public_inputs.json` — JSON array `[<commitment>, <epoch>]` of
//!   hex-encoded 32-byte BE scalars, ready for the
//!   `--public_inputs` arg of `stellar contract invoke`.
//!
//! ## VK matching
//!
//! The baked VK for tier `T` (= depth `5/8/11`) is produced by
//! `bake_membership_vk` from the *canonical* witness, but the VK
//! depends only on the circuit *shape*, not on witness values.
//! Any witness at the same depth produces a proof that verifies
//! under the same baked VK — so an operator can use whatever
//! `(secret_keys, prover_index, epoch, salt)` matches their actual
//! group state and the proof will still accept on-chain.
//!
//! ## RNG seed
//!
//! Pinned to `[0u8; 32]`. Identical witnesses therefore produce
//! byte-identical proofs (useful for replay-test scaffolding); to
//! vary the proof bytes, vary the witness.

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

use sep_xxxx_circuits::circuit::plonk::membership::{synthesize_membership, MembershipWitness};
use sep_xxxx_circuits::prover::plonk;

mod common;
use common::{
    parse_fr_hex, parse_fr_list, parse_salt_hex, write_bin_and_hex, CliError,
};

struct Args {
    depth: usize,
    secret_keys: Vec<Fr>,
    prover_index: usize,
    epoch: u64,
    salt: [u8; 32],
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

    // Native commitment = Poseidon(Poseidon(root, epoch), salt).
    let witness = build_witness(&args)?;
    let commitment = witness.commitment;

    let mut circuit = PlonkCircuit::<Fr>::new_turbo_plonk();
    synthesize_membership(&mut circuit, &witness).map_err(CliError::circuit)?;
    circuit
        .finalize_for_arithmetization()
        .map_err(CliError::circuit)?;
    eprintln!(
        "[gen-membership-proof] circuit finalised: {} gates",
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

    // Sanity self-check: prove → verify against the freshly-preprocessed
    // VK. Catches witness/circuit mismatches before the operator ships
    // the proof to testnet (where they'd see a contract InvalidProof
    // and have to debug remotely).
    let public_inputs = vec![commitment, Fr::from(args.epoch)];
    plonk::verify(&keys.vk, &public_inputs, &proof).map_err(|e| {
        CliError::other(format!(
            "self-verify rejected proof — witness or circuit shape is wrong: {e:?}"
        ))
    })?;

    fs::create_dir_all(&args.out_dir).map_err(CliError::io)?;
    write_bin_and_hex(&args.out_dir, "proof", &proof_bytes)?;

    let commitment_bytes = fr_to_be32(&commitment);
    write_bin_and_hex(&args.out_dir, "commitment", &commitment_bytes)?;

    let epoch_path = args.out_dir.join("epoch.txt");
    fs::write(&epoch_path, args.epoch.to_string()).map_err(CliError::io)?;

    // public_inputs.json — JSON array of hex strings, the shape
    // `stellar contract invoke --public_inputs '[...]'` expects.
    let pi_json = format!(
        "[\"{}\",\"{}\"]\n",
        hex(&commitment_bytes),
        hex(&u64_be32(args.epoch))
    );
    fs::write(args.out_dir.join("public_inputs.json"), &pi_json).map_err(CliError::io)?;

    eprintln!(
        "[gen-membership-proof] depth={} prover_index={} epoch={} → {}",
        args.depth,
        args.prover_index,
        args.epoch,
        args.out_dir.display()
    );
    Ok(())
}

fn build_witness(args: &Args) -> Result<MembershipWitness, CliError> {
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
            "{} secret keys exceed depth-{} tree capacity {}",
            args.secret_keys.len(),
            depth,
            num_leaves
        )));
    }

    let leaves: Vec<Fr> = args.secret_keys.iter().map(poseidon_hash_one_v05).collect();
    let mut nodes = vec![Fr::from(0u64); 2 * num_leaves];
    for (i, leaf) in leaves.iter().enumerate() {
        nodes[num_leaves + i] = *leaf;
    }
    for i in (1..num_leaves).rev() {
        nodes[i] = poseidon_hash_two_v05(&nodes[2 * i], &nodes[2 * i + 1]);
    }
    let root = nodes[1];

    let mut path = Vec::with_capacity(depth);
    let mut cur = num_leaves + args.prover_index;
    for _ in 0..depth {
        let sib = if cur % 2 == 0 { cur + 1 } else { cur - 1 };
        path.push(nodes[sib]);
        cur /= 2;
    }

    let salt_fr = Fr::from_le_bytes_mod_order(&args.salt);
    let inner = poseidon_hash_two_v05(&root, &Fr::from(args.epoch));
    let commitment = poseidon_hash_two_v05(&inner, &salt_fr);

    Ok(MembershipWitness {
        commitment,
        epoch: args.epoch,
        secret_key: args.secret_keys[args.prover_index],
        poseidon_root: root,
        salt: args.salt,
        merkle_path: path,
        leaf_index: args.prover_index,
        depth,
    })
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
    let mut prover_index: Option<usize> = None;
    let mut epoch: Option<u64> = None;
    let mut salt: Option<[u8; 32]> = None;
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
            "--prover-index" => {
                let s = raw.get(i + 1).ok_or_else(|| CliError::usage("--prover-index needs a value"))?;
                prover_index = Some(s.parse().map_err(|_| CliError::usage(format!(
                    "--prover-index must be an integer, got {s:?}"
                )))?);
                i += 2;
            }
            "--epoch" => {
                let s = raw.get(i + 1).ok_or_else(|| CliError::usage("--epoch needs a value"))?;
                epoch = Some(s.parse().map_err(|_| CliError::usage(format!(
                    "--epoch must be a u64, got {s:?}"
                )))?);
                i += 2;
            }
            "--salt" => {
                let s = raw.get(i + 1).ok_or_else(|| CliError::usage("--salt needs a value"))?;
                salt = Some(parse_salt_hex(s)?);
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

    let _ = parse_fr_hex; // keep helper imported

    Ok(Args {
        depth: depth.ok_or_else(|| CliError::usage("missing --depth"))?,
        secret_keys: secret_keys.ok_or_else(|| CliError::usage("missing --secret-keys"))?,
        prover_index: prover_index.ok_or_else(|| CliError::usage("missing --prover-index"))?,
        epoch: epoch.ok_or_else(|| CliError::usage("missing --epoch"))?,
        salt: salt.ok_or_else(|| CliError::usage("missing --salt"))?,
        out_dir: out_dir.ok_or_else(|| CliError::usage("missing --out-dir"))?,
    })
}

const USAGE: &str = "\
usage: gen-membership-proof
    --depth <5|8|11>
    --secret-keys <hex,hex,...>          (comma-separated 32-byte BE Fr scalars; 0x prefix optional)
    --prover-index <N>                   (the prover's position in the roster)
    --epoch <u64>
    --salt <hex>                         (32-byte salt; 0x prefix optional)
    --out-dir <path>                     (directory to write proof.bin / commitment.bin / etc.)";
