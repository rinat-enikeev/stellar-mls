# Ceremony Web UI — Design Doc

**Status**: Draft v1
**Owners**: Onym core + ceremony coordinator
**Target host**: `ceremony.onym.chat`
**Companion documents**:
- `docs/trusted-setup-ceremony-phase1-coordinator-playbook.md` — operational runbook for coordinators
- `docs/trusted-setup-ceremony-phase1-participant-runbook.md` — operational runbook for participants
- `docs/trusted-setup-ceremony-phase2-participant-playbook.md` — Phase 2 participant flow
- `docs/phase2-mpc-integration.md` — snarkjs handoff
- `docs/keyset-generation.md` — keyset artefacts
- `docs/ceremony-tool-verification.md` *(planned)* — binary verification commands
- `docs/ceremony-reproducible-build.md` *(planned)* — reproducible build recipe

---

## 1. Context

Onym ships zero-knowledge group membership proofs over BLS12-381 Groth16. The
proving/verifying keys are the output of a two-phase trusted setup:

- **Phase 1 — Powers of Tau.** Universal MPC producing a structured reference
  string (SRS) that any Groth16 circuit up to a given degree can bind to.
- **Phase 2 — Circuit-specific zkey.** Per-circuit MPC (via snarkjs) binding
  the SRS to the exact R1CS of each tier (`small`, `medium`, `large`), then
  finalised with a public random beacon.

Today both phases are driven by `src/bin/ceremony_tool.rs` + GitHub-issue
coordination. This produces correct artefacts but is effectively invisible
to the broader community: only people willing to read Rust docs, run CLIs,
and paste hashes into GitHub comments will participate. The security model
demands **many independent contributors** (1-of-N honest) — every missing
contributor is a real weakening of the guarantee.

`keyset-v2` (current production candidate) is a **single-party** setup and
is explicitly unsuitable for mainnet. Before mainnet cutover we need:

1. A public, accessible way for anyone to enter a queue, take a turn, and
   contribute their own randomness.
2. A public, accessible way for anyone — not just participants — to verify
   every contribution in the transcript, without trusting a coordinator.
3. Explanations that work for both **lay users** and **cryptographers**, so
   the ceremony builds trust in both communities.

This document specifies the technical design for that system.

## 2. Goals and non-goals

### Goals

- Web UI at `ceremony.onym.chat` that orchestrates Phase 1 and tracks
  Phase 2 for all three tiers concurrently.
- Queue + slot management so participants never race or overwrite.
- Public, auditable transcript anyone can replay even if the coordinator
  disappears.
- In-browser verification with no install and no coordinator trust.
- Signed, reproducible **binaries** of `ceremony_tool` for
  macOS / Linux / Windows, plus a documented **build-from-source** path
  that matches the binaries byte-for-byte.
- Dual-mode educational content: **For Humans** and **For Mathematicians**,
  each page addressing both audiences without compromise.

### Non-goals (v1)

- In-browser **contribution** of Phase 1. Participants run the signed
  native binary locally; the browser never sees the toxic-waste scalar δ.
  *(Rationale: coordinator or CDN compromise could swap in malicious JS
  that exfiltrates δ. Native binary removes this surface.)*
- Byzantine-tolerant peer-to-peer coordination. With O(100) contributions
  over weeks, a single coordinator with a public, replayable transcript
  is sufficient.
- Custom identity / account system. Nostr pubkeys carry participant
  identity; no emails, no passwords, no OAuth.

## 3. Architecture overview

```
                       ┌──────────────────────┐
                       │ ceremony.onym.chat   │
                       └──────────┬───────────┘
                                  │ HTTPS (nginx)
            ┌─────────────────────┼─────────────────────┐
            ▼                     ▼                     ▼
    static frontend        ceremony-coordinator    /wasm/* assets
  (deploy/ceremony)        (Axum, port 9090)      (ceremony-wasm)
                                  │
        ┌─────────────────────────┼─────────────────────────┐
        ▼                         ▼                         ▼
   Blossom (blobs)          strfry (transcript)        SQLite index
   blossom.onym.chat        nostr.onym.chat           (queue + replay)
        ▲                         ▲
        │                         │
        └───── replayable without coordinator ──┘

                                  │
                                  ▼
                        ceremony_tool subprocess
                 (identical binary participants download)
```

**Trust anchors.** The coordinator is a *logistics* service, not a *trust*
service. Every round artefact is:

1. Content-addressed in Blossom (SHA-256 = Blossom key = `srs_hash` in
   `state.txt`).
2. Pointed to by a signed `kind 30078` Nostr event on
   `wss://nostr.onym.chat`, with an `e` tag chaining to the previous round.

If the coordinator vanishes, anyone can reconstruct the whole transcript
from Nostr + Blossom and resume the ceremony (`docs/resume-ceremony.md`,
planned).

## 4. Confirmed decisions

| # | Decision                                  | Value                                                                 |
|---|-------------------------------------------|-----------------------------------------------------------------------|
| 1 | Contribute mode                           | Native signed binary only (browser never holds δ)                     |
| 2 | Authentication                            | Nostr NIP-07 + NIP-98 per-request signing                             |
| 3 | Participant identity                      | Nostr npub (hex pubkey recorded in `receipt.txt`)                    |
| 4 | Tier scope                                | All three tiers (small / medium / large) run concurrently             |
| 5 | Data store                                | SQLite (`rusqlite`, WAL) bind-mounted to a named Docker volume        |
| 6 | Transcript                                | Blossom blobs + `kind 30078` Nostr events                             |
| 7 | Frontend tech                             | Static HTML5 + Alpine.js + vanilla JS + KaTeX (pre-rendered)          |
| 8 | Math rendering                            | KaTeX, pre-rendered at build time; CSS-only shipped to browser        |
| 9 | Dual-mode UX                              | Sticky segment toggle + per-card override; no forced side-by-side     |
| 10 | Phase 2 beacon                           | Pre-announced future Bitcoin block hash; height fixed at Phase 2 start |
| 11 | Admin actions                            | Single coordinator Nostr pubkey allow-list (FROST multisig is future work) |
| 12 | Queue gating                             | FIFO per tier + cheap PoW at signup to deter flooding                 |

## 5. Component inventory

### 5.1 `ceremony-coordinator/` — Axum service (new)

