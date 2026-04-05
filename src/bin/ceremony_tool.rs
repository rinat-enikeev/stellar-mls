use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ark_bls12_381::{G1Affine, G2Affine};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use rand::rngs::OsRng;
use sep_xxxx_circuits::ceremony;
use sep_xxxx_circuits::ceremony::phase2;
use sep_xxxx_circuits::Tier;
use sha2::{Digest, Sha256};

const TOOL_VERSION: &str = "1";
const STATE_META: &str = "state.txt";
const STATE_SRS: &str = "state.srs";
const RECEIPT_META: &str = "receipt.txt";

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        eprintln!();
        print_usage();
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or("missing command")?;
    let rest: Vec<String> = args.collect();
    match command.as_str() {
        "init" => {
            let opts = parse_options(&rest)?;
            let tier = parse_tier(required_opt(&opts, "tier")?)?;
            let out_dir = PathBuf::from(required_opt(&opts, "out-dir")?);
            let participant = opts
                .get("participant")
                .cloned()
                .unwrap_or_else(|| "coordinator-init".to_string());
            init_state(tier, out_dir, participant)
        }
        "contribute" => {
            let opts = parse_options(&rest)?;
            let state_dir = PathBuf::from(required_opt(&opts, "state-dir")?);
            let out_dir = PathBuf::from(required_opt(&opts, "out-dir")?);
            let participant = required_opt(&opts, "participant")?.to_string();
            contribute_state(&state_dir, out_dir, participant)
        }
        "verify-contribution" => {
            let opts = parse_options(&rest)?;
            let before_state_dir = PathBuf::from(required_opt(&opts, "before-state-dir")?);
            let after_state_dir = PathBuf::from(required_opt(&opts, "after-state-dir")?);
            verify_contribution_artifacts(&before_state_dir, &after_state_dir)
        }
        "show-receipt" => {
            let opts = parse_options(&rest)?;
            let state_dir = PathBuf::from(required_opt(&opts, "state-dir")?);
            show_receipt(&state_dir)
        }
        "verify-state" => {
            let opts = parse_options(&rest)?;
            let state_dir = PathBuf::from(required_opt(&opts, "state-dir")?);
            verify_state(&state_dir)
        }
        "phase2-summary" => {
            let opts = parse_options(&rest)?;
            let state_dir = PathBuf::from(required_opt(&opts, "state-dir")?);
            let out_file = opts.get("out-file").map(PathBuf::from);
            phase2_summary(&state_dir, out_file)
        }
        "--help" | "-h" | "help" => {
            print_usage();
            Ok(())
        }
        _ => Err(format!("unknown command: {command}")),
    }
}

fn init_state(tier: Tier, out_dir: PathBuf, participant: String) -> Result<(), String> {
    validate_participant(&participant)?;
    ensure_fresh_output_dir(&out_dir)?;

    let degree = ceremony::required_degree(&tier);
    let (srs, proof, _tau, _alpha, _beta) = ceremony::initialize(degree, &mut OsRng);
    if !ceremony::verify_initial_contribution(&srs, &proof) {
        return Err("initial contribution failed self-verification".to_string());
    }

    let srs_hash = ceremony::hash_srs(&srs);
    let circuit_id = ceremony::compute_circuit_id(&tier);
    let contribution_id = contribution_id(&tier, 0, &srs_hash);
    let created_at = unix_timestamp();

    let state = vec![
        kv("tool_version", TOOL_VERSION),
        kv("tier", tier_name(&tier)),
        kv("round", "0"),
        kv("circuit_id", &hex_encode(&circuit_id)),
        kv("srs_hash", &hex_encode(&srs_hash)),
        kv("contribution_id", &contribution_id),
        kv("srs_file", STATE_SRS),
        kv("receipt_file", RECEIPT_META),
        kv("created_at", &created_at),
    ];
    let receipt = receipt_lines(
        "initial",
        &participant,
        tier_name(&tier),
        0,
        &hex_encode(&circuit_id),
        None,
        &hex_encode(&srs_hash),
        &contribution_id,
        &created_at,
        &proof,
    )?;

    write_state_dir(&out_dir, &srs, &state, &receipt)?;
    print_receipt_summary(tier_name(&tier), 0, &hex_encode(&srs_hash), &contribution_id, &participant);
    Ok(())
}

