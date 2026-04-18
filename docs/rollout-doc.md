# Rolling out a ceremony to your own server

This doc walks a fresh clone of `stellar-mls` to a running public trusted
setup ceremony at `ceremony.<yourdomain>`. Two paths:

- **Path A — DigitalOcean + Cloudflare.** One script provisions the droplet,
  DNS, SSL, and brings every service up. What the upstream operator uses.
- **Path B — Any Linux host.** You bring a server and DNS; the script is
  replaced by ~8 manual steps.

The release of the participant-facing `ceremony_tool` binaries is separate
(GitHub Actions) and is covered at the end.

---

## Prerequisites

On your workstation:

- `git`, `docker`, `docker compose`
- `rustup` with a stable toolchain (for building the WASM verifier locally;
  the droplet never needs Rust)
- `wasm-pack`: `curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh`
- Path A only: `doctl` (`brew install doctl` or
  `snap install doctl --classic`) and a Cloudflare API token with
  `Zone.DNS edit` on your zone

On the server (Path B only):

- Ubuntu 22.04 / 24.04 or Debian 12, root or sudo
- Ports 22, 80, 443 reachable from the internet
- Docker + Docker Compose plugin

You will need:

- A domain you control (registrar pointed at Cloudflare for Path A, or
  wherever you manage DNS for Path B).
- A Nostr keypair to act as **coordinator identity**. Generate one with any
  Nostr client or with `ceremony_tool keygen` (the binary ships a
  generator). Keep the `nsec` secret; the `npub` (hex) is public.
- One or more Nostr pubkeys (hex) to act as **Phase 2 admins** — they are
  the only ones allowed to call `freeze`, `set-beacon`, and publish
  Phase 2 rounds.

---

## 1. Clone and pick your domain

```bash
git clone https://github.com/rinat-enikeev/stellar-mls.git
cd stellar-mls
```

The stock configuration assumes `onym.chat`. Replace it with yours across
all nginx vhost configs:

```bash
# macOS: use `gsed` (brew install gnu-sed) or add `''` after -i
sed -i 's/onym\.chat/yourdomain.com/g' deploy/nginx/conf.d/*.conf
```

The six subdomains the stack expects are:

| Host                            | Purpose                                |
|---------------------------------|----------------------------------------|
| `yourdomain.com`                | marketing site (`deploy/website`)      |
| `ceremony.yourdomain.com`       | ceremony UI + coordinator API          |
| `blossom.yourdomain.com`        | content-addressed blob store           |
| `nostr.yourdomain.com`          | strfry relay for the transcript        |
| `relay.yourdomain.com`          | stellar-mls relayer                    |
| `push.yourdomain.com`           | push notification relay (optional)     |

If you don't need all of them, delete the matching file under
`deploy/nginx/conf.d/` and the corresponding service in
`docker-compose.yml` — but `ceremony`, `blossom`, and `nostr` are the
minimum for a working ceremony.

---

## 2. Configure the coordinator

Copy the example env and fill in the admin + coordinator keys:

```bash
cp ceremony-coordinator/.env.example ceremony-coordinator/.env
$EDITOR ceremony-coordinator/.env
```

Minimum edits:

```ini
# Public URL of your Blossom, used in API responses so clients can fetch
# blobs directly without going through the coordinator.
CEREMONY_BLOSSOM_PUBLIC_URL=https://blossom.yourdomain.com

# Comma-separated hex pubkeys allowed to hit Phase 2 admin endpoints.
CEREMONY_ADMIN_PUBKEYS=d1a2...e9,b4c5...77

# Hex secret key the coordinator uses to sign kind-30078 transcript
# events. Never commit this.
CEREMONY_COORDINATOR_NSEC=<64-hex-chars>
```

Everything else in `.env.example` has working defaults for the dockerized
stack. Do not set `CEREMONY_ALLOW_BROWSER_CONTRIBUTE=true` on a public
deployment — browser-side δ generation is not safe.

---

## 3. Build the browser WASM verifier

The `/verify` page runs the six pairing equations client-side. The WASM
bundle has to be built on your workstation and shipped with the static
site (the server has no Rust toolchain):

