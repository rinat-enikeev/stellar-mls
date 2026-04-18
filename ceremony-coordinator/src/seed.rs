//! One-shot seed subcommand.
//!
//! The coordinator refuses slot claims until round 0 exists for the tier
//! (see handlers/slot.rs). `ceremony-coordinator seed <tier>` initialises
//! round 0: shells out to `ceremony_tool init`, uploads the three files to
//! Blossom, inserts the row, and optionally publishes a kind-30078 Nostr
//! event. Idempotent — re-running a seeded tier exits 0 with a message.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::str::FromStr;

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::params;
use tokio::process::Command;
use uuid::Uuid;

use crate::blossom::BlossomClient;
use crate::config::Config;
use crate::model::{now_unix, Tier};
use crate::nostr::{NostrPublisher, UnsignedEvent};
use crate::store::Store;

pub async fn run(tier_str: &str) -> Result<()> {
    let tier = Tier::from_str(tier_str).map_err(|e| anyhow!(e))?;
    let config = Config::from_env()?;

    let store = Store::open(&config.db_path)?;
    store.run_migrations()?;

    if store.head_round(tier)? >= 0 {
        println!(
            "tier {} already seeded (head_round={}); nothing to do",
            tier,
            store.head_round(tier)?
        );
        return Ok(());
    }

    let work_root = config.work_dir.join(format!("seed-{}-{}", tier, Uuid::new_v4()));
    tokio::fs::create_dir_all(&work_root)
        .await
        .with_context(|| format!("mkdir {}", work_root.display()))?;
    let out_dir = work_root.join("round0");

    let status = Command::new(&config.ceremony_tool_bin)
        .arg("init")
        .arg("--tier")
        .arg(tier.as_str())
        .arg("--out-dir")
        .arg(&out_dir)
        .arg("--participant")
        .arg("coordinator-seed")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("spawn ceremony_tool init")?;
    if !status.success() {
        let _ = tokio::fs::remove_dir_all(&work_root).await;
        bail!("ceremony_tool init exited {:?}", status.code());
    }

    let srs_bytes = tokio::fs::read(out_dir.join("state.srs")).await?;
    let state_txt = tokio::fs::read(out_dir.join("state.txt")).await?;
    let receipt_txt = tokio::fs::read(out_dir.join("receipt.txt")).await?;

    let kv = parse_kv(std::str::from_utf8(&state_txt).context("state.txt not utf-8")?);
    let srs_hash = kv.get("srs_hash").cloned().ok_or_else(|| anyhow!("state.txt missing srs_hash"))?;
    let contribution_id = kv
        .get("contribution_id")
        .cloned()
        .ok_or_else(|| anyhow!("state.txt missing contribution_id"))?;
    let circuit_id = kv
        .get("circuit_id")
        .cloned()
        .ok_or_else(|| anyhow!("state.txt missing circuit_id"))?;

    let blossom = BlossomClient::new(config.blossom_url.clone(), config.blossom_public_url.clone());
    let srs_blob = blossom
        .put(srs_bytes, "application/octet-stream")
        .await
        .context("blossom put state.srs")?;
    let txt_blob = blossom
        .put(state_txt.clone(), "text/plain")
        .await
        .context("blossom put state.txt")?;
    let receipt_blob = blossom
        .put(receipt_txt.clone(), "text/plain")
        .await
        .context("blossom put receipt.txt")?;

    let nostr = NostrPublisher::new(config.nostr_relay.clone(), config.coordinator_nsec.clone());
    let nostr_event_id = publish_round0_event(
        &nostr,
        tier,
        &contribution_id,
        &circuit_id,
        &srs_hash,
        &srs_blob.sha256_hex,
        &srs_blob.public_url,
        &txt_blob.sha256_hex,
        &txt_blob.public_url,
        &receipt_blob.sha256_hex,
        &receipt_blob.public_url,
        std::str::from_utf8(&receipt_txt).unwrap_or(""),
    );

    let conn = store.get()?;
    conn.execute(
        "INSERT INTO rounds(tier, round, contribution_id, circuit_id, srs_hash,
                            state_txt_hash, receipt_hash, participant_pk,
                            participant_label, nostr_event_id, prev_nostr_event_id,
                            created_at, verified_ok)
         VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6, 'coordinator-seed', NULL, ?7, NULL, ?8, 1)",
        params![
            tier.as_str(),
            contribution_id,
            circuit_id,
            srs_hash,
            txt_blob.sha256_hex,
            receipt_blob.sha256_hex,
            nostr_event_id,
            now_unix(),
        ],
    )
    .context("insert rounds row")?;

    store.log_event("seed", Some(tier), Some(0), None, None).ok();

    // Give the Nostr publisher task a beat to drain before the process exits.
    if nostr_event_id.is_some() {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    let _ = tokio::fs::remove_dir_all(&work_root).await;

    println!("seeded {} round 0", tier);
    println!("  srs_hash         {}", srs_hash);
    println!("  contribution_id  {}", contribution_id);
    println!("  state.srs blob   {}", srs_blob.public_url);
    println!("  state.txt blob   {}", txt_blob.public_url);
    println!("  receipt.txt blob {}", receipt_blob.public_url);
    if let Some(id) = &nostr_event_id {
        println!("  nostr_event_id   {}", id);
    } else {
        println!("  nostr_event_id   — (CEREMONY_COORDINATOR_NSEC unset)");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn publish_round0_event(
    nostr: &NostrPublisher,
    tier: Tier,
    contribution_id: &str,
    circuit_id: &str,
    srs_hash: &str,
    srs_file_sha: &str,
    srs_url: &str,
    txt_file_sha: &str,
    txt_url: &str,
    receipt_file_sha: &str,
    receipt_url: &str,
    receipt_content: &str,
) -> Option<String> {
    let tags: Vec<Vec<String>> = vec![
        vec!["d".into(), format!("sepceremony1:{}:r0", tier.as_str())],
        vec!["tier".into(), tier.as_str().into()],
        vec!["round".into(), "0".into()],
        vec!["circuit_id".into(), circuit_id.into()],
        vec!["srs_hash".into(), srs_hash.into()],
        vec!["contribution_id".into(), contribution_id.into()],
        vec!["blob".into(), "state.srs".into(), srs_file_sha.into(), srs_url.into()],
        vec!["blob".into(), "state.txt".into(), txt_file_sha.into(), txt_url.into()],
        vec!["blob".into(), "receipt.txt".into(), receipt_file_sha.into(), receipt_url.into()],
    ];
    nostr
        .publish(UnsignedEvent {
            kind: 30078,
            tags,
            content: receipt_content.to_string(),
        })
        .ok()
        .flatten()
}

fn parse_kv(s: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in s.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.insert(k.to_string(), v.to_string());
        }
    }
    out
}
