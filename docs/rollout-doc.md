# Rolling out a ceremony from a fresh clone

This doc takes a just-cloned `stellar-mls` repo to **a live ceremony stack
at `ceremony.<yourdomain>` that is ready to accept contributions**. It
deliberately stops short of kicking the ceremony off — the coordinator
box must exist, be healthy, and have verified binaries published before
you pick the moment to announce Phase 1.

Two paths are supported and share every step except §6:

- **Path A — DigitalOcean + Cloudflare.** `deploy/digitalocean/deploy.sh`
  provisions the droplet, the DNS records, the TLS certs, and brings the
  stack up. This is what the upstream operator uses.
- **Path B — any Linux box you already own.** You bring SSH and DNS; the
  certbot init script does SSL and `docker compose up -d` does the rest.

Acceptance criteria are §8. If everything there is green, you are ready
to kick off the ceremony.

---

## 1. Prerequisites

On your workstation:

- `git`, `docker`, `docker compose`
- `wasm-pack` for the browser verifier:
  `curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh`
  (needs a stable Rust toolchain via `rustup`)
- Path A only: `doctl` (`brew install doctl` / `snap install doctl`)

On the server (Path B only):

- Ubuntu 22.04 / 24.04 or Debian 12, root or sudo
- Ports 22, 80, 443 open to the internet
- Docker + the Docker Compose plugin

Things you need to have on hand before you start:

- A domain you control.
- A Nostr keypair to act as **coordinator identity**. The `nsec` (hex
  secret) signs every kind-30078 transcript event. Generate one with any
  Nostr client or `ceremony_tool keygen`.
- One or more Nostr **admin pubkeys** (hex). These are the only
  identities that can call Phase 2 endpoints (`freeze`, `set-beacon`,
  publish round). Include your own.
- Path A only: a DigitalOcean API token and a Cloudflare token with
  `Zone.DNS edit` on your zone.

---

## 2. Clone and pick your domain

```bash
git clone https://github.com/rinat-enikeev/stellar-mls.git
cd stellar-mls
```

The stock config hardcodes `onym.chat` in a handful of non-env places.
Replace it with your domain across those files:

```bash
# nginx vhosts — hostnames + cert paths
sed -i '' 's/onym\.chat/yourdomain.com/g' deploy/nginx/conf.d/*.conf     # macOS
# sed -i   's/onym\.chat/yourdomain.com/g' deploy/nginx/conf.d/*.conf    # Linux

# docker-compose hardcodes the public Blossom URL that the coordinator
# returns in API responses — the browser uses it to pull blobs directly
sed -i '' 's|blossom\.onym\.chat|blossom.yourdomain.com|g' docker-compose.yml
```

The six subdomains the full stack uses:

| Host                            | Required for ceremony? | Served by              |
|---------------------------------|------------------------|------------------------|
| `yourdomain.com`                | no (marketing site)    | `deploy/website/`      |
| `ceremony.yourdomain.com`       | **yes**                | coordinator + static UI |
| `blossom.yourdomain.com`        | **yes**                | `blossom` container    |
| `nostr.yourdomain.com`          | **yes**                | `nostr-relay` (strfry) |
| `relay.yourdomain.com`          | no (Stellar relayer)   | `relayer` container    |
| `push.yourdomain.com`           | no (mobile push)       | `pn-relay` container   |

If you only care about the ceremony, you can delete
`deploy/nginx/conf.d/{relay,push}.onym.chat.conf` and the
`relayer` / `pn-relay` services in `docker-compose.yml`. Either way,
**`relay` and `push` still need stub `.env` files** because `deploy.sh`
SCPs them blindly (§6a note). That's covered below.

---

## 3. Fill in the `.env` files

The repo ships `.env.example` templates you copy and fill in. There are
four of them; only the first two matter for running a ceremony.

### 3a. Top-level `.env` — deploy-script config

```bash
cp .env.production.example .env
$EDITOR .env
```

What goes in it:

```ini
DOMAIN=yourdomain.com
CERTBOT_EMAIL=you@yourdomain.com

# Path A only (DigitalOcean + Cloudflare). Leave blank for Path B —
# deploy.sh will still prompt, but you won't run deploy.sh at all.
DO_API_KEY=
CF_API_TOKEN=
DO_REGION=ams3
DO_DROPLET_SIZE=s-2vcpu-4gb
SSH_KEY_PATH=~/.ssh/id_ed25519
```