Sibling to `relayer/`. Multi-stage Dockerfile; builds `ceremony_tool` from
the main workspace into `/usr/local/bin/ceremony_tool` so the service and
participants run the same binary.

```
ceremony-coordinator/
├── Cargo.toml
├── Dockerfile
├── .env.example
├── migrations/
│   └── 001_init.sql
└── src/
    ├── main.rs
    ├── config.rs
    ├── auth.rs                # NIP-98 middleware
    ├── queue.rs               # per-tier slot state machine
    ├── store.rs               # rusqlite + WAL
    ├── blossom.rs             # PUT /<sha256>, GET /<sha256>
    ├── nostr.rs               # publish kind 30078, sign as coordinator
    ├── ceremony_exec.rs       # spawn ceremony_tool subprocesses
    ├── sse.rs                 # status broadcaster
    ├── model.rs               # serde types
    └── handlers/
        ├── signup.rs
        ├── slot.rs
        ├── upload.rs
        ├── verify.rs
        ├── transcript.rs
        ├── phase2.rs
        └── admin.rs
```

### 5.2 `crates/ceremony-wasm/` — verify-only WASM bundle (new)

Exposes **only** the audit surface. Never ships contribution code.

```rust
#[wasm_bindgen] pub fn verify_state_js(state_txt: &str, srs_bytes: &[u8], receipt_txt: &str) -> JsValue;
#[wasm_bindgen] pub fn verify_contribution_js(
    before_state_txt: &str, before_srs_bytes: &[u8],
    after_state_txt: &str,  after_srs_bytes: &[u8],
    after_receipt_txt: &str,
) -> JsValue;
#[wasm_bindgen] pub fn hash_srs_js(srs_bytes: &[u8]) -> String;
```

**Build**: `wasm32-unknown-unknown`, `wasm-opt -Oz`. Target <2 MB gzipped.

**Enabling change**: the root `Cargo.toml` currently depends on
`jni = "0.21"` unconditionally, which breaks `wasm32` builds. Before the
WASM crate can compile, feature-gate it:

```toml
[dependencies]
jni = { version = "0.21", optional = true }

[features]
default = ["jni"]
jni = ["dep:jni"]
```

The existing Android build uses the default feature set; the WASM crate
builds with `--no-default-features`.

### 5.3 `deploy/ceremony/` — static frontend (new)

```
deploy/ceremony/
├── index.html                # landing + dual-mode explainer
├── contribute.html           # signup, slot, download/upload
├── verify.html               # WASM verifier
├── phase2.html               # Phase 2 dashboard
├── download.html             # auto-detecting binary download
├── app.js                    # ~1000 LOC vanilla JS + Alpine
├── assets/
│   ├── ceremony.css
│   └── katex/                # vendored CSS + fonts only
├── content/                  # 12 step cards (dual-mode md)
│   ├── 00-what-is-trusted-setup.md
│   ├── 01-toxic-waste.md
│   ├── 02-one-of-n-honest.md
│   ├── 03-total-collusion.md
│   ├── 04-your-turn.md
│   ├── 05-good-randomness.md
│   ├── 06-prove-no-cheating.md
│   ├── 07-what-verification-means.md
│   ├── 08-six-pairing-equations.md
│   ├── 09-anyone-can-verify.md
│   ├── 10-phase-2-circuit-specific.md
│   ├── 11-random-beacon.md
│   └── _bibliography.md
├── tools/
│   ├── build.mjs             # md → static HTML with KaTeX prerender
│   ├── check-drift.mjs       # CI lint for runbook-excerpt divergence
│   └── render-step.mjs
└── wasm/                     # built artefact, gitignored
```

No bundler, no SPA framework. Alpine.js + vanilla JS + KaTeX CSS is the
whole runtime.

### 5.4 Release pipeline (new)

`.github/workflows/release-ceremony-tool.yml` — standalone, `workflow_dispatch`
and `workflow_call`. Doesn't block the mobile-app `release.yml`.

Matrix targets:

| Target                         | Artefact                                              | Signing               |
|--------------------------------|-------------------------------------------------------|-----------------------|
| `x86_64-unknown-linux-musl`    | `ceremony_tool-vX.Y.Z-x86_64-linux.tar.gz`            | minisign + cosign + SLSA |
| `aarch64-unknown-linux-musl`   | `ceremony_tool-vX.Y.Z-aarch64-linux.tar.gz`           | minisign + cosign + SLSA |
| `aarch64-apple-darwin`         | `ceremony_tool-vX.Y.Z-aarch64-macos.{tar.gz,dmg}`     | Developer ID + notarize + cosign |
| `x86_64-apple-darwin`          | `ceremony_tool-vX.Y.Z-x86_64-macos.{tar.gz,dmg}`      | Developer ID + notarize + cosign |
| `x86_64-pc-windows-msvc`       | `ceremony_tool-vX.Y.Z-x86_64-windows.zip`             | v1 unsigned; v2 Azure Trusted Signing |
| Multi-arch Docker              | `ghcr.io/rinat-enikeev/ceremony-tool:vX.Y.Z`          | cosign keyless        |

Also ships `generate_keyset` (same artefacts, distinct name) so coordinators
and auditors can reproduce keyset bundles.

## 6. Data model

SQLite schema (`ceremony-coordinator/migrations/001_init.sql`):