```bash
bash deploy/ceremony/tools/build-wasm.sh
```

Output lands in `deploy/ceremony/wasm/` and is picked up by nginx's
`/wasm/` location. Commit nothing — the directory's `.gitignore` already
excludes the build artifacts.

---

## 4a. Path A — deploy to DigitalOcean

One script, first run takes ~10 minutes (DNS propagation is the slow part):

```bash
export DO_API_KEY=...                # DigitalOcean personal access token
export CF_API_TOKEN=...              # Cloudflare token, Zone.DNS:edit
export DOMAIN=yourdomain.com
export CERTBOT_EMAIL=you@yourdomain.com

bash deploy/digitalocean/deploy.sh
```

What it does, in order:

1. Uploads your `~/.ssh/id_ed25519.pub` to DO (reuses if present).
2. Creates an `s-2vcpu-4gb` Ubuntu 24.04 droplet in `ams3`
   (override via `DO_REGION`, `DO_DROPLET_SIZE`). Records the ID in
   `.env` so re-runs reuse the same box.
3. Creates/updates A records for the six subdomains on Cloudflare.
4. SCPs the repo's `deploy/`, `docker-compose.yml`, `ceremony-coordinator/`,
   and friends to `/opt/onym-chat` on the droplet.
5. `docker compose build --pull` on the droplet.
6. Bootstraps Let's Encrypt certs via
   `deploy/certbot/init-certs.sh` (self-signed placeholder first so nginx
   starts, then webroot ACME for the real cert covering all six SANs).
7. Smoke-tests each public URL and prints a summary.

Subsequent runs skip droplet/DNS/SSL bootstrap and only push updated
code + `docker compose up -d`.

---

## 4b. Path B — deploy to any Linux host

```bash
# On your workstation:
rsync -az --delete \
  deploy docker-compose.yml ceremony-coordinator rust-toolchain.toml \
  root@YOUR_HOST:/opt/ceremony/

# On the server:
cd /opt/ceremony
docker compose build --pull
```

Point these A records at the server's public IP at your DNS host (TTL 300):

```
yourdomain.com         A  <server-ip>
ceremony.yourdomain.com A  <server-ip>
blossom.yourdomain.com  A  <server-ip>
nostr.yourdomain.com    A  <server-ip>
```

Wait for propagation (`dig +short ceremony.yourdomain.com` returns your IP),
then bootstrap SSL + bring everything up:

```bash
bash deploy/certbot/init-certs.sh you@yourdomain.com yourdomain.com
docker compose up -d
```

The certbot sidecar renews automatically every 12 h; nginx reloads on
its own schedule.

---

## 5. Verify the stack is live

```bash
curl -fsS https://ceremony.yourdomain.com/api/v1/healthz
curl -fsS https://ceremony.yourdomain.com/                 # landing page
curl -fsSI https://ceremony.yourdomain.com/wasm/ceremony_wasm_bg.wasm \
  | grep -i content-type   # → application/wasm
```

Open `https://ceremony.yourdomain.com/` in a browser. The queue should
load; `/verify.html` should instantiate WASM without a console error.

---

## 6. Release `ceremony_tool` binaries

Participants don't build from source — they download signed binaries from
GitHub Releases and the coordinator shells out to the same binary it
served to them. To cut a release:

```bash
# 1. Tag (the workflow refuses untagged refs)
git tag -s v0.1.0 -m "ceremony_tool v0.1.0"
git push origin v0.1.0

# 2. Dispatch the release workflow
gh workflow run release-ceremony-tool.yml -f tag=v0.1.0
gh run watch
```

The matrix builds Linux (x86_64 + aarch64 musl), macOS (x86_64 +
aarch64), and Windows (x86_64 MSVC). For each artifact the workflow
publishes:

```
ceremony_tool-<tag>-<target>[.exe]
ceremony_tool-<tag>-<target>[.exe].sha256
ceremony_tool-<tag>-<target>[.exe].buildinfo.json
ceremony_tool-<tag>-<target>[.exe].minisig   # if MINISIGN_SECRET_KEY set
```

