# Postmortem: ceremony artifact loss + silent Nostr publisher

**Status:** Decided 2026-04-29 · prune fix landed in [PR #161](https://github.com/rinat-enikeev/stellar-mls/pull/161); broader response captured in [`fflonk-migration-design.md`](fflonk-migration-design.md)
**Decision:** Hot-fix the immediate prune bug; pivot to fflonk + Ethereum Foundation KZG SRS to eliminate the operational surface that produced the incident.
**Discovered:** Routine backup of `ceremony.onym.chat` artifacts on 2026-04-29 surfaced 22 of 30 sidecars missing on `blossom.onym.chat`. Cross-checking against the local strfry relay surfaced a second, independent bug.

## TL;DR

Two unrelated bugs converged across the trusted-setup stack and silently destroyed audit data for 12 of the 15 phase-1 contribution rounds:

1. **`blossom-server` ran with no `config.yml` mounted.** Its built-in default rules expire every blob 1 week after last access. The cold ceremony sidecars (`state.txt` ≈360 B, `receipt.txt` ≈1.4 KB) idled past the 7-day window and were deleted by the regular prune cycles. The hot `state.srs` files (≈1 MB, fetched on every next-round claim) survived.
2. **The coordinator's Nostr publisher dies on first websocket send error.** `ceremony-coordinator/src/nostr.rs:135-164` `return`s out of `relay_loop` on any send failure rather than reconnecting; the `mpsc::UnboundedSender` then accepts further events into a void. The 3 seed events made it to the relay at 17:44–17:49 on 2026-04-18; from the first contribution commit onward (small/r1 at 18:07:58) every subsequent `kind=30078` event was silently dropped.

Net loss: 22 of 30 phase-1 sidecars + 12 of 15 round-commit Nostr attestations. SRS files intact (15 of 15). Cryptographic soundness of the resulting SRS is preserved; **public re-auditability of the contribution chain is degraded** — see "Blast radius" below.

## Why this wasn't found earlier

- **No canary test crossed the 7-day window.** Phase-1 ran Apr 17–20; the audit ran Apr 29. The first three rounds aged out before anyone re-fetched their sidecars; subsequent rounds aged out as the front of the chain shifted past them. There was no scheduled "compare blob inventory against rounds-DB" sweep.
- **The publisher's failure mode was silent end-to-end.** `publish()` returns `Ok(Some(id))` after computing the event id, regardless of whether the relay ever received the event. The coordinator wrote the id back into `rounds.nostr_event_id` and the front-end displayed it as published. There was no "did the relay actually accept this?" probe.
- **Both surfaces lacked liveness metrics.** Blossom's prune deletions are logged at INFO (`[prune] deleted=N errors=0`) but no alert ever fires when N > 0 on coordinator-owned blobs. The relay had no count-of-coordinator-events monitor.
- **The default `blossom-server` config is hostile to ceremony use.** A wildcard 1-week LRU rule is sensible for a chat CDN; for a content-addressed transcript store it is the wrong default. We deployed the image without overriding it, and the default did exactly what it advertised.

## The bugs, in detail

### Bug 1 — Blossom default prune

`hzrd149/blossom-server`'s loader falls back to the default ruleset when no `config.yml` exists at the working directory:

```yaml
storage.rules:
  - { type: "image/*", expiration: "1 month" }
  - { type: "video/*", expiration: "1 week" }
  - { type: "*",       expiration: "1 week" }   # catches text/plain
```

Prune is **last-access based** (`accessed.timestamp` table). The coordinator uploads three blobs per round in `handlers/upload.rs:145-159`:
- `state.srs` (~590 KB / 1.18 MB) — touched whenever the next round's claimer fetches `prev.zip`.
- `state.txt` (~360 B) — only touched if a viewer clicks the round-detail artifact link.
- `receipt.txt` (~1.4 KB) — same as state.txt.

Across 12 of the 15 rounds, no client touched the sidecars within the 7-day window after upload. Prune deleted them. Container logs show `[prune] deleted=2 errors=0` and `[prune] deleted=4 errors=0` cycles in the days leading up to the audit.

The compose file (`docker-compose.yml:41-50`) mounted only `blossom-data:/app/data` and set two env vars; no config.yml was bind-mounted. **PR #161** adds `deploy/blossom/config.yml` with a coordinator-pubkey-pinned 100-year rule; this stops the bleed but does not recover the lost data.

### Bug 2 — Coordinator publisher dies on first ws error

`ceremony-coordinator/src/nostr.rs:140-156`:

```rust
loop {
    match connect_async(&relay_url).await {
        Ok((mut ws, _resp)) => {
            while let Some(event) = rx.recv().await {
                if let Err(e) = ws.send(...).await {
                    warn!(error = %e, "nostr send failed, reconnecting");
                    break;          // breaks the inner loop
                }
                // ...
            }
            let _ = ws.close(None).await;
            return;                  // ← exits the function permanently
        }
        Err(e) => { /* retries */ }
    }
}
```

The intent is clearly "reconnect on send failure" — the warn message says so. But `break` exits the `while let Some(event)` loop, which falls through to `ws.close(...).await` and `return`, killing the publisher task. Subsequent events sent on the channel succeed silently because `tx.send()` on an `UnboundedSender` returns an error only if the receiver has been dropped — and `relay_loop`'s receiver was moved into the function, then dropped on `return`, but `publish()` ignores that error with `let _ = tx.send(signed)` (line 83).

Connection lifetime in production: strfry has been continuously running since 2026-04-04; it has no idle-timeout config; but `enableTcpKeepalive = false` (the config default we kept) means any intermediate NAT or load balancer is free to drop an idle TCP flow. Between 17:49 and 18:07 on 2026-04-18 — 18 minutes of idleness following the seed bursts — the websocket apparently dropped silently, the publisher's next send errored, and `relay_loop` returned.

### Confirmation

- All 4 surviving `kind=30078` events (3 r0 seeds + 1 `releasekeys`) carry `created_at` between 17:46 and 17:49 on 2026-04-18.
- 12 missing events correspond to round commits at 18:07 and later. The coordinator's `events` table records every commit; the relay has none of them.
- A by-id query against the local relay for each of the 12 missing event ids returns `hits=0`. Public relays (damus.io, nos.lol, nostr.band, primal, snort, wine) all return zero — the coordinator only ever pushed to the local relay, so there's no upstream copy.

## The architectural argument

The two bugs are independent, but their joint effect points at a deeper problem: **the trusted-setup ceremony is an operational surface this project cannot afford to maintain.**

We're a small team running:
- A custom ceremony-coordinator HTTP service with its own SQLite DB, queue logic, slot management, and reaper.
- A custom signed receipt format and verification subprocess (`ceremony_tool verify-contribution`).
- A custom signed-event publishing pipeline against a custom relay.
- A custom Blossom storage layer for the artifacts.
- A custom WASM verifier (`crates/ceremony-wasm`) so browsers can independently re-verify the chain.
- A custom static download surface (`deploy/ceremony/`) and frontend.
- Phase 2 infrastructure that hasn't even run yet, with its own table, routes, and freeze/beacon logic in `handlers/phase2.rs`.

Every one of those layers is a place for a default-config bug, a silent-error bug, or a cold-storage retention bug to destroy data. We hit two of them in the first ten days of operation. There's no reason to believe we found all of them.

The ceremony exists for one reason: to pin the trapdoor of the per-circuit Groth16 SRS to a multi-party computation no single contributor can subvert. **Ethereum Foundation's 2023 KZG ceremony already produced a public, audited, 140k-contributor powers-of-tau on BLS12-381 — the same curve we use.** A PLONK-family proving system consumes that SRS directly without any additional setup, universally across all circuits. Adopting it deletes the operational surface that produced this incident.

## Decision

1. **Hot-fix.** [PR #161](https://github.com/rinat-enikeev/stellar-mls/pull/161) lands `deploy/blossom/config.yml` with a 100-year coordinator-pubkey-pinned rule. Already deployed to the live droplet on 2026-04-29; first post-fix prune cycle deleted 0 blobs.
2. **Pivot.** Adopt fflonk on BLS12-381 with EF KZG SRS as the production proving stack. See [`fflonk-migration-design.md`](fflonk-migration-design.md). This decommissions the entire ceremony surface (coordinator, relay-publisher, Blossom retention rules, browser verifier, participant CLI, deploy frontend, install-vks scripts).
3. **Don't re-run the current ceremony from scratch.** The SRS is cryptographically sound (one-honest-contributor argument is unaffected by lost receipts; coordinator's at-upload `verify_contribution` ran for every round and is logged in `rounds.verified_ok`). Current testnet contracts continue using the existing `keyset-v2` until the migration ships.
4. **Don't fix `nostr.rs:relay_loop`.** The migration deletes the file. Spending engineering on a publisher that the migration removes is wasted.

## Alternatives weighed

| Option | Cost | Closes the failure mode? | Why not chosen |
|---|---|:-:|---|
| **Hot-fix prune + fix relay_loop + re-run ceremony from scratch** | High — coordinate 7+ contributors again, run another 15 rounds, pay weeks of operational tail | Same surface, same default-config and silent-error risks | Fixes today's instances, leaves the architecture that produced them. We'd be one config file or one libtungstenite version bump away from the next loss. |
| **Hot-fix prune + restore receipts from participants only** | Low–Medium — outreach, depends on participant cooperation | Restores public auditability for rounds where participants still have local state | Worth doing as a side-task, but doesn't address the architectural argument. Recommended **as a parallel cleanup** even if we adopt fflonk, to bring the historical record up to par before the legacy contracts are deprecated. |
| **Hot-fix prune + accept current artifacts as-is** | Zero | No | Cryptographically defensible (SRS soundness intact) but leaves us with a partially-auditable transcript and the same surface for future loss. |
| **Adopt fflonk + EF KZG** ✓ | High one-time — circuit port, verifier rewrite, mobile rebundle | Eliminates the entire ceremony surface | Chosen. The migration cost is paid once; the operational surface goes away forever. Universal SRS also kills the per-circuit phase-2 ceremony for the 5 gov contracts that haven't shipped yet. |

## Blast radius

### Cryptographic soundness — intact

The SRS produced by the existing 15 rounds is sound under the standard MPC argument: at least one honest contributor in each tier generated their own contribution scalar and erased it. This argument is about contributor behavior at contribution time, not about post-hoc artifact persistence. Losing receipt files does not retroactively make any contributor dishonest.

Per-round verification ran at upload time (`handlers/upload.rs:122-141` calls `ceremony_tool verify-contribution` and only inserts the row on success; `rounds.verified_ok = 1` for all 15 rounds). The chain that the surviving SRSes form is consistent — round N's `before` SRS hash matches round N-1's published SRS, end-to-end.

### Public auditability — degraded

For 22 of 30 sidecars (12 of 15 rounds), an external auditor can no longer:
- Re-derive each round's Schnorr proofs of knowledge (`τ_proof_g1`, `α_proof_g2`, etc. lived in `receipt.txt`).
- Verify the participant pubkey-to-contribution binding without trusting the coordinator's at-upload check.

For 12 of 15 round commits, there is no Nostr-relay attestation that the coordinator processed the round. The only remaining record is the coordinator's local SQLite (`rounds` table).

### Production deployment status

`keyset-v2` is the active testnet keyset. Mainnet has not deployed. **No production user is currently relying on these artifacts.** The freeze on running the production ceremony pending the migration decision means the loss is contained to the testnet/dev keyset — the artifacts that would matter least if irrecoverable.

### Service exposure

- `blossom.onym.chat` continues to serve all surviving blobs.
- `ceremony.onym.chat` continues to serve transcript browsing; missing sidecars surface as 404 on artifact links.
- `relay.onym.chat` is unaffected (relay was the victim, not the cause).

## Migration phases (high level)

Detailed phasing in [`fflonk-migration-design.md`](fflonk-migration-design.md). One-line sketch:

| Phase | Scope |
|---|---|
| A — receipt salvage | parallel cleanup; recover what we can from participants and seal the historical record |
| B — proving-system swap | replace `ark-groth16` → PLONK-family on BLS12-381, port `src/circuit/`, fold EF KZG SRS into the prover |
| C — Soroban verifier rewrite | new verify path in each gov contract, drop `sep-xxxx` |
| D — mobile rebundle | replace 12-file `keyset-v2/` bundles with a single SRS commitment + per-circuit selectors |
| E — coordinator decommission | delete `ceremony-coordinator`, `tools/ceremony`, `crates/ceremony-wasm`, `deploy/ceremony`, `scripts/install-*-vks-*.sh`, the `phase2_*` table and routes |
| F — postmortem-ledger close | this document goes from "decided" to "complete" once the legacy artifacts are archived |

## What this isn't

- **Not an indictment of `hzrd149/blossom-server`.** The default 1-week LRU rule is correct for chat blob storage. We deployed it without overriding the default, and the default did its job. PR #161's fix is to deploy the right config, not to change the upstream image.
- **Not an indictment of strfry.** The relay accepted everything sent to it. The coordinator stopped sending.
- **Not a call for re-running the existing ceremony.** It's a call for retiring the *concept* of running our own ceremony.
- **Not a security incident in the user-facing sense.** No user data was leaked or compromised. No production groups depend on the lost artifacts. The damage is to internal auditability of a testnet keyset that the migration will retire.

## How this was found, in detail

1. User asked for a backup of ceremony artifacts (2026-04-29 ~16:30 UTC).
2. Backup script enumerating `/api/v1/tiers/{tier}/rounds` then GETting each `srs_hash` / `state_txt_hash` / `receipt_hash` from `blossom.onym.chat` returned 22 × 404 against the sidecar hashes; all 15 SRS hashes returned 200.
3. SSH inspection of `/var/lib/docker/volumes/onym_blossom-data/_data/` confirmed the 22 sidecars were absent on disk and absent from the local SQLite `blobs` index — they were never re-evicted from a backup, they were genuinely gone.
4. Reading `/app/src/prune/prune.ts` and `/app/src/config/schema.ts` inside the running container surfaced the default rule and confirmed the prune cause. Container logs showed historical `[prune] deleted=N errors=0` cycles consistent with the loss.
5. Hypothesis "maybe receipts were also stored on Nostr" sent us to `handlers/upload.rs:250` which publishes the receipt content as the `kind=30078` event body.
6. Querying the local strfry relay returned 4 events (3 seed + 1 `releasekeys`); query by id for each of the 12 missing per-round event ids returned `hits=0`.
7. Probing public relays (damus.io, nos.lol, nostr.band, primal, snort, wine) returned zero matching events from the coordinator pubkey — the coordinator only pushes to the local relay.
8. Reading `nostr.rs:relay_loop` surfaced the `return` bug.
9. Aligning the four surviving `kind=30078` events' timestamps against the `events` audit-log commits showed the publisher accepted events through 17:49 on 2026-04-18 and silently dropped everything after 18:07. Roughly 18 minutes of idleness in between, consistent with NAT/LB idle-timeout dropping the websocket.

## References

- `ceremony-backup/20260429T163349Z/` — full local snapshot used for this analysis (gitignored)
- [PR #161](https://github.com/rinat-enikeev/stellar-mls/pull/161) — `deploy/blossom/config.yml` + bind mount
- [`fflonk-migration-design.md`](fflonk-migration-design.md) — the architectural response
- `ceremony-coordinator/src/nostr.rs:135-164` — relay_loop bug (will be deleted in Phase E, not patched)
- `ceremony-coordinator/src/handlers/upload.rs:145-159` — three-blob upload; `:250` — receipt-as-event-body publish
- `docker-compose.yml:41-50` — pre-fix blossom service block with no config mount
- `/var/lib/docker/volumes/onym_blossom-data/_data/sqlite.db` (live droplet) — blob index showing the 22 holes