`deploy.sh` rewrites this file on every run and appends `DROPLET_ID` and
`DROPLET_IP` after the first successful provision, so it stays idempotent
across re-runs. Commit nothing from here — `.env` is gitignored.

### 3b. `ceremony-coordinator/.env` — coordinator secrets

```bash
cp ceremony-coordinator/.env.example ceremony-coordinator/.env
$EDITOR ceremony-coordinator/.env
```

Most lines in the example are already correct for the dockerized stack
(`CEREMONY_BIND`, `CEREMONY_DB_PATH`, the internal `http://blossom:3000`
and `ws://nostr-relay:7777` URLs, the binary path). Those values are
*also* set inline in `docker-compose.yml` and win over the `.env` file —
leaving them in the file is harmless.

The lines you **must** fill in:

```ini
# Hex Nostr pubkeys (no npub prefix) allowed to hit admin endpoints.
# Include your own. Comma-separated, no spaces.
CEREMONY_ADMIN_PUBKEYS=d1a2...e9,b4c5...77

# Hex Nostr secret key the coordinator uses to sign kind-30078
# transcript events. NEVER commit. If blank, the coordinator boots
# read-only and refuses to commit rounds.
CEREMONY_COORDINATOR_NSEC=<64 hex chars>
```

Lines you may want to tune:

```ini
CEREMONY_SLOT_DEADLINE_SECS=7200     # 2h per slot; lower for a dry run
CEREMONY_POW_BITS=8                  # signup anti-spam difficulty
CEREMONY_RATE_LIMIT_RPM=60
CEREMONY_ALLOW_BROWSER_CONTRIBUTE=false   # LEAVE false in production
```

### 3c. `relayer/.env` — only if you use the Stellar relayer

```bash
cp relayer/.env.example relayer/.env
$EDITOR relayer/.env
```

If you don't run the Stellar relayer, `deploy.sh` still requires the file
to exist (it SCPs it unconditionally). Either leave the example values in
place (the container will fail to connect — harmless, it's isolated on
the internal network), or delete the `relayer` service from
`docker-compose.yml` and create an empty stub:

```bash
touch relayer/.env
```

### 3d. `pn-relay/.env` — only if you use mobile push

```bash
cp pn-relay/.env.example pn-relay/.env   # optional
# or:
touch pn-relay/.env                      # empty stub, container noops
```

Not used by the ceremony. Skip unless you also run the mobile clients.

---

## 4. Build the browser WASM verifier

`/verify.html` runs the six pairing equations client-side. The WASM
bundle is built on your workstation (the server has no Rust toolchain)
and shipped as part of `deploy/ceremony/`:

```bash
bash deploy/ceremony/tools/build-wasm.sh
```

Output:

```
deploy/ceremony/wasm/ceremony_wasm.js
deploy/ceremony/wasm/ceremony_wasm_bg.wasm   # ~155 KB (62 KB gz)
deploy/ceremony/wasm/ceremony_wasm.d.ts
```

The directory is gitignored except for `.gitkeep`; rebuild before every
deploy. Path A's `deploy.sh` runs this script for you, but it warns and
continues on failure — run it manually once first to confirm your
toolchain is set up.

---

## 5. Release `ceremony_tool` binaries

Participants will download these binaries from GitHub Releases; the
coordinator shells out to **the same binary** (built into its image from
this repo) to verify uploads. You should cut the release before you
announce the ceremony so `/download.html` points at real artifacts.

```bash
# 1. Create a signed tag (the workflow only accepts existing tags):
git tag -s v0.1.0 -m "ceremony_tool v0.1.0"
git push origin v0.1.0

# 2. Fire the release workflow:
gh workflow run release-ceremony-tool.yml -f tag=v0.1.0
gh run watch
```

The matrix publishes five targets: Linux x86_64/aarch64 musl, macOS
x86_64/aarch64, Windows x86_64 MSVC. Per artifact you get the binary,
`.sha256`, `.buildinfo.json`, and (optionally) `.minisig`.

Optional but recommended — add minisign signing:

```bash
minisign -G -p minisign.pub -s minisign.key   # one-time keygen
gh secret set MINISIGN_SECRET_KEY < minisign.key
gh secret set MINISIGN_PASSWORD                # paste passphrase, Ctrl-D
```

Then pin the **public** key in three places so a single compromise can't
switch it silently — full instructions in
`docs/ceremony-tool-verification.md`:

