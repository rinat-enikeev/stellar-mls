//! Background task that keeps the per-tier queues moving.
//!
//! Handles two stall modes:
//!
//!   1. A participant claimed a slot but never uploaded. Their `slot_deadline`
//!      is checked lazily at the next claim attempt (`handlers/slot.rs`), so
//!      without this reaper the `claimed` row lingers and the queue looks
//!      "someone is working" to observers. We flip it to `expired` so the UI
//!      reflects reality and the next claim succeeds cleanly.
//!
//!   2. The head of the queue enrolled but never called `/claim`. Only the
//!      head can claim (FIFO), so everyone behind them is frozen until they
//!      act. We treat them as idle if they've been head-of-queue longer than
//!      `CEREMONY_QUEUE_IDLE_SECS`, counting from whichever came later:
//!      their own `joined_at`, the most recent round commit on the tier, or
//!      the deadline of the most recently-expired claim (the moment the slot
//!      actually became free for them). This avoids expiring someone the
//!      instant they inherit headship from a long-departed predecessor.
//!
//! Runs every `CEREMONY_REAPER_INTERVAL_SECS` seconds (default 60).

use std::time::Duration;

use rusqlite::{params, OptionalExtension};
use tokio::time::interval;
use tracing::{error, info, warn};

use crate::model::{now_unix, Tier};
use crate::SharedState;

pub fn spawn(state: SharedState) {
    let period = Duration::from_secs(state.config.reaper_interval_secs.max(5));
    info!(
        interval_secs = state.config.reaper_interval_secs,
        queue_idle_secs = state.config.queue_idle_secs,
        slot_deadline_secs = state.config.slot_deadline_secs,
        "spawning queue reaper"
    );
    tokio::spawn(async move {
        let mut ticker = interval(period);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if let Err(e) = reap_once(&state).await {
                error!(error = %e, "reaper iteration failed");
            }
        }
    });
}

async fn reap_once(state: &SharedState) -> anyhow::Result<()> {
    for tier in Tier::ALL {
        let lock = state.queues.lock_for(tier);
        let _guard = lock.lock().await;
        if let Err(e) = reap_tier(state, tier) {
            warn!(%tier, error = %e, "reap_tier failed");
        }
    }
    Ok(())
}

fn reap_tier(state: &SharedState, tier: Tier) -> anyhow::Result<()> {
    let conn = state.store.get()?;
    let now = now_unix();
    let tier_s = tier.as_str();

    // 1. Expire stale `claimed` rows. Collect first so we can log per-row and
    //    also compute the most recent claim-end moment for the head check.
    let mut stmt = conn.prepare(
        "SELECT id, pubkey, slot_deadline
           FROM signups
          WHERE tier = ?1 AND status = 'claimed'
            AND slot_deadline IS NOT NULL
            AND slot_deadline < ?2",
    )?;
    let stale: Vec<(i64, String, i64)> = stmt
        .query_map(params![tier_s, now], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    let mut last_claim_end: i64 = 0;
    for (id, pubkey, deadline) in &stale {
        conn.execute(
            "UPDATE signups SET status = 'expired'
              WHERE id = ?1 AND status = 'claimed'",
            params![id],
        )?;
        info!(%tier, %pubkey, deadline, "reaped stale claimed slot");
        state
            .store
            .log_event("slot_expired", Some(tier), None, Some(pubkey), None)
            .ok();
        if *deadline > last_claim_end {
            last_claim_end = *deadline;
        }
    }

    // If anyone's claim is still live, head-of-queue isn't actually blocked.
    let active_claim: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM signups
              WHERE tier = ?1 AND status = 'claimed'
                AND slot_deadline IS NOT NULL AND slot_deadline > ?2
              LIMIT 1",
            params![tier_s, now],
            |r| r.get(0),
        )
        .optional()?;
    if active_claim.is_some() {
        return Ok(());
    }

    // 2. Head-of-queue idle check.
    let latest_round_at: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(created_at), 0) FROM rounds WHERE tier = ?1",
            params![tier_s],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);

    let head: Option<(i64, String, i64)> = conn
        .query_row(
            "SELECT id, pubkey, joined_at
               FROM signups
              WHERE tier = ?1 AND status = 'queued'
              ORDER BY joined_at ASC
              LIMIT 1",
            params![tier_s],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;

    let Some((id, pubkey, joined_at)) = head else {
        return Ok(());
    };

    let became_head_at = joined_at.max(latest_round_at).max(last_claim_end);
    let idle_deadline = became_head_at + state.config.queue_idle_secs;
    if idle_deadline >= now {
        return Ok(());
    }

    conn.execute(
        "UPDATE signups SET status = 'expired'
          WHERE id = ?1 AND status = 'queued'",
        params![id],
    )?;
    info!(
        %tier, %pubkey, became_head_at, idle_deadline, now,
        "reaped idle head-of-queue signup"
    );
    state
        .store
        .log_event("queue_idle_expired", Some(tier), None, Some(&pubkey), None)
        .ok();

    Ok(())
}