fn contribute_state(state_dir: &Path, out_dir: PathBuf, participant: String) -> Result<(), String> {
    validate_participant(&participant)?;
    ensure_fresh_output_dir(&out_dir)?;

    let (before_state, before_srs, _before_receipt) = load_state_dir(state_dir)?;
    let tier = parse_tier(state_value(&before_state, "tier")?)?;
    let before_round = parse_usize(state_value(&before_state, "round")?, "round")?;
    let before_hash = ceremony::hash_srs(&before_srs);
    if hex_encode(&before_hash) != state_value(&before_state, "srs_hash")? {
        return Err("before state hash does not match state.txt".to_string());
    }

    let (after_srs, proof, _dt, _da, _db) = ceremony::contribute(&before_srs, &mut OsRng);
    if !ceremony::verify_contribution(&before_srs, &after_srs, &proof) {
        return Err("contribution failed self-verification".to_string());
    }

    let round = before_round + 1;
    let after_hash = ceremony::hash_srs(&after_srs);
    let contribution_id = contribution_id(&tier, round, &after_hash);
    let created_at = unix_timestamp();
    let circuit_id = state_value(&before_state, "circuit_id")?.to_string();

    let state = vec![
        kv("tool_version", TOOL_VERSION),
        kv("tier", tier_name(&tier)),
        kv("round", &round.to_string()),
        kv("circuit_id", &circuit_id),
        kv("srs_hash", &hex_encode(&after_hash)),
        kv("contribution_id", &contribution_id),
        kv("srs_file", STATE_SRS),
        kv("receipt_file", RECEIPT_META),
        kv("created_at", &created_at),
    ];
    let receipt = receipt_lines(
        "contribution",
        &participant,
        tier_name(&tier),
        round,
        &circuit_id,
        Some(state_value(&before_state, "srs_hash")?),
        &hex_encode(&after_hash),
        &contribution_id,
        &created_at,
        &proof,
    )?;

    write_state_dir(&out_dir, &after_srs, &state, &receipt)?;
    print_receipt_summary(tier_name(&tier), round, &hex_encode(&after_hash), &contribution_id, &participant);
    Ok(())
}

fn verify_contribution_artifacts(before_state_dir: &Path, after_state_dir: &Path) -> Result<(), String> {
    let (before_state, before_srs, _before_receipt) = load_state_dir(before_state_dir)?;
    let (after_state, after_srs, after_receipt) = load_state_dir(after_state_dir)?;

    let before_tier = state_value(&before_state, "tier")?;
    let after_tier = state_value(&after_state, "tier")?;
    if before_tier != after_tier {
        return Err("tier mismatch between states".to_string());
    }

    let before_circuit_id = state_value(&before_state, "circuit_id")?;
    let after_circuit_id = state_value(&after_state, "circuit_id")?;
    if before_circuit_id != after_circuit_id {
        return Err("circuit_id mismatch between states".to_string());
    }

    let before_round = parse_usize(state_value(&before_state, "round")?, "round")?;
    let after_round = parse_usize(state_value(&after_state, "round")?, "round")?;
    if after_round != before_round + 1 {
        return Err(format!(
            "expected after round {} but found {}",
            before_round + 1,
            after_round
        ));
    }

    let before_hash = ceremony::hash_srs(&before_srs);
    let after_hash = ceremony::hash_srs(&after_srs);
    if hex_encode(&before_hash) != state_value(&before_state, "srs_hash")? {
        return Err("before state.txt has wrong srs_hash".to_string());
    }
    if hex_encode(&after_hash) != state_value(&after_state, "srs_hash")? {
        return Err("after state.txt has wrong srs_hash".to_string());
    }
    if receipt_value(&after_receipt, "before_srs_hash")? != state_value(&before_state, "srs_hash")? {
        return Err("receipt before_srs_hash does not match previous state".to_string());
    }
    if receipt_value(&after_receipt, "after_srs_hash")? != state_value(&after_state, "srs_hash")? {
        return Err("receipt after_srs_hash does not match after state".to_string());
    }
    if receipt_value(&after_receipt, "contribution_id")? != state_value(&after_state, "contribution_id")? {
        return Err("receipt contribution_id does not match after state".to_string());
    }

    let proof = proof_from_lines(&after_receipt)?;
    if !ceremony::verify_contribution(&before_srs, &after_srs, &proof) {
        return Err("pairing verification failed for contribution".to_string());
    }

    println!("verified contribution");
    println!("tier: {}", after_tier);
    println!("round: {}", after_round);
    println!("participant: {}", receipt_value(&after_receipt, "participant")?);
    println!("contribution_id: {}", state_value(&after_state, "contribution_id")?);
    Ok(())
}