- `deploy/ceremony/download.html` (the text block next to the downloads)
- the GitHub Release's release notes
- a kind-30078 Nostr event on your relay with
  `d="sepceremony1:releasekeys"`

Reproducibility: `./scripts/verify-ceremony-tool.sh v0.1.0
x86_64-unknown-linux-musl` rebuilds the Linux binary in the pinned
Docker image and diffs against the published `buildinfo.json`. See
`docs/ceremony-reproducible-build.md`.

---

## 6a. Path A — deploy to DigitalOcean

Everything you edited in §§2–4 is on disk; the four `.env` files are
filled. Run:

```bash
bash deploy/digitalocean/deploy.sh
```

First run takes ~10 minutes (DNS propagation dominates). What it does:

1. Uploads `~/.ssh/id_ed25519.pub` to DO (reuses the key if present).
2. Creates an `s-2vcpu-4gb` Ubuntu 24.04 droplet in `ams3` if no
   `DROPLET_ID` is saved in `.env` yet. Cloud-init installs Docker and
   opens the firewall.
3. Calls the Cloudflare API to create/update A records for
   `@`, `relay`, `nostr`, `blossom`, `push`, `ceremony`.
4. `git clone`s the repo on the droplet into `/opt/onym-chat`.
5. Re-runs `deploy/ceremony/tools/build-wasm.sh` locally and SCPs the
   local `deploy/`, `docker-compose.yml`, and every service's `.env` /
   `Dockerfile` / `src/` on top of the clone. Your uncommitted edits win
   over whatever is on `origin/main`.
6. `docker compose build --pull` on the droplet.
7. If `/etc/letsencrypt/live/$DOMAIN/fullchain.pem` doesn't exist, waits
   for DNS propagation, then runs
   `deploy/certbot/init-certs.sh "$CERTBOT_EMAIL" "$DOMAIN"` to place a
   self-signed placeholder, start nginx, and request the real cert via
   webroot ACME.
8. `docker compose up -d` and smoke-tests each public URL.

Subsequent runs skip droplet/DNS/SSL bootstrap and just redeploy code.

---

## 6b. Path B — deploy to your own Linux box

Point your DNS at the server. Minimum A records (TTL 300):

```
yourdomain.com          A  <server-ip>
ceremony.yourdomain.com A  <server-ip>
blossom.yourdomain.com  A  <server-ip>
nostr.yourdomain.com    A  <server-ip>
```

Wait for propagation:

```bash
dig +short ceremony.yourdomain.com   # must return <server-ip>
```

Rsync the tree up and build:

```bash
rsync -az --delete \
  deploy docker-compose.yml ceremony-coordinator relayer pn-relay \
  rust-toolchain.toml .env \
  root@YOUR_HOST:/opt/ceremony/

ssh root@YOUR_HOST 'cd /opt/ceremony && docker compose build --pull'
```

Bootstrap SSL, then bring everything up:

```bash
ssh root@YOUR_HOST 'cd /opt/ceremony && \
  bash deploy/certbot/init-certs.sh "you@yourdomain.com" "yourdomain.com" && \
  docker compose up -d'
```

The certbot sidecar renews every 12 h.

---

## 7. Smoke tests

Run these from your workstation. Any non-200 / missing field is a stop
sign.

```bash
# Coordinator is up and the reverse proxy is routing:
curl -fsS https://ceremony.yourdomain.com/api/v1/healthz
# → "ok"

# Queue state is reachable and all three tiers are initialised:
curl -fsS https://ceremony.yourdomain.com/api/v1/status | jq '.tiers | keys'
# → ["large","medium","small"]

# Static landing page:
curl -fsSI https://ceremony.yourdomain.com/ | head -1
# → HTTP/2 200

# WASM verifier served with the right MIME type:
curl -fsSI https://ceremony.yourdomain.com/wasm/ceremony_wasm_bg.wasm \
  | grep -i '^content-type'
# → content-type: application/wasm

# Blossom is reachable and returns its kind-0 info doc:
curl -fsS https://blossom.yourdomain.com/ | head -c 120

# Nostr relay accepts websocket upgrades:
curl -fsSI -H 'Connection: Upgrade' -H 'Upgrade: websocket' \
  https://nostr.yourdomain.com/ | head -1
# → HTTP/2 101 (or a 426 from nginx if no wss — both mean the hop is live)

# GitHub Release has the binaries the UI will link to:
gh release view v0.1.0 --json assets --jq '.assets[].name' | sort
# → ceremony_tool-v0.1.0-aarch64-apple-darwin
#   ceremony_tool-v0.1.0-aarch64-apple-darwin.buildinfo.json
#   ...one buildinfo + sha256 per target, 5 targets
```