```sql
CREATE TABLE participants (
  pubkey           TEXT PRIMARY KEY,     -- Nostr hex pubkey
  display_name     TEXT,
  first_seen_at    INTEGER NOT NULL,
  email_optional   TEXT
);

CREATE TABLE signups (
  id               INTEGER PRIMARY KEY AUTOINCREMENT,
  pubkey           TEXT NOT NULL REFERENCES participants(pubkey),
  tier             TEXT NOT NULL CHECK (tier IN ('small','medium','large')),
  joined_at        INTEGER NOT NULL,
  status           TEXT NOT NULL CHECK (status IN
                     ('queued','claimed','committed','expired','skipped','withdrawn')),
  slot_claimed_at  INTEGER,
  slot_deadline    INTEGER,
  retry_count      INTEGER NOT NULL DEFAULT 0,
  UNIQUE(pubkey, tier, joined_at)
);
CREATE INDEX signups_tier_status ON signups(tier, status, joined_at);

CREATE TABLE rounds (
  tier              TEXT NOT NULL,
  round             INTEGER NOT NULL,
  contribution_id   TEXT NOT NULL,
  circuit_id        TEXT NOT NULL,
  srs_hash          TEXT NOT NULL,        -- = Blossom key for state.srs
  state_txt_hash   TEXT NOT NULL,
  receipt_hash     TEXT NOT NULL,
  participant_pk   TEXT,                  -- NULL for round 0
  participant_label TEXT,
  nostr_event_id   TEXT,
  prev_nostr_event_id TEXT,
  created_at       INTEGER NOT NULL,
  verified_ok      INTEGER NOT NULL,
  PRIMARY KEY (tier, round)
);
CREATE INDEX rounds_srs_hash ON rounds(srs_hash);

CREATE TABLE phase2_rounds (
  tier             TEXT NOT NULL,
  round            INTEGER NOT NULL,
  participant_pk   TEXT,
  zkey_hash        TEXT,
  attestation_hash TEXT,
  nostr_event_id   TEXT,
  created_at       INTEGER NOT NULL,
  beacon_applied   INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (tier, round)
);

CREATE TABLE nip98_replay (
  event_id         TEXT PRIMARY KEY,
  seen_at          INTEGER NOT NULL
);

CREATE TABLE events (                   -- audit log
  id               INTEGER PRIMARY KEY AUTOINCREMENT,
  at               INTEGER NOT NULL,
  kind             TEXT NOT NULL,
  tier             TEXT,
  round            INTEGER,
  pubkey           TEXT,
  detail_json      TEXT
);
```

Rebuild path: if the DB is lost, a bootstrap task replays all
`kind 30078` events with `d` prefix `sepceremony1:` from
`wss://nostr.onym.chat`, re-resolves Blossom blobs by hash, and
re-populates `rounds` + `events`. `signups` cannot be fully rebuilt
(queued-but-not-committed rows live only in the DB); participants can
re-sign up and lose their old queue position. Documented as a known
failure mode.

## 7. HTTP API

All authenticated endpoints require `Authorization: Nostr <base64(nip98_event_json)>`
per NIP-98. The coordinator rejects events older than 60 s and caches
event IDs for 10 min against replay.

```
GET  /api/v1/status
  → { "tiers": [ { "tier":"small", "head_round":7, "queue_depth":12,
                   "current_slot": { "participant_pk":"...",
                                     "claimed_at":..., "deadline":... } },
                  ... ] }

GET  /api/v1/tiers/:tier/queue
  → { "queue": [ { "position":1, "pubkey":"...", "display_name":"alice",
                   "joined_at":... }, ... ] }

POST /api/v1/signup                                              [auth]
  body: { "tier":"small", "display_name":"alice",
          "email_optional":null, "pow":"<nonce>" }
  → 201 { "position":13, "signup_id":42 }

POST /api/v1/tiers/:tier/claim                                   [auth, queue head]
  → 200 { "round":8, "deadline":...,
          "download": { "state_srs":"https://blossom.onym.chat/<hash>",
                        "state_txt":"...", "receipt":"..." },
          "expected_before_srs_hash":"<hex>" }
    409 if someone else holds the slot

POST /api/v1/tiers/:tier/contribute                              [auth, slot holder]
  multipart/form-data:
    state_srs:  file
    state_txt:  file
    receipt:    file
  → 200 { "round":8, "contribution_id":"sepceremony1:small:r8:...",
          "srs_hash":"<hex>", "nostr_event_id":"..." }
    400 { "error":"verify_contribution_failed", "detail":"..." }

GET  /api/v1/tiers/:tier/rounds
  → [ { "round":0, "contribution_id":"...", "srs_hash":"...",
        "participant_pk":null, "blobs":{...}, "verified_ok":true }, ... ]

GET  /api/v1/tiers/:tier/rounds/:round
  → { "round":8, "srs_hash":"...", "blobs":{...},
      "nostr_event_id":"...", "prev_nostr_event_id":"..." }

GET  /api/v1/tiers/:tier/rounds/:round/artifacts/:name
  → 302 redirect to https://blossom.onym.chat/<sha256>

GET  /api/v1/phase2/summary
  → { "frozen":true, "phase1_srs_hash":"...", "phase1_contribution_id":"...",
      "phase1_archive_url":"...", "snarkjs_ptau_url":"...",
      "rounds":[...], "beacon":{"kind":"bitcoin_block_hash",
                                 "value":"...","height":900000} }

POST /api/v1/phase2/rounds                                       [auth, admin]
  body: { "tier":"small", "round":3, "participant_pk":"...",
          "zkey_hash":"...", "attestation":"..." }
  → 201

GET  /api/v1/verify/state                                        [public]
  body: { "state_txt":"...", "srs_blob_sha256":"...", "receipt":"..." }
  → { "ok":true, "round":4, "tier":"small", ... }
  (Clients should prefer in-browser WASM verification.)

GET  /api/v1/status/stream                                       [public, SSE]
  → stream of JSON lines as status changes

GET  /api/v1/downloads                                           [public]
  → [ { "target":"aarch64-apple-darwin",
        "url":"...", "sha256":"...",
        "minisign_url":"...", "cosign_bundle_url":"..." }, ... ]
```

Nginx proxies `/api/*` → `ceremony-coordinator:9090` with
`proxy_buffering off` (SSE) and `proxy_read_timeout 86400s`.
`client_max_body_size 200M` supports Phase 2 zkey uploads.

## 8. Queue state machine

```
              ┌─────────────────────┐
              │  POST /signup       │
              ├─────────────────────┘
              ▼
       ┌─────────────┐    participant cancels
       │   queued    │ ─────────────────────────► withdrawn
       └──────┬──────┘
              │ coordinator pops head
              │ + mints 2h deadline
              ▼
       ┌─────────────┐    deadline or 3 retries
       │   claimed   │ ─────────────────────────► expired
       └──────┬──────┘                             │
              │ upload + verify ok                │ coordinator
              ▼                                    │ admin action
       ┌─────────────┐                             ▼
       │  committed  │                        ┌─────────┐
       └─────────────┘                        │ skipped │
                                              └─────────┘
```

- **FIFO per tier.** Signup row carries a monotonic `joined_at`; the head
  of the queue is `min(joined_at) where status='queued'`.
- **2-hour slot.** Wall clock from `slot_claimed_at`. 15-minute warning
  before expiry via SSE.
- **Invalid upload.** `ceremony_tool verify-contribution` non-zero exit
  bumps `retry_count`. Third failure force-expires the slot; participant
  must re-signup (goes to back of queue).