fn show_receipt(state_dir: &Path) -> Result<(), String> {
    let (state, _srs, receipt) = load_state_dir(state_dir)?;
    print_receipt_summary(
        state_value(&state, "tier")?,
        parse_usize(state_value(&state, "round")?, "round")?,
        state_value(&state, "srs_hash")?,
        state_value(&state, "contribution_id")?,
        receipt_value(&receipt, "participant")?,
    );
    Ok(())
}

fn verify_state(state_dir: &Path) -> Result<(), String> {
    let (state, srs, receipt) = load_state_dir(state_dir)?;
    let round = parse_usize(state_value(&state, "round")?, "round")?;
    let srs_hash = ceremony::hash_srs(&srs);
    let expected_hash = state_value(&state, "srs_hash")?;
    if hex_encode(&srs_hash) != expected_hash {
        return Err("state.txt srs_hash does not match state.srs".to_string());
    }
    if !ceremony::verify_consistency(&srs) {
        return Err("state.srs failed consistency verification".to_string());
    }
    if receipt_value(&receipt, "after_srs_hash")? != expected_hash {
        return Err("receipt after_srs_hash does not match state.txt".to_string());
    }

    if round == 0 {
        let proof = proof_from_lines(&receipt)?;
        if !ceremony::verify_initial_contribution(&srs, &proof) {
            return Err("initial contribution proof verification failed".to_string());
        }
        println!("verified state");
        println!("kind: initial");
    } else {
        println!("verified state");
        println!("kind: contributed-state");
        println!("note: use verify-contribution with the previous round to verify the transition proof");
    }
    println!("tier: {}", state_value(&state, "tier")?);
    println!("round: {round}");
    println!("srs_hash: {expected_hash}");
    println!("contribution_id: {}", state_value(&state, "contribution_id")?);
    Ok(())
}

