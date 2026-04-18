use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, OptionalExtension};

use crate::model::{now_unix, Round, Signup, SignupStatus, Tier};

pub type Conn = r2d2::PooledConnection<SqliteConnectionManager>;

#[derive(Clone)]
pub struct Store {
    pool: Pool<SqliteConnectionManager>,
    pub db_path: PathBuf,
}

impl Store {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let manager = SqliteConnectionManager::file(db_path).with_init(|c| {
            c.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
        });
        let pool = Pool::builder()
            .max_size(8)
            .build(manager)
            .context("build sqlite pool")?;
        Ok(Self {
            pool,
            db_path: db_path.to_path_buf(),
        })
    }

    pub fn get(&self) -> Result<Conn> {
        Ok(self.pool.get()?)
    }

    pub fn run_migrations(&self) -> Result<()> {
        let conn = self.get()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
             );",
        )?;
        let applied: std::collections::BTreeSet<i64> = conn
            .prepare("SELECT version FROM schema_migrations")?
            .query_map([], |r| r.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .collect();

        let migrations: &[(i64, &str)] = &[
            (1, include_str!("../migrations/001_init.sql")),
            (2, include_str!("../migrations/002_phase2_state.sql")),
        ];

        for (v, sql) in migrations {
            if applied.contains(v) {
                continue;
            }
            conn.execute_batch(sql)
                .with_context(|| format!("apply migration {v}"))?;
            conn.execute(
                "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
                params![v, now_unix()],
            )?;
        }
        Ok(())
    }

    pub fn upsert_participant(&self, pubkey: &str, display_name: Option<&str>) -> Result<()> {
        let conn = self.get()?;
        conn.execute(
            "INSERT INTO participants(pubkey, display_name, first_seen_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(pubkey) DO UPDATE SET display_name = COALESCE(excluded.display_name, display_name)",
            params![pubkey, display_name, now_unix()],
        )?;
        Ok(())
    }

    pub fn insert_signup(&self, pubkey: &str, tier: Tier) -> Result<i64> {
        let conn = self.get()?;
        conn.execute(
            "INSERT INTO signups(pubkey, tier, joined_at, status)
             VALUES (?1, ?2, ?3, 'queued')",
            params![pubkey, tier.as_str(), now_unix()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn queue_position(&self, signup_id: i64) -> Result<i64> {
        let conn = self.get()?;
        let pos: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM signups
                 WHERE tier = (SELECT tier FROM signups WHERE id = ?1)
                   AND status = 'queued'
                   AND joined_at <= (SELECT joined_at FROM signups WHERE id = ?1)",
                params![signup_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        Ok(pos)
    }

    pub fn queue_for_tier(&self, tier: Tier) -> Result<Vec<Signup>> {
        let conn = self.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, pubkey, tier, joined_at, status, slot_claimed_at, slot_deadline, retry_count
             FROM signups
             WHERE tier = ?1 AND status = 'queued'
             ORDER BY joined_at ASC",
        )?;
        let rows = stmt
            .query_map(params![tier.as_str()], row_to_signup)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn current_claim(&self, tier: Tier) -> Result<Option<Signup>> {
        let conn = self.get()?;
        let s = conn
            .query_row(
                "SELECT id, pubkey, tier, joined_at, status, slot_claimed_at, slot_deadline, retry_count
                 FROM signups
                 WHERE tier = ?1 AND status = 'claimed'
                 ORDER BY slot_claimed_at DESC
                 LIMIT 1",
                params![tier.as_str()],
                row_to_signup,
            )
            .optional()?;
        Ok(s)
    }

    pub fn head_round(&self, tier: Tier) -> Result<i64> {
        let conn = self.get()?;
        let head: Option<i64> = conn
            .query_row(
                "SELECT MAX(round) FROM rounds WHERE tier = ?1",
                params![tier.as_str()],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(head.unwrap_or(-1))
    }

    pub fn get_round(&self, tier: Tier, round: i64) -> Result<Option<Round>> {
        let conn = self.get()?;
        let r = conn
            .query_row(
                "SELECT tier, round, contribution_id, circuit_id, srs_hash, state_txt_hash,
                        receipt_hash, participant_pk, participant_label, nostr_event_id,
                        prev_nostr_event_id, created_at, verified_ok
                 FROM rounds WHERE tier = ?1 AND round = ?2",
                params![tier.as_str(), round],
                row_to_round,
            )
            .optional()?;
        Ok(r)
    }

    pub fn list_rounds(&self, tier: Tier) -> Result<Vec<Round>> {
        let conn = self.get()?;
        let mut stmt = conn.prepare(
            "SELECT tier, round, contribution_id, circuit_id, srs_hash, state_txt_hash,
                    receipt_hash, participant_pk, participant_label, nostr_event_id,
                    prev_nostr_event_id, created_at, verified_ok
             FROM rounds WHERE tier = ?1 ORDER BY round ASC",
        )?;
        let rows = stmt
            .query_map(params![tier.as_str()], row_to_round)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn queue_depth(&self, tier: Tier) -> Result<i64> {
        let conn = self.get()?;
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM signups WHERE tier = ?1 AND status = 'queued'",
            params![tier.as_str()],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    pub fn log_event(
        &self,
        kind: &str,
        tier: Option<Tier>,
        round: Option<i64>,
        pubkey: Option<&str>,
        detail_json: Option<&str>,
    ) -> Result<()> {
        let conn = self.get()?;
        conn.execute(
            "INSERT INTO events(at, kind, tier, round, pubkey, detail_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                now_unix(),
                kind,
                tier.map(|t| t.as_str()),
                round,
                pubkey,
                detail_json,
            ],
        )?;
        Ok(())
    }

    pub fn mark_nip98_seen(&self, event_id: &str) -> Result<bool> {
        let conn = self.get()?;
        let changed = conn.execute(
            "INSERT OR IGNORE INTO nip98_replay(event_id, seen_at) VALUES (?1, ?2)",
            params![event_id, now_unix()],
        )?;
        Ok(changed == 1)
    }

    /// Returns the most recent signup row for this participant (across all
    /// tiers), preferring an active `claimed` slot, then `queued`, then any
    /// committed history. Used by the frontend to render the participant's
    /// current state on the contribute page.
    pub fn participant_signup(&self, pubkey: &str) -> Result<Option<Signup>> {
        let conn = self.get()?;
        let s = conn
            .query_row(
                "SELECT id, pubkey, tier, joined_at, status, slot_claimed_at, slot_deadline, retry_count
                 FROM signups
                 WHERE pubkey = ?1
                 ORDER BY
                   CASE status
                     WHEN 'claimed'   THEN 0
                     WHEN 'queued'    THEN 1
                     WHEN 'committed' THEN 2
                     ELSE 3
                   END,
                   joined_at DESC
                 LIMIT 1",
                params![pubkey],
                row_to_signup,
            )
            .optional()?;
        Ok(s)
    }

    /// Position of the participant's active queued signup in their tier (1-indexed)
    /// or None if they're not currently queued.
    pub fn participant_queue_position(&self, pubkey: &str, tier: Tier) -> Result<Option<i64>> {
        let conn = self.get()?;
        let pos: Option<i64> = conn
            .query_row(
                "SELECT COUNT(*) FROM signups s1
                 WHERE s1.tier = ?1 AND s1.status = 'queued'
                   AND s1.joined_at <= (
                     SELECT joined_at FROM signups
                     WHERE pubkey = ?2 AND tier = ?1 AND status = 'queued'
                     LIMIT 1
                   )",
                params![tier.as_str(), pubkey],
                |r| r.get(0),
            )
            .optional()?;
        Ok(pos.filter(|n| *n > 0))
    }

    /// Return the most recent committed round for `pubkey` within `tier`.
    pub fn participant_committed_round(&self, pubkey: &str, tier: Tier) -> Result<Option<i64>> {
        let conn = self.get()?;
        let r: Option<i64> = conn
            .query_row(
                "SELECT MAX(round) FROM rounds WHERE tier = ?1 AND participant_pk = ?2",
                params![tier.as_str(), pubkey],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(r)
    }

    pub fn cleanup_replay(&self, older_than_secs: i64) -> Result<()> {
        let conn = self.get()?;
        conn.execute(
            "DELETE FROM nip98_replay WHERE seen_at < ?1",
            params![now_unix() - older_than_secs],
        )?;
        Ok(())
    }

    // ----- Phase 2 state + rounds -----

    pub fn ceremony_state(&self) -> Result<CeremonyState> {
        let conn = self.get()?;
        let s = conn
            .query_row(
                "SELECT phase1_frozen, phase1_frozen_at, phase1_frozen_by_pk,
                        beacon_block_height, beacon_block_hash, beacon_applied_at, note
                 FROM ceremony_state WHERE id = 1",
                [],
                |r| {
                    Ok(CeremonyState {
                        phase1_frozen: r.get::<_, i64>(0)? != 0,
                        phase1_frozen_at: r.get(1)?,
                        phase1_frozen_by_pk: r.get(2)?,
                        beacon_block_height: r.get(3)?,
                        beacon_block_hash: r.get(4)?,
                        beacon_applied_at: r.get(5)?,
                        note: r.get(6)?,
                    })
                },
            )
            .optional()?;
        Ok(s.unwrap_or_default())
    }

    pub fn freeze_phase1(&self, frozen_by_pk: &str, note: Option<&str>) -> Result<()> {
        let conn = self.get()?;
        conn.execute(
            "UPDATE ceremony_state
             SET phase1_frozen = 1, phase1_frozen_at = ?1,
                 phase1_frozen_by_pk = ?2, note = COALESCE(?3, note)
             WHERE id = 1",
            params![now_unix(), frozen_by_pk, note],
        )?;
        Ok(())
    }

    pub fn set_beacon(&self, height: i64, block_hash: Option<&str>) -> Result<()> {
        let conn = self.get()?;
        conn.execute(
            "UPDATE ceremony_state
             SET beacon_block_height = ?1,
                 beacon_block_hash = ?2,
                 beacon_applied_at = CASE WHEN ?2 IS NULL THEN NULL ELSE ?3 END
             WHERE id = 1",
            params![height, block_hash, now_unix()],
        )?;
        Ok(())
    }

    pub fn phase2_rounds(&self) -> Result<Vec<Phase2Round>> {
        let conn = self.get()?;
        let mut stmt = conn.prepare(
            "SELECT tier, round, participant_pk, zkey_hash, zkey_blob_sha256,
                    attestation_hash, nostr_event_id, created_at, beacon_applied
             FROM phase2_rounds
             ORDER BY tier ASC, round ASC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let tier_str: String = r.get(0)?;
                let tier: Tier = tier_str.parse().map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        format!("{e}").into(),
                    )
                })?;
                Ok(Phase2Round {
                    tier,
                    round: r.get(1)?,
                    participant_pk: r.get(2)?,
                    zkey_hash: r.get(3)?,
                    zkey_blob_sha256: r.get(4)?,
                    attestation_hash: r.get(5)?,
                    nostr_event_id: r.get(6)?,
                    created_at: r.get(7)?,
                    beacon_applied: r.get::<_, i64>(8)? != 0,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

#[derive(Default, Clone, Debug, serde::Serialize)]
pub struct CeremonyState {
    pub phase1_frozen: bool,
    pub phase1_frozen_at: Option<i64>,
    pub phase1_frozen_by_pk: Option<String>,
    pub beacon_block_height: Option<i64>,
    pub beacon_block_hash: Option<String>,
    pub beacon_applied_at: Option<i64>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Phase2Round {
    pub tier: Tier,
    pub round: i64,
    pub participant_pk: Option<String>,
    pub zkey_hash: Option<String>,
    pub zkey_blob_sha256: Option<String>,
    pub attestation_hash: Option<String>,
    pub nostr_event_id: Option<String>,
    pub created_at: i64,
    pub beacon_applied: bool,
}

fn row_to_signup(row: &rusqlite::Row) -> rusqlite::Result<Signup> {
    let status_str: String = row.get(4)?;
    let status = match status_str.as_str() {
        "queued" => SignupStatus::Queued,
        "claimed" => SignupStatus::Claimed,
        "committed" => SignupStatus::Committed,
        "expired" => SignupStatus::Expired,
        "skipped" => SignupStatus::Skipped,
        "withdrawn" => SignupStatus::Withdrawn,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                format!("unknown signup status: {other}").into(),
            ))
        }
    };
    let tier_str: String = row.get(2)?;
    let tier: Tier = tier_str
        .parse()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, format!("{e}").into()))?;
    Ok(Signup {
        id: row.get(0)?,
        pubkey: row.get(1)?,
        tier,
        joined_at: row.get(3)?,
        status,
        slot_claimed_at: row.get(5)?,
        slot_deadline: row.get(6)?,
        retry_count: row.get(7)?,
    })
}

fn row_to_round(row: &rusqlite::Row) -> rusqlite::Result<Round> {
    let tier_str: String = row.get(0)?;
    let tier: Tier = tier_str
        .parse()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, format!("{e}").into()))?;
    Ok(Round {
        tier,
        round: row.get(1)?,
        contribution_id: row.get(2)?,
        circuit_id: row.get(3)?,
        srs_hash: row.get(4)?,
        state_txt_hash: row.get(5)?,
        receipt_hash: row.get(6)?,
        participant_pk: row.get(7)?,
        participant_label: row.get(8)?,
        nostr_event_id: row.get(9)?,
        prev_nostr_event_id: row.get(10)?,
        created_at: row.get(11)?,
        verified_ok: row.get::<_, i64>(12)? != 0,
    })
}