- **Admin skip.** Requires NIP-98 event signed by the coordinator's
  admin pubkey (env allow-list).

## 9. Sequence diagrams

### 9.1 Contribution (native, recommended path)

```
Participant   Browser       Coordinator      Blossom       Nostr       ceremony_tool
    │            │               │              │            │              │
    │── visit ──►│               │              │            │              │
    │            │               │              │            │              │
    │  NIP-07 sign (signup)      │              │            │              │
    │            │── POST /signup ─►             │            │              │
    │            │               │ verify NIP-98, PoW        │              │
    │            │               │ INSERT signup │            │              │
    │            │◄── 201 ───────│              │            │              │
    │            │               │              │            │              │
    │       ... SSE ticks until participant is head ...       │              │
    │            │               │              │            │              │
    │            │── POST /claim ─►              │            │              │
    │            │               │ queued→claimed │           │              │
    │            │◄── round=N, blob URLs, deadline ────────── │              │
    │            │               │              │            │              │
    │── fetch state.srs etc. directly from Blossom ──────► │              │
    │                                                                        │
    │ [local]                                                                │
    │ $ ceremony_tool verify-state                                           │
    │ $ ceremony_tool contribute --out out/                                  │
    │ $ ceremony_tool verify-contribution                                    │
    │                                                                        │
    │── upload out/ as multipart ───►│             │            │           │
    │                                 │ verify NIP-98, slot holder            │
    │                                 │ PUT state.srs ────►│                 │
    │                                 │ PUT state.txt ────►│                 │
    │                                 │ PUT receipt ───────►│                │
    │                                 │ ceremony_tool verify-contribution ───►│
    │                                 │◄──────────────────────── ok / error ─┤
    │                                 │ INSERT round, events                   │
    │                                 │ publish kind 30078 ──────►│            │
    │                                 │◄── event_id ──────────────│            │
    │◄────────── 200 { contribution_id, nostr_event_id } ──────── │            │
    │                                 │ claimed→committed                     │
    │                                 │ SSE status update                     │
```

### 9.2 Verify (anyone, any round)

```
Visitor       Browser+WASM       Coordinator       Blossom
   │              │                   │               │
   │── /verify ──►│                   │               │
   │◄── page + wasm ─│                │               │
   │              │── GET /rounds/7 ──►│               │
   │              │◄── blob URLs, hashes ─             │
   │              │                                   │
   │              │── GET blossom/<hash of state.srs>─►│
   │              │── GET blossom/<hash of state.txt>─►│
   │              │── GET blossom/<hash of receipt  >─►│
   │              │◄── bytes (hash-checked locally) ──│
   │              │                                   │
   │              │ wasm: verify_state_js(...)        │
   │              │ wasm: verify_contribution_js(prev, cur)
   │              │                                   │
   │◄── six pairing equations rendered via KaTeX, each with a green tick
```

Zero coordinator trust. Browser fetches content-addressed bytes, verifies
hashes, runs the pairing checks in local WASM.

### 9.3 Phase 2 handoff

```
Coordinator   Coordinator svc   ceremony_tool   Blossom   Nostr
admin              │                 │            │         │
   │               │                 │            │         │
   │── freeze ────►│                 │            │         │
   │ (NIP-98 admin)│ lock tier queues │            │         │
   │               │── phase2-summary ►│           │         │
   │               │◄── summary file ─│            │         │
   │               │── upload ────────────────────►│        │
   │               │── publish kind 30078 d=sepceremony1:phase2:frozen ──►│
   │◄── freeze event id + summary URL ─│           │         │
   │                                                         │
   │ [offline] snarkjs per participant, upload per round      │
   │── POST /phase2/rounds (admin) ──►│            │         │
   │                                   │ PUT zkey ─►│         │
   │                                   │ publish kind 30078 d=sepceremony1:phase2:<tier>:r<N> ──►│
   │                                                         │
   │── final beacon ──►│ record beacon, publish kind 30078 d=sepceremony1:phase2:<tier>:final ──►│
```

## 10. Authentication (NIP-98)

- Client (browser or CLI) constructs kind `27235` event:
  - `url` tag = absolute request URL
  - `method` tag = uppercase HTTP method
  - `payload` tag = SHA-256 of request body (if any)
  - `created_at` = current unix time
- Client signs with participant Nostr key, base64-encodes JSON, sends as
  `Authorization: Nostr <b64>`.
- Server verifies: signature valid, `created_at` within ±60 s, URL+method+body
  hash match, event ID not in `nip98_replay` (10-min window).
- Browser path: NIP-07 `window.nostr.signEvent`.
- CLI path: `tools/ceremony/sign-request.sh <key.json> <method> <url> [body]`
  uses a tiny Rust signer (shares code with `ceremony_tool`).

Admin actions (freeze, skip, phase2 round publish) additionally require the
event pubkey to be in the `CEREMONY_ADMIN_PUBKEYS` env allow-list.

## 11. Randomness and toxic waste — why browser contribute is excluded

The ceremony's security rests on **at least one honest contributor destroying
their δ_τ, δ_α, δ_β**. A browser-hosted contribute path exposes:

- **Coordinator compromise**: coordinator serves the page, so a malicious
  coordinator trivially swaps in JS that posts δ to an attacker.
- **CDN/TLS compromise**: a mis-issued cert on `ceremony.onym.chat` can
  serve malicious JS that still validates.
- **Browser-extension leak**: any content-script-capable extension can
  read page memory, including δ before it's zeroised.

The native binary removes all three surfaces: it runs locally under the
user's OS security, it uses `OsRng`, and `Drop`-zero on arkworks `Fr` types
handles secret cleanup. Recommended execution environment: ephemeral
Firecracker or QEMU VM with swap disabled, destroyed after the run.

This is documented in the "How to generate good randomness" card
(`deploy/ceremony/content/05-good-randomness.md`) in both audience modes.

## 12. Dual-mode content

### 12.1 Rendering

- **Primary control**: sticky top-of-page segment toggle
  `[ For Humans ] [ For Mathematicians ]`. Active state is purple-accent
  pill matching existing `.hero-badge` style. Persisted to
  `localStorage['ceremony.explainMode']` and URL hash (`#mode=math`) so
  readers can deep-link.
- **Per-card override**: each step card shows
  `Read the mathy version ↓` (or reverse) in its header.