fn phase2_summary(state_dir: &Path, out_file: Option<PathBuf>) -> Result<(), String> {
    let (state, srs, receipt) = load_state_dir(state_dir)?;
    let round = parse_usize(state_value(&state, "round")?, "round")?;
    let srs_hash = ceremony::hash_srs(&srs);
    let expected_hash = state_value(&state, "srs_hash")?;
    if hex_encode(&srs_hash) != expected_hash {
        return Err("state.txt srs_hash does not match state.srs".to_string());
    }
    if !ceremony::verify_consistency(&srs) {
        return Err("state.srs failed consistency verification".to_string());
    }

    let state_srs_bytes = fs::read(state_dir.join(STATE_SRS))
        .map_err(|e| format!("failed to read {}: {e}", state_dir.join(STATE_SRS).display()))?;
    let state_srs_archive_sha256 = sha256_hex(&state_srs_bytes);

    let mut lines = vec![
        "# Phase 2 transparency summary".to_string(),
        format!("generated_at={}", unix_timestamp()),
        format!("tool_version={TOOL_VERSION}"),
        format!("trust_model_target=public-phase2-mpc"),
        format!("tier={}", state_value(&state, "tier")?),
        format!("round={round}"),
        format!("participant_of_latest_round={}", receipt_value(&receipt, "participant")?),
        format!("circuit_id={}", state_value(&state, "circuit_id")?),
        format!("phase1_srs_hash={expected_hash}"),
        format!("phase1_contribution_id={}", state_value(&state, "contribution_id")?),
        format!("phase1_state_archive_sha256={state_srs_archive_sha256}"),
        format!("phase1_state_archive_bytes={}", state_srs_bytes.len()),
        format!("phase1_state_dir={}", state_dir.display()),
        "recommended_publication=publish_this_summary_before_starting_public_phase2".to_string(),
        "recommended_outputs=publish_phase2_round_hashes_beacon_final_pk_hash_final_vk_hash_and_verification_artifacts".to_string(),
    ];
    lines.push(String::new());
    lines.push(String::from("Issue comment template:"));
    lines.push(format!(
        "We are freezing the final Phase 1 input for public Phase 2 MPC. tier={}, round={}, phase1_contribution_id={}, phase1_srs_hash={}, phase1_state_archive_sha256={}",
        state_value(&state, "tier")?,
        round,
        state_value(&state, "contribution_id")?,
        expected_hash,
        state_srs_archive_sha256
    ));
    let rendered = lines.join("\n") + "\n";

    if let Some(path) = out_file {
        fs::write(&path, rendered.as_bytes())
            .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
        println!("wrote {}", path.display());
    } else {
        print!("{rendered}");
    }
    Ok(())
}

fn load_state_dir(path: &Path) -> Result<(BTreeMap<String, String>, ceremony::PowersOfTau, BTreeMap<String, String>), String> {
    let state = read_kv_file(&path.join(STATE_META))?;
    let receipt = read_kv_file(&path.join(RECEIPT_META))?;
    if state_value(&state, "tool_version")? != TOOL_VERSION {
        return Err("unsupported tool_version in state.txt".to_string());
    }
    if receipt_value(&receipt, "tool_version")? != TOOL_VERSION {
        return Err("unsupported tool_version in receipt.txt".to_string());
    }
    let srs_bytes = fs::read(path.join(STATE_SRS))
        .map_err(|e| format!("failed to read {}: {e}", path.join(STATE_SRS).display()))?;
    let srs = phase2::import_srs(&srs_bytes)?;
    Ok((state, srs, receipt))
}

fn write_state_dir(
    out_dir: &Path,
    srs: &ceremony::PowersOfTau,
    state_lines: &[(String, String)],
    receipt_lines: &[(String, String)],
) -> Result<(), String> {
    fs::create_dir_all(out_dir)
        .map_err(|e| format!("failed to create {}: {e}", out_dir.display()))?;
    let exported = phase2::export_srs(srs)?;
    fs::write(out_dir.join(STATE_SRS), exported.srs_bytes)
        .map_err(|e| format!("failed to write {}: {e}", out_dir.join(STATE_SRS).display()))?;
    write_kv_file(&out_dir.join(STATE_META), state_lines)?;
    write_kv_file(&out_dir.join(RECEIPT_META), receipt_lines)?;
    Ok(())
}

fn ensure_fresh_output_dir(out_dir: &Path) -> Result<(), String> {
    if out_dir.exists() {
        let mut entries = fs::read_dir(out_dir)
            .map_err(|e| format!("failed to read {}: {e}", out_dir.display()))?;
        if entries.next().is_some() {
            return Err(format!("output directory {} is not empty", out_dir.display()));
        }
    }
    Ok(())
}

