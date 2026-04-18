PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS participants (
  pubkey          TEXT PRIMARY KEY,
  display_name    TEXT,
  first_seen_at   INTEGER NOT NULL,
  email_optional  TEXT
);

CREATE TABLE IF NOT EXISTS signups (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  pubkey          TEXT NOT NULL REFERENCES participants(pubkey),
  tier            TEXT NOT NULL CHECK (tier IN ('small','medium','large')),
  joined_at       INTEGER NOT NULL,
  status          TEXT NOT NULL CHECK (status IN
                    ('queued','claimed','committed','expired','skipped','withdrawn')),
  slot_claimed_at INTEGER,
  slot_deadline   INTEGER,
  retry_count     INTEGER NOT NULL DEFAULT 0,
  UNIQUE(pubkey, tier, joined_at)
);
CREATE INDEX IF NOT EXISTS signups_tier_status ON signups(tier, status, joined_at);

CREATE TABLE IF NOT EXISTS rounds (
  tier               TEXT NOT NULL,
  round              INTEGER NOT NULL,
  contribution_id    TEXT NOT NULL,
  circuit_id         TEXT NOT NULL,
  srs_hash           TEXT NOT NULL,
  state_txt_hash     TEXT NOT NULL,
  receipt_hash       TEXT NOT NULL,
  participant_pk     TEXT,
  participant_label  TEXT,
  nostr_event_id     TEXT,
  prev_nostr_event_id TEXT,
  created_at         INTEGER NOT NULL,
  verified_ok        INTEGER NOT NULL,
  PRIMARY KEY (tier, round)
);
CREATE INDEX IF NOT EXISTS rounds_srs_hash ON rounds(srs_hash);

CREATE TABLE IF NOT EXISTS phase2_rounds (
  tier              TEXT NOT NULL,
  round             INTEGER NOT NULL,
  participant_pk    TEXT,
  zkey_hash         TEXT,
  attestation_hash  TEXT,
  nostr_event_id    TEXT,
  created_at        INTEGER NOT NULL,
  beacon_applied    INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (tier, round)
);

CREATE TABLE IF NOT EXISTS nip98_replay (
  event_id  TEXT PRIMARY KEY,
  seen_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS nip98_replay_seen ON nip98_replay(seen_at);

CREATE TABLE IF NOT EXISTS events (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  at            INTEGER NOT NULL,
  kind          TEXT NOT NULL,
  tier          TEXT,
  round         INTEGER,
  pubkey        TEXT,
  detail_json   TEXT
);
CREATE INDEX IF NOT EXISTS events_at ON events(at);