- **Desktop ≥1200 px**: optional "Compare" toggle stacks both modes
  vertically per card (never two-column — wide equations break).
- **Mobile (<768 px)**: segment toggle only.

### 12.2 Step matrix

| # | Title                              | Visualisation                          |
|---|------------------------------------|----------------------------------------|
| 0 | What is a trusted setup?           | —                                      |
| 1 | What is toxic waste?               | —                                      |
| 2 | Why 1-of-N honest works            | Contribution-chain animator (SVG)      |
| 3 | What if everyone colludes?         | —                                      |
| 4 | Your turn, step by step            | —                                      |
| 5 | How to generate good randomness    | —                                      |
| 6 | How to prove you didn't cheat      | —                                      |
| 7 | What verification means            | —                                      |
| 8 | The six pairing equations          | Equation-unifier (hover τ to highlight) |
| 9 | Why anyone can verify              | —                                      |
| 10 | Why Phase 2 is circuit-specific    | QAP-shape morph (tier slider)          |
| 11 | The random beacon at the end       | —                                      |

Each card is one markdown file under `deploy/ceremony/content/` with:

```yaml
---
id: 08
slug: six-pairing-equations
title: "The six pairing equations"
audience: [contributor, verifier]
sources:
  - docs/trusted-setup-ceremony-phase1-participant-runbook.md
  - src/ceremony/mod.rs#L209-L280
last_verified_commit: <sha>
visual: equation-unifier
bibliography: [Gro16, BGM17]
---

## humans
…80–150 word analogy…

## math
…formal notation + \(inline\) / $$display$$ KaTeX + [Gro16] citations…
```

### 12.3 Maths rendering

**KaTeX, pre-rendered at build time.** The build script
(`deploy/ceremony/tools/build.mjs`) uses `katex.renderToString` and emits
a single static HTML page; only KaTeX CSS + fonts ship to the browser.
Payload drops from ~280 KB runtime to ~23 KB CSS. Matches the existing
`deploy/website/math.html` minimalist aesthetic.

### 12.4 Bibliography

Minimum bibliography for v1 (`deploy/ceremony/content/_bibliography.md`):

- `[Gro16]` Groth, J. *On the Size of Pairing-Based Non-interactive Arguments.* EUROCRYPT 2016.
- `[BGM17]` Bowe, Gabizon, Miers. *Scalable MPC for zk-SNARK Parameters in the Random Beacon Model.* ePrint 2017/1050.
- `[BGG19]` Bowe, Gabizon, Green. *A Multi-party Protocol for Constructing the Public Parameters of the Pinocchio zk-SNARK.* FC 2019 WTSC.
- `[KMSV21]` Kohlweiss, Maller, Siim, Volkhov. *Snarky Ceremonies.* ASIACRYPT 2021.
- `[BBBF18]` Boneh, Bonneau, Bünz, Fisch. *Verifiable Delay Functions.* CRYPTO 2018.
- `[BLS12-381]` Bowe. *BLS12-381: New zk-SNARK Elliptic Curve Construction.*
- `[snarkjs]` Baylina, J. et al. *snarkjs.* github.com/iden3/snarkjs
- `[drand]` Cloudflare / League of Entropy. *drand: distributed randomness beacon.*

Humans mode: no citations in body. Single "Want the papers?" link to the
bibliography panel at page bottom.

### 12.5 Drift prevention

- Each content file declares `sources:` pointing at `docs/*.md` files it
  summarises, plus a `last_verified_commit:` field.
- CI script `deploy/ceremony/tools/check-drift.mjs` fails PRs where any
  linked `docs/*.md` source has commits newer than `last_verified_commit`
  AND the card markdown was not touched.