fn validate_participant(participant: &str) -> Result<(), String> {
    if participant.is_empty() {
        return Err("participant cannot be empty".to_string());
    }
    if participant.contains('\n') || participant.contains('\r') || participant.contains('=') {
        return Err("participant cannot contain newlines or '='".to_string());
    }
    Ok(())
}

fn receipt_lines(
    kind: &str,
    participant: &str,
    tier: &str,
    round: usize,
    circuit_id: &str,
    before_srs_hash: Option<&str>,
    after_srs_hash: &str,
    contribution_id: &str,
    created_at: &str,
    proof: &ceremony::ContributionProof,
) -> Result<Vec<(String, String)>, String> {
    Ok(vec![
        kv("tool_version", TOOL_VERSION),
        kv("kind", kind),
        kv("participant", participant),
        kv("tier", tier),
        kv("round", &round.to_string()),
        kv("circuit_id", circuit_id),
        kv("before_srs_hash", before_srs_hash.unwrap_or("")),
        kv("after_srs_hash", after_srs_hash),
        kv("contribution_id", contribution_id),
        kv("created_at", created_at),
        kv("tau_proof_g1", &serialize_g1_hex(&proof.tau_proof.0)?),
        kv("tau_proof_delta_g1", &serialize_g1_hex(&proof.tau_proof.1)?),
        kv("alpha_proof_g2", &serialize_g2_hex(&proof.alpha_proof.0)?),
        kv("alpha_proof_delta_g2", &serialize_g2_hex(&proof.alpha_proof.1)?),
        kv("beta_proof_g1", &serialize_g1_hex(&proof.beta_proof.0)?),
        kv("beta_proof_delta_g1", &serialize_g1_hex(&proof.beta_proof.1)?),
    ])
}

fn proof_from_lines(receipt: &BTreeMap<String, String>) -> Result<ceremony::ContributionProof, String> {
    Ok(ceremony::ContributionProof {
        tau_proof: (
            deserialize_g1_hex(receipt_value(receipt, "tau_proof_g1")?)?,
            deserialize_g1_hex(receipt_value(receipt, "tau_proof_delta_g1")?)?,
        ),
        alpha_proof: (
            deserialize_g2_hex(receipt_value(receipt, "alpha_proof_g2")?)?,
            deserialize_g2_hex(receipt_value(receipt, "alpha_proof_delta_g2")?)?,
        ),
        beta_proof: (
            deserialize_g1_hex(receipt_value(receipt, "beta_proof_g1")?)?,
            deserialize_g1_hex(receipt_value(receipt, "beta_proof_delta_g1")?)?,
        ),
    })
}

fn serialize_g1_hex(point: &G1Affine) -> Result<String, String> {
    let mut bytes = Vec::new();
    point.serialize_compressed(&mut bytes)
        .map_err(|e| format!("failed to serialize G1 point: {e}"))?;
    Ok(hex_encode(&bytes))
}

fn serialize_g2_hex(point: &G2Affine) -> Result<String, String> {
    let mut bytes = Vec::new();
    point.serialize_compressed(&mut bytes)
        .map_err(|e| format!("failed to serialize G2 point: {e}"))?;
    Ok(hex_encode(&bytes))
}

fn deserialize_g1_hex(value: &str) -> Result<G1Affine, String> {
    let bytes = hex_decode(value)?;
    G1Affine::deserialize_compressed(&bytes[..])
        .map_err(|e| format!("failed to deserialize G1 point: {e}"))
}

fn deserialize_g2_hex(value: &str) -> Result<G2Affine, String> {
    let bytes = hex_decode(value)?;
    G2Affine::deserialize_compressed(&bytes[..])
        .map_err(|e| format!("failed to deserialize G2 point: {e}"))
}

fn parse_options(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        let flag = &args[i];
        if !flag.starts_with("--") {
            return Err(format!("expected flag, got: {flag}"));
        }
        if i + 1 >= args.len() {
            return Err(format!("missing value for {flag}"));
        }
        out.insert(flag.trim_start_matches("--").to_string(), args[i + 1].clone());
        i += 2;
    }
    Ok(out)
}

