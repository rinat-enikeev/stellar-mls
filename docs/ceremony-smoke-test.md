# Ceremony smoke test — first contribution on tier `small`

End-to-end walkthrough to prove the production coordinator at
`ceremony.onym.chat` works: seed round 0, claim the slot, contribute locally
with `ceremony_tool`, upload, and confirm the round lands in SQLite,
Blossom, and the Nostr transcript.

## Preflight (operator, one-time per tier)

The coordinator refuses slot claims until round 0 exists for the tier.
Use the `seed` subcommand baked into the coordinator image — it runs
`ceremony_tool init`, uploads the three files to Blossom, inserts the
`rounds` row, and (if `CEREMONY_COORDINATOR_NSEC` is set) publishes the
kind-30078 transcript event. Idempotent: re-running on a seeded tier
exits 0 with a "nothing to do" message.

```bash
ssh root@onym.chat
cd /opt/ceremony
docker compose exec ceremony-coordinator ceremony-coordinator seed small
```

Expected output:

```
seeded small round 0
  srs_hash         <hex>
  contribution_id  sepceremony1:small:r0:<hex>
  state.srs blob   https://blossom.onym.chat/<hex>
  state.txt blob   https://blossom.onym.chat/<hex>
  receipt.txt blob https://blossom.onym.chat/<hex>
  nostr_event_id   <64-char hex>
```

Verify:

```bash
curl -s https://ceremony.onym.chat/api/v1/status | jq '.tiers[] | select(.tier=="small")'
# expect: "head_round": 0
```

Repeat `seed medium` and `seed large` when you want those tiers live.

---

## Smoke test (participant)

### 1. Sign in

Install [Alby](https://getalby.com/) or [nos2x](https://github.com/fiatjaf/nos2x).
Visit `https://ceremony.onym.chat/contribute.html`, click **Connect
Nostr**, approve the pubkey prompt. The page shows your npub hex.

### 2. Join the `small` queue

Click the `small` tier card, then **Join small queue**. Your status flips
to `queued`, position `#1`.

### 3. Claim the slot

Because you're at the head of the queue, the queued card now shows a
**Take your turn** button. Click it. The UI POSTs to
`/api/v1/tiers/small/claim` with your NIP-98 header, the coordinator
flips your signup row to `claimed` with a 2-hour deadline, and the page
advances to step 4 with the previous-round blob URLs and the
`ceremony_tool contribute` command to run.

### 4. Run the tool locally

Download the three files into `./prev/`, keeping the filenames
`state.srs`, `state.txt`, `receipt.txt`. Then:

```bash
ceremony_tool contribute \
  --state-dir ./prev \
  --out-dir ./mine \
  --participant <your-npub-hex>
```

Run this on an **ephemeral or air-gapped machine** — the `OsRng` scalar
never touches disk, but the process memory is where the toxic waste
briefly lives.

### 5. Upload

Back on the contribute page (still at step 4), use the three file pickers
to select `./mine/state.srs`, `./mine/state.txt`, `./mine/receipt.txt`.
Click **Submit contribution**.

The coordinator will:

1. Fetch round-0 artifacts from Blossom.
2. Shell out to `ceremony_tool verify-contribution` against your files.
3. PUT your files to Blossom.
4. Insert `rounds(tier='small', round=1, ...)`.
5. Flip your signup row to `committed`.
6. Publish the `kind 30078` transcript event to
   `wss://nostr.onym.chat`.

The UI flips to the "Thank you" card with your round number.

### 6. Verify the round landed

Coordinator view:

```bash
curl -s https://ceremony.onym.chat/api/v1/status | jq '.tiers[] | select(.tier=="small")'
# expect: "head_round": 1

curl -s https://ceremony.onym.chat/api/v1/tiers/small/rounds | jq
# expect: two entries (round 0 seed + round 1 yours)
```

Nostr transcript:

```bash
nak req -k 30078 -t d=sepceremony1:small:r1 wss://nostr.onym.chat
# expect: one event; content is receipt.txt; tags include blob refs
#         for state.srs, state.txt, receipt.txt
```

Browser verify page:

`https://ceremony.onym.chat/verify.html?tier=small&round=1` — once the
WASM crate lands, this re-runs the six pairing equations locally against
bytes pulled directly from Blossom (no coordinator trust).

---

## Cleanup if something breaks mid-test

The coordinator image doesn't ship `sqlite3`, so database inspection
runs against the mounted volume from the host. Locate it once:

```bash
DB=$(docker volume inspect onym_ceremony-data \
       | jq -r '.[0].Mountpoint')/ceremony.db
```

If you claimed a slot but never uploaded, the signup row blocks the
queue for yourself until the 2-hour deadline expires. Either wait it
out, or force-expire:

```bash
sqlite3 "$DB" \
  "UPDATE signups SET status = 'expired' \
   WHERE tier = 'small' AND status = 'claimed' AND pubkey = '<your-npub-hex>';"
```

If the upload failed verification, the coordinator logs `verify_fail` in
the `events` table with the subprocess stderr:

```bash
sqlite3 "$DB" \
  "SELECT at, kind, detail_json FROM events \
   WHERE kind LIKE 'verify%' ORDER BY id DESC LIMIT 5;"
```