Optional minisign signing: add two repo secrets and the workflow will
sign every artifact.

```
gh secret set MINISIGN_SECRET_KEY < ~/.minisign/minisign.key
gh secret set MINISIGN_PASSWORD   < /dev/stdin   # paste, then Ctrl-D
```

Pin the minisign **public** key in three places so no single compromise
can switch it without tripping cross-checks — see
`docs/ceremony-tool-verification.md`:

- `deploy/ceremony/download.html`
- the GitHub Release notes
- a kind-30078 Nostr event on your relay with `d="sepceremony1:releasekeys"`

To confirm the released Linux binary is byte-for-byte reproducible:

```bash
./scripts/verify-ceremony-tool.sh v0.1.0 x86_64-unknown-linux-musl
```

Details in `docs/ceremony-reproducible-build.md`.

---

## 7. Kick off the ceremony

Phase 1 starts the moment the coordinator boots with an empty database —
the first participant to claim a slot on each tier contributes on top of
the canonical initial SRS. No manual bootstrap is needed.

When Phase 1 has enough contributors (plan calls for ≥1 honest
participant per tier, realistically target 20+), freeze and move to
Phase 2. Signed as one of the admin pubkeys you configured in step 2:

```bash
# Freeze Phase 1:
ceremony_tool admin freeze \
  --coordinator https://ceremony.yourdomain.com \
  --tier small

# Pin the beacon block (choose a future Bitcoin block height):
ceremony_tool admin set-beacon \
  --coordinator https://ceremony.yourdomain.com \
  --tier small --height 900000
```

Phase 2 contributors then use the snarkjs handoff helper, built from this
repo:

```bash
docker build -t onym/phase2-helper docker/phase2-helper
docker run --rm -v "$(pwd):/work" onym/phase2-helper contribute \
  --input  round04.zkey \
  --output round05.zkey \
  --name   "alice@keybase"
```

Upload the output via `/phase2.html` while signed in with an admin
Nostr key. `docs/phase2-mpc-integration.md` is the long-form playbook.

---

## Operational notes

- **Backups.** The coordinator's SQLite DB and the Blossom blob store
  both live in named Docker volumes (`ceremony-data`, `blossom-data`).
  The authoritative transcript is on the Nostr relay and in Blossom — a
  full rebuild from those two alone is supported and documented. Still
  take nightly snapshots (`docker run --rm -v ceremony-data:/src -v
  $(pwd):/dst alpine tar czf /dst/ceremony-$(date +%F).tar.gz -C /src .`).
- **Logs.** `docker compose logs -f ceremony-coordinator` streams
  structured logs; `RUST_LOG=ceremony_coordinator=debug` in the `.env`
  raises verbosity.
- **Upgrading.** `git pull && docker compose build && docker compose up -d`.
  Migrations under `ceremony-coordinator/migrations/` are applied
  automatically on boot via a `schema_migrations` table.
- **Teardown (DO).** `doctl compute droplet delete $DROPLET_ID` — the
  Cloudflare records and the DO SSH key are left alone so re-runs are
  cheap.

---

## Troubleshooting

| Symptom                                          | Likely cause / fix                                                                     |
|--------------------------------------------------|----------------------------------------------------------------------------------------|
| `/api/v1/healthz` returns 502                    | `ceremony-coordinator` container crashed — `docker compose logs ceremony-coordinator`. |
| `/verify.html` shows "WebAssembly failed to instantiate" | `deploy/ceremony/wasm/` was empty at deploy time — rerun `build-wasm.sh` and redeploy. |
| Let's Encrypt rate-limited during cert issue     | Wait an hour, or use Let's Encrypt staging: `STAGING=1 bash deploy/certbot/init-certs.sh ...` |
| `gh workflow run` says `no tag`                  | Push the tag first: `git push origin vX.Y.Z`.                                          |
| `deploy.sh` hangs on DNS propagation             | Cloudflare proxy is on — open the zone in the dashboard and set each A record to "DNS only" (grey cloud). |
| `admin freeze` returns 403                       | Your signing pubkey is not in `CEREMONY_ADMIN_PUBKEYS`. Update the env and restart the container. |