fn required_opt<'a>(opts: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    opts.get(key)
        .map(|value| value.as_str())
        .ok_or_else(|| format!("missing --{key}"))
}

fn parse_tier(value: &str) -> Result<Tier, String> {
    match value {
        "small" => Ok(Tier::Small),
        "medium" => Ok(Tier::Medium),
        "large" => Ok(Tier::Large),
        _ => Err(format!("unknown tier: {value}")),
    }
}

fn tier_name(tier: &Tier) -> &'static str {
    match tier {
        Tier::Small => "small",
        Tier::Medium => "medium",
        Tier::Large => "large",
    }
}

fn contribution_id(tier: &Tier, round: usize, srs_hash: &[u8; 32]) -> String {
    format!(
        "sepceremony1:{}:r{}:{}",
        tier_name(tier),
        round,
        hex_encode(srs_hash)
    )
}

fn print_receipt_summary(tier: &str, round: usize, srs_hash: &str, contribution_id: &str, participant: &str) {
    println!("tier: {tier}");
    println!("round: {round}");
    println!("srs_hash: {srs_hash}");
    println!("contribution_id: {contribution_id}");
    println!("participant: {participant}");
    println!();
    println!("Issue comment line:");
    println!(
        "I contributed to the ceremony. participant={participant}, round={round}, contribution_id={contribution_id}"
    );
}

fn kv(key: &str, value: &str) -> (String, String) {
    (key.to_string(), value.to_string())
}

fn write_kv_file(path: &Path, lines: &[(String, String)]) -> Result<(), String> {
    let mut out = String::new();
    for (key, value) in lines {
        out.push_str(key);
        out.push('=');
        out.push_str(value);
        out.push('\n');
    }
    fs::write(path, out).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

fn read_kv_file(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let data = fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let mut out = BTreeMap::new();
    for (index, line) in data.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("invalid line {} in {}: {}", index + 1, path.display(), line));
        };
        out.insert(key.to_string(), value.to_string());
    }
    Ok(out)
}

fn state_value<'a>(state: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    state
        .get(key)
        .map(|value| value.as_str())
        .ok_or_else(|| format!("missing key '{key}' in state.txt"))
}

fn receipt_value<'a>(receipt: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    receipt
        .get(key)
        .map(|value| value.as_str())
        .ok_or_else(|| format!("missing key '{key}' in receipt.txt"))
}

fn parse_usize(value: &str, field: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|e| format!("invalid {field}: {e}"))
}

fn unix_timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("unix:{}", duration.as_secs()),
        Err(_) => "unknown".to_string(),
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hex_encode(&hash)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(nibble_to_hex(byte >> 4));
        out.push(nibble_to_hex(byte & 0x0f));
    }
    out
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 {
        return Err("hex string has odd length".to_string());
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let high = hex_value(bytes[i])?;
        let low = hex_value(bytes[i + 1])?;
        out.push((high << 4) | low);
        i += 2;
    }
    Ok(out)
}

fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => unreachable!(),
    }
}

fn hex_value(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(format!("invalid hex character: {}", byte as char)),
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  cargo run --bin ceremony_tool -- init --tier <small|medium|large> --out-dir <dir> [--participant <name>]");
    eprintln!("  cargo run --bin ceremony_tool -- contribute --state-dir <dir> --out-dir <dir> --participant <name>");
    eprintln!("  cargo run --bin ceremony_tool -- verify-contribution --before-state-dir <dir> --after-state-dir <dir>");
    eprintln!("  cargo run --bin ceremony_tool -- show-receipt --state-dir <dir>");
    eprintln!("  cargo run --bin ceremony_tool -- verify-state --state-dir <dir>");
    eprintln!("  cargo run --bin ceremony_tool -- phase2-summary --state-dir <dir> [--out-file <path>]");
}