- Optional fenced ` ```runbook-excerpt ` blocks copy exact CLI snippets
  from the playbook; the linter diffs them against the live file and
  blocks on divergence.

### 12.6 Interactive visualisations (three, hand-rolled SVG)

1. **Contribution-chain animator** (step 2). 5 circles labelled P1…P5;
   click "Ocean" under one, splash animation, struck-through δ, live
   accumulator `τ = δ₁·δ₂·…`. ~80 LOC JS + 30 lines inline SVG.
2. **Pairing-equation unifier** (step 8). Six equations as static SVG
   text; hovering `τ` in any equation lights up `τ` in all six via
   synced CSS classes. Zero JS beyond the mode toggle. ~50 SVG lines +
   20 CSS rules.
3. **QAP-shape morph** (step 10). Two side-by-side SVG lattices; slider
   (32 → 256 → 2048 gates) reshapes the right-side density. ~100 LOC.

All visualisations honour `prefers-reduced-motion: reduce` with a static
final-frame fallback. Mode toggle uses `role="switch"` + ARIA labels;
math-mode announce via `aria-live="polite"`.

### 12.7 Draft content samples

**Step 1 — Toxic waste (humans, 113 words):**

> Three numbers. Just three. We'll call them tau, alpha, and beta. To
> build the proof system we need them, briefly, the way a baker needs
> yeast. But unlike yeast, these three numbers have a nasty property:
> anyone who reassembles all three can forge a proof for any group chat
> — pretend to be a member they aren't, pretend a message is authentic
> when it's a lie.
>
> So we destroy them. Not just erase — we never let a single person know
> all three in the first place. We spread each number across many
> contributors; every contributor mixes in their own slice; every slice
> gets burned. That's toxic waste: the three numbers that must never
> come back.

**Step 1 — Toxic waste (mathematicians, 142 words):**

> Groth16's CRS is parameterised by a simulation trapdoor
> \(\mathsf{td} = (\tau, \alpha, \beta) \in \mathbb{F}_r^3\), with \(r\)
> the scalar-field order of BLS12-381. The published CRS contains
> the images \(\{\tau^i G_1\}_{i=0}^{2d-1}\), \(\{\tau^i G_2\}_{i=0}^{d}\),
> \(\{\alpha\tau^i G_1\}\), \(\{\beta\tau^i G_1\}\), \(\beta G_2\) —
> committing to \(\tau,\alpha,\beta\) but not revealing them (DLOG-hard).
>
> An adversary possessing \(\mathsf{td}\) can invoke the Groth16
> simulator \(\mathcal{S}(\mathsf{td}, x)\) (cf. [Gro16, §3.2]) to
> produce accepting proofs for any statement \(x\), including false
> ones — knowledge-soundness collapses to honest-CRS assumption.
>
> The ceremony replaces the scalars with
> \(\tau = \prod_{i=1}^{N} \delta_{\tau,i}\), analogously for
> \(\alpha, \beta\). The MPC invariant is: after each round \(i\),
> contributor \(i\) securely erases \(\delta_{\star,i}\); \(\mathsf{td}\)
> is recoverable only by colluding with *every* participant
> [BGM17, Thm 1].

**Step 2 — 1-of-N honest (humans, 134 words):**

> Imagine a vault with one keyhole but a ridiculous key: it's split into
> N slivers, and each of the N people who walked past the vault added
> their own metal shaving and then threw that shaving into the ocean.
> To open the vault you'd need to fish every single shaving back out.
> Miss one? The key is forever unrecoverable.
>
> That's our trusted setup. Every contributor adds their own random
> twist to the secret; every contributor then deletes their twist. To
> rebuild the toxic waste you'd need to corrupt — or subpoena, or
> torture, or pay off — every single contributor. If *one* of them
> actually destroyed their twist, the vault is sealed.
>
> That's why we want as many independent, unrelated contributors as
> possible. You only need to trust yourself.

**Step 8 — The six pairing equations (mathematicians, 189 words):**

> For contribution \(i\) with update factors \(\delta_\tau,\delta_\alpha,\delta_\beta\)
> and Schnorr-PoK tokens \((s_\tau G_1, \delta_\tau s_\tau G_1) \in \mathbb{G}_1^2\),
> \((s_\alpha G_2, \delta_\alpha s_\alpha G_2) \in \mathbb{G}_2^2\),
> \((s_\beta G_1, \delta_\beta s_\beta G_1) \in \mathbb{G}_1^2\), the
> verifier runs (pairing \(e: \mathbb{G}_1 \times \mathbb{G}_2 \to \mathbb{G}_T\)):
>
> **Ratio checks (proofs of knowledge):**
>
> 1. \(\tau\)-ratio: \(\quad e(s_\tau G_1, \tau_{\text{new}} G_2) \stackrel{?}{=} e(\delta_\tau s_\tau G_1, \tau_{\text{old}} G_2)\)
> 2. \(\alpha\)-ratio: \(\quad e(\alpha_{\text{new}}\tau^0 G_1, s_\alpha G_2) \stackrel{?}{=} e(\alpha_{\text{old}}\tau^0 G_1, \delta_\alpha s_\alpha G_2)\)
> 3. \(\beta\)-ratio: \(\quad e(s_\beta G_1, \beta_{\text{new}} G_2) \stackrel{?}{=} e(\delta_\beta s_\beta G_1, \beta_{\text{old}} G_2)\)
>
> **Consistency checks (cross-group coherence of the new SRS):**
>
> 4. \(\tau\)-consistency: \(\quad e(\tau_{\text{new}} G_1, G_2) \stackrel{?}{=} e(G_1, \tau_{\text{new}} G_2)\)
> 5. \(\alpha\text{-}\tau\)-consistency: \(\quad e(\alpha\tau_{\text{new}} G_1, G_2) \stackrel{?}{=} e(\alpha G_1, \tau_{\text{new}} G_2)\)
> 6. \(\beta\text{-}\tau\)-consistency: \(\quad e(\beta\tau_{\text{new}} G_1, G_2) \stackrel{?}{=} e(\beta G_1, \tau_{\text{new}} G_2)\)
>
> Identity-point rejection is enforced prior to pairing calls to prevent
> the \(e(\mathcal{O}, \cdot) = 1_{\mathbb{G}_T}\) degenerate-proof attack
> (`src/ceremony/mod.rs:214–245`). All six implemented in
> `verify_contribution` (op. cit. L209–280); an initial-contribution
> variant against generator points appears in `verify_initial_contribution`
> (L289–342).

## 13. Binary distribution

### 13.1 Targets and signing

| Target                         | Notes |
|--------------------------------|-------|
| `x86_64-unknown-linux-musl`    | Fully static. Works on any Linux kernel ≥ 3.2. Reproducible via pinned Alpine Docker. |
| `aarch64-unknown-linux-musl`   | Built via `cargo-zigbuild` from the same Linux runner. |
| `aarch64-apple-darwin`         | `codesign --options runtime --timestamp`, `notarytool submit --wait`. Ship both raw `.tar.gz` (for curl users) and stapled `.dmg` (for Gatekeeper-offline). |
| `x86_64-apple-darwin`          | Same treatment. |
| `x86_64-pc-windows-msvc`       | v1: unsigned `.zip` + `SHA256SUMS` + minisign. v2: Azure Trusted Signing. SmartScreen workaround documented on the download page. |
| Docker (linux/amd64+arm64)     | `scratch` base, from the musl binaries. `ghcr.io/rinat-enikeev/ceremony-tool:vX.Y.Z`. cosign keyless. |

### 13.2 Reproducibility

- **Toolchain pin**: new `rust-toolchain.toml` (starting `channel = "1.82.0"`,
  `profile = "minimal"`).
- **Profile**: new `[profile.release-ceremony]` in root `Cargo.toml`:
  `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`,
  `strip = "symbols"`, `panic = "abort"`, `debug = false`. Distinct from
  the mobile `release` profile (which is tuned for binary size).
- **Env**: `SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)`, `TZ=UTC`,
  `LC_ALL=C`, `RUSTFLAGS="-C link-arg=-Wl,--build-id=none
  --remap-path-prefix=${CARGO_HOME}=/cargo
  --remap-path-prefix=${PWD}=/src"`.
- **Linux reference build** runs inside a digest-pinned
  `rust:1.82.0-alpine3.20@sha256:…` image. Linux byte-reproducibility is
  the bar; macOS/Windows achieve reproducible **unsigned pre-image**
  bytes since signing introduces deterministic-but-run-dependent blobs.
- **`buildinfo.json`** per artefact:

  ```json
  {
    "tool": "ceremony_tool",
    "version": "v1.3.0",
    "commit": "…",
    "target": "x86_64-unknown-linux-musl",
    "rust_version": "1.82.0",
    "cargo_lock_sha256": "…",
    "source_date_epoch": 1713456789,
    "docker_image": "rust:1.82.0-alpine3.20@sha256:…",
    "rustflags": "…",
    "cargo_flags": "--locked --profile release-ceremony --bin ceremony_tool",
    "artifact_sha256": "…",
    "unsigned_sha256": "…"
  }
  ```