Open `https://ceremony.yourdomain.com/verify.html` in a browser. Open
DevTools → Console. You should see the WASM module instantiate and no
red errors. The page will say "no round selected yet" — that's fine,
the ceremony hasn't started.

---

## 8. Acceptance checklist — ready to kick off

You are ready to announce and kick off the ceremony when every box is
checked:

- [ ] `https://ceremony.<domain>/api/v1/healthz` returns `ok`.
- [ ] `https://ceremony.<domain>/api/v1/status` lists all three tiers
      (`small`, `medium`, `large`) with empty queues.
- [ ] `https://ceremony.<domain>/` landing page loads and the "For
      Humans / For Mathematicians" toggle works.
- [ ] `https://ceremony.<domain>/verify.html` instantiates WASM without
      console errors.
- [ ] `https://ceremony.<domain>/download.html` lists a binary for every
      platform you intend to support, and every SHA-256 on the page
      matches the corresponding `.sha256` sibling on the GitHub Release.
- [ ] `https://blossom.<domain>/` is reachable and writable by the
      coordinator (tested by looking at `docker compose logs
      ceremony-coordinator` for a successful boot-time probe).
- [ ] `wss://nostr.<domain>/` accepts connections.
- [ ] `gh release view vX.Y.Z` shows all five targets with minisign
      signatures (if you enabled minisign).
- [ ] The minisign public key you'll pin is identical on (a)
      `/download.html`, (b) the GitHub Release notes, (c) the
      `sepceremony1:releasekeys` Nostr event on your relay. (Skip if you
      aren't using minisign.)
- [ ] Your Nostr pubkey is in `CEREMONY_ADMIN_PUBKEYS` and the
      coordinator picked it up:
      `docker compose exec ceremony-coordinator env | grep ADMIN_PUBKEYS`.
- [ ] Nightly backups of `ceremony-data` and `blossom-data` volumes are
      scheduled somewhere. One-shot example:
      ```bash
      docker run --rm -v onym_ceremony-data:/src -v "$(pwd)":/dst alpine \
        tar czf /dst/ceremony-$(date +%F).tar.gz -C /src .
      ```

When all of the above is green, kicking off the ceremony is a matter of
announcing the coordinator URL to participants — Phase 1 auto-bootstraps
from the canonical initial SRS as soon as the first slot is claimed on
each tier. That announcement is out of scope for this doc.

---

## Troubleshooting

| Symptom                                                   | Likely cause / fix                                                                 |
|-----------------------------------------------------------|------------------------------------------------------------------------------------|
| `/api/v1/healthz` returns 502                             | Coordinator crashed — `docker compose logs ceremony-coordinator`. Most common cause: `CEREMONY_COORDINATOR_NSEC` is not 64 hex chars. |
| `/api/v1/status` returns 500 with "admin allowlist empty" | `CEREMONY_ADMIN_PUBKEYS` wasn't picked up. Restart the container after editing the `.env`: `docker compose up -d ceremony-coordinator`. |
| `/verify.html` shows "WebAssembly failed to instantiate"  | `deploy/ceremony/wasm/` was empty at deploy time — rerun `build-wasm.sh` and redeploy. |
| `gh workflow run` errors `no tag`                         | Push the tag first: `git push origin vX.Y.Z`.                                      |
| `deploy.sh` hangs on DNS propagation                      | Cloudflare proxy is on — open the zone and set each A record to "DNS only" (grey cloud). |
| Let's Encrypt rate-limited during cert issue              | Wait an hour. To iterate in the meantime, edit `deploy/certbot/init-certs.sh` to pass `--staging` to certbot. |
| Coordinator boots but `/api/v1/status` 404s               | You forgot to rewrite `CEREMONY_BLOSSOM_PUBLIC_URL` in `docker-compose.yml` — the inline block there overrides `.env`. Re-sed, redeploy. |
| `docker compose build` fails on `ceremony-coordinator`    | You skipped §4. Run `build-wasm.sh` — the coordinator image copies the built WASM into the frontend tree during its own build. |
