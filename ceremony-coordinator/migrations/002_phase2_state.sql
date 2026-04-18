-- Phase 2 ceremony state: freeze flag + pre-announced Bitcoin beacon height.
-- Single-row singleton (id=1) so we can upsert without worrying about keys.

CREATE TABLE IF NOT EXISTS ceremony_state (
  id                      INTEGER PRIMARY KEY CHECK (id = 1),
  phase1_frozen           INTEGER NOT NULL DEFAULT 0,
  phase1_frozen_at        INTEGER,
  phase1_frozen_by_pk     TEXT,
  beacon_block_height     INTEGER,
  beacon_block_hash       TEXT,
  beacon_applied_at       INTEGER,
  note                    TEXT
);
INSERT OR IGNORE INTO ceremony_state(id, phase1_frozen) VALUES (1, 0);

-- Blossom-addressed zkey for each Phase 2 round (so the phase 2 dashboard
-- can link to downloadable artifacts the same way Phase 1 does).
ALTER TABLE phase2_rounds ADD COLUMN zkey_blob_sha256 TEXT;