- **`scripts/verify-ceremony-tool.sh <tag> [target]`**: checks out the
  tag, sets the reproducible env, runs `cargo build --locked --profile
  release-ceremony`, strips, hashes, fetches the `unsigned_sha256` from
  the release's `buildinfo.json`, diffs. On Linux-in-Docker: byte-match
  expected. On bare macOS/Windows: may differ; script warns.

### 13.3 Trust chain

Three independent signing surfaces per artefact:

1. **minisign** (`deploy/website/pubkeys.txt`, pinned in repo + on
   `ceremony.onym.chat/download`). Private key in GH Actions secrets.
2. **cosign keyless** (Sigstore / Rekor). OIDC identity =
   `https://github.com/rinat-enikeev/stellar-mls/.github/workflows/release-ceremony-tool.yml@refs/tags/vX.Y.Z`.
3. **SLSA v1 provenance** via `slsa-framework/slsa-github-generator`.

Compromise of any one does not break the chain. Pubkeys are also
published as a `kind 0` event on `wss://nostr.onym.chat` signed by the
coordinator Nostr key — a fourth independent channel.

### 13.4 Download page

`deploy/ceremony/download.html`:

- Auto-detects OS+arch via `navigator.userAgentData.platform` +
  `architecture`; highlights one row, lists all.
- **Never auto-downloads.** Always requires a click.
- Shows inline: SHA-256, minisign signature URL, cosign bundle URL,
  verification command (`shasum -a 256 …`, `codesign --verify --deep --strict …`,
  `minisign -Vm … -P RWT…`, `cosign verify-blob …`), pubkey values.
- Machine-readable `ceremony-downloads.json` consumed by the page and
  updated by the release workflow.

### 13.5 CI structure

`.github/workflows/release-ceremony-tool.yml`:

```yaml
name: Release ceremony tool
on:
  workflow_dispatch:
    inputs: { tag: { required: true, type: string } }
  workflow_call:
    inputs: { tag: { required: true, type: string } }

permissions:
  contents: write
  id-token: write
  attestations: write

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { os: ubuntu-22.04, target: x86_64-unknown-linux-musl  }
          - { os: ubuntu-22.04, target: aarch64-unknown-linux-musl }
          - { os: macos-14,     target: aarch64-apple-darwin       }
          - { os: macos-14,     target: x86_64-apple-darwin        }
          - { os: windows-2022, target: x86_64-pc-windows-msvc     }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
        with: { ref: ${{ inputs.tag }}, fetch-depth: 0 }
      - uses: dtolnay/rust-toolchain@master
        with: { toolchain: '1.82.0', targets: ${{ matrix.target }} }
      # set SOURCE_DATE_EPOCH, TZ, LC_ALL, RUSTFLAGS …
      - run: cargo build --locked --profile release-ceremony --target ${{ matrix.target }} --bin ceremony_tool --bin generate_keyset
      - run: ./scripts/sign-macos.sh   # if apple-darwin
      - run: ./scripts/sign-linux.sh   # if linux
      - uses: sigstore/cosign-installer@v3
      - run: cosign sign-blob --yes --bundle artefact.cosign-bundle artefact
      - run: ./scripts/write-buildinfo.sh
      - uses: actions/upload-artifact@v4

  publish:
    needs: build
    steps:
      - uses: actions/download-artifact@v4
      - run: sha256sum dist/*/* > sha256sums.txt
      - run: ./scripts/sign-checksums.sh
      - uses: softprops/action-gh-release@v2

  docker-image:
    needs: build
    permissions: { packages: write, id-token: write }
    steps:
      - uses: docker/setup-buildx-action@v3
      - run: docker buildx build --platform linux/amd64,linux/arm64 --push -t ghcr.io/…/ceremony-tool:${{ inputs.tag }} -f Dockerfile.ceremony .

  slsa:
    needs: publish
    permissions: { id-token: write, contents: write, actions: read }
    uses: slsa-framework/slsa-github-generator/.github/workflows/generator_generic_slsa3.yml@v2.0.0

  update-website:
    needs: [publish, slsa]
    steps:
      - run: ./scripts/update-ceremony-downloads.py --tag ${{ inputs.tag }}
      - run: git add … && git commit -m "Update ceremony download page for ${{ inputs.tag }}" && git push
```

CI fail-safe: no `continue-on-error` on signing. `publish` depends on
`build`. Wall time ≈ 20 min cold.

## 14. Nginx and Docker Compose wiring

### 14.1 `deploy/nginx/conf.d/ceremony.onym.chat.conf` (new)

```nginx
server {
    listen 443 ssl;
    http2 on;
    server_name ceremony.onym.chat;

    ssl_certificate     /etc/letsencrypt/live/onym.chat/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/onym.chat/privkey.pem;
    include /etc/nginx/ssl-params.conf;

    root /var/www/ceremony;
    index index.html;

    client_max_body_size 200M;

    location /api/ {
        proxy_pass http://ceremony-coordinator:9090;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_buffering off;
        proxy_read_timeout 86400s;
    }

    location /wasm/ {
        add_header Cache-Control "public, max-age=31536000, immutable";
        try_files $uri =404;
    }

    location / {
        try_files $uri $uri.html $uri/ /index.html;
    }
}
```

### 14.2 `docker-compose.yml` additions

```yaml
ceremony-coordinator:
  build:
    context: ./ceremony-coordinator
    dockerfile: Dockerfile
  env_file: ./ceremony-coordinator/.env
  environment:
    - CEREMONY_BIND=0.0.0.0:9090
    - CEREMONY_DB_PATH=/app/data/ceremony.db
    - CEREMONY_BLOSSOM_URL=http://blossom:3000
    - CEREMONY_NOSTR_RELAY=ws://nostr-relay:7777
    - CEREMONY_TOOL_BIN=/usr/local/bin/ceremony_tool
    - CEREMONY_ALLOW_BROWSER_CONTRIBUTE=false
  volumes:
    - ceremony-data:/app/data
  restart: unless-stopped
  networks: [internal]
  depends_on: [blossom, nostr-relay]

volumes:
  ceremony-data:
```

### 14.3 DNS + certs

`deploy/digitalocean/deploy.sh`: add `ceremony.onym.chat` to the
Cloudflare A-record provisioning block. Certbot config gains
`-d ceremony.onym.chat`.

### 14.4 Nostr retention

