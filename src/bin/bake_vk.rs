//! Bake a deterministic verifying-key byte file for a Soroban
//! contract's per-tier embedded VK.
//!
//! Phase C.2 contracts include the output via:
//!
//! ```rust,ignore
//! pub const VK_BYTES: &[u8] = include_bytes!("membership-d11.vk.bin");
//! ```
//!
//! Pipeline:
//! ```text
//!     cargo run --bin bake-vk --features bake-vk-tool --release -- \
//!         --circuit membership --depth 11 \
//!         --out contracts/sep-anarchy/src/vk/membership-d11.vk.bin
//! ```
//!
//! Writes `vk.serialize_uncompressed()` bytes (no header). Prints the
//! SHA-256 of the output to stderr; the binary fails with exit code 2
//! if that SHA-256 doesn't match the pinned `VK_SHA256_HEX_*` constants
//! in [`crate::circuit::plonk::baker`] — guaranteeing the on-chain VK
//! and the cross-platform-fingerprint tests stay in lock-step.
//!
//! The `--circuit` flag is currently `membership`-only; future
//! tier-aware circuits (the `update_*` circuits per the migration plan)
//! will register additional values here.

#![cfg(feature = "plonk")]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use sep_xxxx_circuits::circuit::plonk::baker::{
    bake_membership_vk, pinned_vk_sha256_hex, vk_sha256_hex,
};

#[derive(Debug)]
enum Circuit {
    Membership,
}

impl Circuit {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "membership" => Ok(Self::Membership),
            other => Err(format!(
                "unknown --circuit {other:?} (supported: membership)"
            )),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Membership => "membership",
        }
    }
}

struct Args {
    circuit: Circuit,
    depth: usize,
    out_path: PathBuf,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(BakeCliError::Bake(e)) => {
            eprintln!("bake error: {e}");
            ExitCode::FAILURE
        }
        Err(BakeCliError::Io(e)) => {
            eprintln!("io error: {e}");
            ExitCode::FAILURE
        }
        Err(BakeCliError::Usage(s)) => {
            eprintln!("usage error: {s}");
            eprintln!();
            eprintln!("usage: bake-vk --circuit <name> --depth <n> --out <path>");
            eprintln!("  <name>: membership");
            eprintln!("  <n>:    5, 8, or 11");
            ExitCode::FAILURE
        }
        Err(BakeCliError::ShaDrift {
            circuit,
            depth,
            computed,
            pinned,
        }) => {
            eprintln!(
                "fatal: bake of circuit={circuit} depth={depth} produced \
                 SHA-256 {computed} but pinned baker constant is {pinned}. \
                 Either the circuit shape changed or the canonical witness \
                 changed; update VK_SHA256_HEX_* in baker.rs and \
                 docs/cross-platform-test-vectors.json before re-running."
            );
            ExitCode::from(2)
        }
    }
}

#[derive(Debug)]
enum BakeCliError {
    Usage(String),
    Bake(sep_xxxx_circuits::circuit::plonk::baker::BakeError),
    Io(std::io::Error),
    ShaDrift {
        circuit: &'static str,
        depth: usize,
        computed: String,
        pinned: &'static str,
    },
}

fn run() -> Result<(), BakeCliError> {
    let args = parse_args()?;

    let bytes = match args.circuit {
        Circuit::Membership => bake_membership_vk(args.depth).map_err(BakeCliError::Bake)?,
    };

    let computed = vk_sha256_hex(&bytes);
    let pinned = pinned_vk_sha256_hex(args.depth)
        .expect("pinned constant exists for the same depth set bake_membership_vk accepts");
    if computed != pinned {
        return Err(BakeCliError::ShaDrift {
            circuit: args.circuit.as_str(),
            depth: args.depth,
            computed,
            pinned,
        });
    }

    if let Some(parent) = args.out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(BakeCliError::Io)?;
        }
    }
    fs::write(&args.out_path, &bytes).map_err(BakeCliError::Io)?;

    eprintln!(
        "[bake-vk] circuit={} depth={} bytes={} sha256={}",
        args.circuit.as_str(),
        args.depth,
        bytes.len(),
        computed
    );
    eprintln!("[bake-vk] wrote {}", args.out_path.display());
    Ok(())
}

fn parse_args() -> Result<Args, BakeCliError> {
    let raw: Vec<String> = env::args().collect();
    let mut circuit: Option<String> = None;
    let mut depth: Option<usize> = None;
    let mut out_path: Option<PathBuf> = None;

    let mut i = 1;
    while i < raw.len() {
        let arg = &raw[i];
        match arg.as_str() {
            "--circuit" => {
                circuit = Some(
                    raw.get(i + 1)
                        .cloned()
                        .ok_or_else(|| BakeCliError::Usage("--circuit needs a value".into()))?,
                );
                i += 2;
            }
            "--depth" => {
                let s = raw
                    .get(i + 1)
                    .ok_or_else(|| BakeCliError::Usage("--depth needs a value".into()))?;
                depth = Some(s.parse::<usize>().map_err(|_| {
                    BakeCliError::Usage(format!("--depth must be an integer, got {s:?}"))
                })?);
                i += 2;
            }
            "--out" => {
                out_path = Some(PathBuf::from(
                    raw.get(i + 1)
                        .ok_or_else(|| BakeCliError::Usage("--out needs a value".into()))?,
                ));
                i += 2;
            }
            other => {
                return Err(BakeCliError::Usage(format!("unknown arg {other:?}")));
            }
        }
    }

    let circuit_name = circuit.ok_or_else(|| BakeCliError::Usage("missing --circuit".into()))?;
    let circuit = Circuit::parse(&circuit_name).map_err(BakeCliError::Usage)?;
    let depth = depth.ok_or_else(|| BakeCliError::Usage("missing --depth".into()))?;
    let out_path = out_path.ok_or_else(|| BakeCliError::Usage("missing --out".into()))?;

    Ok(Args {
        circuit,
        depth,
        out_path,
    })
}