`deploy/strfry/strfry.conf`: add retention rule to keep `kind 30078`
events with `d` prefix `sepceremony1:` indefinitely. A cron-like job
weekly dumps them to JSON + uploads to Blossom so the transcript is
mirrored at a content-addressed second location.

## 15. Rollout plan

| Phase | Duration | Scope |
|-------|----------|-------|
| **A** | 2-3 wk   | Coordinator MVP: native-only contribute, SQLite queue, NIP-98 auth, three tiers, Blossom + Nostr publish, static site with explainer + live queue, binary release workflow for Linux + macOS (unsigned first), staging on `ceremony-staging.onym.chat`. |
| **B** | 1 wk     | WASM verifier crate, `/verify` page, per-round "verify in your browser" button, cosign + minisign + SLSA + macOS notarisation. |
| **C** | 2 wk     | Phase 2 dashboard, snarkjs handoff helper Docker image, Windows signing (Azure Trusted Signing), Homebrew tap + Scoop bucket. |
| **D** | —        | Public ceremony runs for small / medium / large in parallel. Monitor, respond, triage invalid contributions. |
| **E** | —        | Post-ceremony freeze: coordinator goes read-only, transcript mirrored to GitHub Releases, `/verify` stays live forever. |

Phase A is the minimum to run a real ceremony. B/C overlap with a small-tier
dry-run.

## 16. Acceptance criteria

- [ ] Anyone visiting `ceremony.onym.chat` can sign up with a Nostr key,
      land in one of three queues, and receive a turn.
- [ ] At their turn they download the current state, run the native
      binary, upload the result, and see their contribution land in the
      public transcript within seconds.
- [ ] Anyone can verify any round in-browser without installing anything
      and without trusting the coordinator.
- [ ] Every explanatory step is rendered in both modes, with a
      persistent toggle and per-card override.
- [ ] Signed binaries for macOS (Apple Silicon + Intel), Linux
      (x86_64 + arm64 musl), and Windows (x86_64) are downloadable from
      `ceremony.onym.chat/download`, with inline checksums, signatures,
      and pubkeys.
- [ ] A documented build-from-source path reproduces the Linux binary
      byte-for-byte and the macOS / Windows unsigned pre-image
      byte-for-byte.
- [ ] Transcript is replayable from Blossom + Nostr if the coordinator
      is offline.

## 17. Risks and mitigations

| Risk                                                        | Mitigation                                                                                                 |
|-------------------------------------------------------------|------------------------------------------------------------------------------------------------------------|
| Coordinator availability outage                             | Transcript replayable from Nostr + Blossom; `docs/resume-ceremony.md` (planned) describes handover.         |
| Coordinator lies about queue order                          | Signups also published as Nostr events; queue order is publicly auditable.                                  |
| Invalid upload DoS                                          | Verify at upload, `retry_count` bump, 2 h hard deadline, force-expire after 3 rejections.                   |
| `jni` dep blocks WASM build                                 | Feature-gate `jni` in root `Cargo.toml` as part of Phase A.                                                 |
| Apple notarisation flakes                                   | Retry 3× then fail; fall back to unsigned artefact + signed checksums.                                      |
| Reproducibility drift from arkworks proc-macro output       | `Cargo.lock` committed, `--locked`, pinned Docker image; monitor closely on first release.                  |
| Signing key compromise                                      | Two independent signatures (minisign + cosign keyless) + SLSA provenance + pubkeys pinned in repo + Nostr.  |
| Contributor uses weak randomness                            | Documented in step 5; binary uses `OsRng`; ephemeral VM recommended; out-of-protocol.                       |
| Toxic-waste leak via browser                                | Contribute mode is native-only; the `CEREMONY_ALLOW_BROWSER_CONTRIBUTE` flag stays `false`.                 |
| strfry drops old events                                     | Retention policy in `strfry.conf`; weekly JSON dump mirrored to Blossom.                                    |
| Admin pubkey compromise                                     | Single coordinator key documented as a weakness; FROST multisig is future work (Phase-E+).                  |

## 18. Open questions

- **Coordinator Nostr key**: generate offline, publish kind 0 profile
  with ceremony details, pin pubkey in repo README and download page.
- **Phase 2 beacon height**: announce exact Bitcoin block height on
  Phase 2 freeze; record in `phase2/summary` JSON.
- **`scripts/srs_to_ptau.py`**: referenced in
  `docs/phase2-mpc-integration.md`; confirm status. Either ship in the
  snarkjs helper Docker image or rewrite in Rust and merge into
  `ceremony_tool`.
- **Rate-limiting policy**: per-endpoint budget (signup slow, status
  hot); document in coordinator `.env.example`.
- **Homebrew tap + Scoop bucket**: decide in Phase C whether the
  maintenance cost is justified.
- **Email opt-in channel**: reuse the existing `pn-relay` push channel
  for slot-up notifications rather than introducing email infra. Confirm
  in Phase A.

## 19. References

- `/Users/programyzer/Developer/stellar-mls/src/bin/ceremony_tool.rs` —
  existing CLI (init, contribute, verify-contribution, verify-state,
  show-receipt, phase2-summary).
- `/Users/programyzer/Developer/stellar-mls/src/ceremony/mod.rs` —
  authoritative Phase 1 protocol implementation (`verify_contribution`
  L209-L280, `verify_initial_contribution` L289-L342,
  `verify_consistency` L348-L419).
- `/Users/programyzer/Developer/stellar-mls/src/ceremony/phase2.rs` —
  SRS export/import for snarkjs interop.
- `/Users/programyzer/Developer/stellar-mls/tools/ceremony/run.sh` —
  wrapper invoked by participants and the coordinator subprocess.
- `/Users/programyzer/Developer/stellar-mls/docs/trusted-setup-ceremony-phase1-*.md` —
  operational runbooks; remain the authoritative source for participant
  commands.
- `/Users/programyzer/Developer/stellar-mls/docs/phase2-mpc-integration.md`,
  `docs/keyset-generation.md` — Phase 2 integration and keyset workflow.
- `/Users/programyzer/Developer/stellar-mls/deploy/website/{math,architecture,correctness}.html` —
  existing onym.chat aesthetic to inherit.
- `/Users/programyzer/Developer/stellar-mls/docker-compose.yml`,
  `deploy/nginx/conf.d/*.conf`, `deploy/digitalocean/deploy.sh` —
  existing deployment model to extend.
