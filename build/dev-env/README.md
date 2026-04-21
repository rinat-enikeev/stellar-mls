# stellar-mls dev-env

arm64-native Docker dev environment that lets you drive Claude Code from an
iPhone over SSH. Two fully isolated agent containers (`qa-agent`,
`release-agent`) + an `n8n` container for automation. iOS builds and
Android JNI builds are delegated to the Mac mini host via a single
forced-command SSH dispatcher.

> **F-Droid reproducible builds are unaffected.** `build/dev-builder/`
> (amd64, pinned) still runs in `.github/workflows/release.yml`. This
> directory is a separate, additive tree for *interactive* development.

---

## Architecture

```
iPhone (Blink) ──SSH──► Mac mini :2222 (stellar-builder)
                           │  Xcode + fastlane + Match + darwin-arm64 NDK
                           │  Docker Desktop runs the dev compose stack
                           │
              ProxyJump ───┼──► qa-agent :2201  (Debian arm64 + Rust + Claude)
                           └──► release-agent :2202 (same image, ROLE=release)
                                      │
                                      │ forced-command SSH back to mac-host
                                      ▼
                          /Users/stellar-builder/bin/host-build-dispatch
                              $SSH_ORIGINAL_COMMAND = "ios <sha>" | "jnilibs <sha>"

                          n8n container :5678 (orchestration net only)
                              └── SSHes into agents for headless work
```

Key trust rules:

- Container keys on the Mac are pinned to
  `command="/Users/stellar-builder/bin/host-build-dispatch",restrict` —
  a compromised container can run the dispatcher and nothing else.
- Match / Keychain signing lives **only** in the `stellar-builder` login
  keychain. Containers never see signing creds.
- Android keystore stays in GitHub Actions secrets; release agent just
  pushes a tag and `.github/workflows/release.yml` takes over.
- Public port 2222 is the only inbound port. Everything else is
  loopback. **Tailscale is strongly recommended** instead of public
  SSH (same ProxyJump UX, zero public attack surface).

---

## Layout

```
build/dev-env/
├── Dockerfile.base              # arm64 Debian + JDK 21 + Android SDK (no NDK) + Rust
├── Dockerfile.agent             # + sshd + Claude Code + gh + agent user
├── docker-compose.dev.yml       # qa-agent, release-agent, n8n
├── qa.env.example               # copy → qa.env, fill in
├── release.env.example
├── n8n.env.example
├── ssh/sshd_config              # hardened config baked into the agent image
├── entrypoint/
│   ├── agent-entrypoint.sh      # runs as PID 1
│   └── first-run.sh             # keygen + workspace clone (idempotent)
├── bin/
│   ├── up.sh                    # build base if missing + compose up -d --build
│   ├── down.sh
│   ├── reset-agent.sh qa|release
│   ├── doctor.sh                # PASS/FAIL checklist
│   ├── remote-xcodebuild.sh     # baked to /usr/local/bin/remote-xcodebuild in container
│   └── remote-jnilibs.sh        # baked to /usr/local/bin/remote-jnilibs
├── n8n/
│   ├── README.md                # workflow specs (6 workflows — see docs/design-n8n-agent-round-trip.md for the round-trip)
│   └── workflows/               # exported JSON (manual import in n8n UI)
└── host/                        # deployed manually to the Mac mini
    ├── host-build-dispatch.sh
    ├── stellar-builder-bootstrap.sh   # copy-paste playbook
    └── launchd/com.stellar.devenv.plist
```

---

## Mac mini rollout (do this once, in order)

Every step runs on the Mac mini itself. Some as an admin user, some as
the dedicated `stellar-builder` user. Work through
`host/stellar-builder-bootstrap.sh` — each block is commented with who
runs it. Summary:

### 1. Create `stellar-builder`

Admin user → System Settings → Users & Groups → Add a Standard user
`stellar-builder`. Log in once to finish Apple's first-run dance.

### 2. Xcode + tooling (as stellar-builder)

```bash
xcode-select --install
sudo xcodebuild -license accept
```

**Homebrew**: if `/opt/homebrew` doesn't exist yet, install it fresh:

```bash
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

If `/opt/homebrew` already exists (installed earlier under an admin
account), the installer fails with *"You should change the ownership of
these directories to your user."* **Do not re-run the installer.** Pick
one of:

- **Option A — hand brew to stellar-builder** (simplest; admin account
  loses write access to brew):
  ```bash
  # run from your admin account
  sudo chown -R stellar-builder:staff /opt/homebrew
  ```
- **Option B — share brew between admin + stellar-builder via a group**:
  ```bash
  # run from your admin account
  sudo dseditgroup -o create brew
  sudo dseditgroup -o edit -a <admin-username> -t user brew
  sudo dseditgroup -o edit -a stellar-builder -t user brew
  sudo chgrp -R brew /opt/homebrew
  sudo chmod -R g+rwX /opt/homebrew
  sudo find /opt/homebrew -type d -exec chmod g+s {} \;
  ```

Then, back as `stellar-builder`, put brew on PATH and install the
tooling:

```bash
echo 'eval "$(/opt/homebrew/bin/brew shellenv)"' >> ~/.zprofile
eval "$(/opt/homebrew/bin/brew shellenv)"
brew --version

brew install xcodegen fastlane cocoapods gh sshguard rustup-init
rustup-init -y --default-toolchain 1.94.1 --profile minimal
rustup target add aarch64-linux-android x86_64-linux-android \
                  aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin
```

### 3. Android SDK + NDK (as stellar-builder)

> **Note on NDK architecture.** `sdkmanager --install "ndk;27.2.12479018"`
> ships a `darwin-x86_64` prebuilt only — there is no `darwin-arm64/`
> subdirectory at that version. On Apple Silicon, the x86_64 clang runs
> transparently under Rosetta 2, and `scripts/build-android.sh` already
> falls back to `darwin-x86_64/` when the native arch dir is missing.
> This is fine: the heavy lifting (Rust codegen) is still native arm64;
> only the linker step passes through Rosetta. Make sure Rosetta 2 is
> installed.

```bash
softwareupdate --install-rosetta --agree-to-license   # usually already installed

export ANDROID_HOME="$HOME/Library/Android/sdk"
mkdir -p "$ANDROID_HOME/cmdline-tools"
curl -sSLo /tmp/cmdline.zip \
  "https://dl.google.com/android/repository/commandlinetools-mac-11076708_latest.zip"
unzip -q /tmp/cmdline.zip -d "$ANDROID_HOME/cmdline-tools"
mv "$ANDROID_HOME/cmdline-tools/cmdline-tools" "$ANDROID_HOME/cmdline-tools/latest"
export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
yes | sdkmanager --licenses >/dev/null
sdkmanager --install "platform-tools" "platforms;android-34" \
                     "build-tools;34.0.0" "ndk;27.2.12479018"

# Persist in ~/.zprofile:
{
  echo 'export ANDROID_HOME="$HOME/Library/Android/sdk"'
  echo 'export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.2.12479018"'
  echo 'export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"'
} >> ~/.zprofile

ls "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin/clang" \
  && echo "NDK (darwin-x86_64 under Rosetta): OK"
```

### 4. fastlane Match (as stellar-builder, one-time)

The repo is private, so first authenticate `gh` — it registers a git
credential helper so regular `git` commands just work afterwards. No
SSH key needed.

```bash
gh auth login
# Pick: GitHub.com → HTTPS → Login with a web browser
# gh prints an 8-char code; open the URL on any device, paste the code.

gh repo clone rinat-enikeev/stellar-mls ~/src/stellar-mls
cd ~/src/stellar-mls/clients

# Install a modern Ruby via brew (macOS system Ruby 2.6 is EOL and read-only).
brew install ruby
echo 'export PATH="/opt/homebrew/opt/ruby/bin:$PATH"' >> ~/.zprofile
export PATH="/opt/homebrew/opt/ruby/bin:$PATH"
gem env home   # should print a path under /opt/homebrew

# Gemfile.lock pins BUNDLED WITH 2.5.3 — install that exact bundler.
gem install bundler:2.5.3

bundle install --path vendor/bundle
bundle exec fastlane match development --readonly false
```

Signing certs and provisioning profiles land in stellar-builder's login
keychain. Containers never see these.

### 5. Bare repo + dispatcher

```bash
mkdir -p ~/work ~/bin ~/logs
git init --bare ~/work/stellar-mls.git

cp ~/src/stellar-mls/build/dev-env/host/host-build-dispatch.sh \
   ~/bin/host-build-dispatch
chmod +x ~/bin/host-build-dispatch
```

### 6. Harden sshd (as admin)

```bash
sudo systemsetup -setremotelogin on
sudo dseditgroup -o create -q com.apple.access_ssh 2>/dev/null || true
sudo dseditgroup -o edit -a stellar-builder -t user com.apple.access_ssh

sudo tee /etc/ssh/sshd_config.d/100-stellar.conf >/dev/null <<'CONF'
Port 2222
PermitRootLogin no
PasswordAuthentication no
KbdInteractiveAuthentication no
ChallengeResponseAuthentication no
UsePAM yes
AllowUsers stellar-builder
AuthenticationMethods publickey
MaxAuthTries 3
LoginGraceTime 20
ClientAliveInterval 60
ClientAliveCountMax 3
CONF
sudo launchctl kickstart -k system/com.openssh.sshd

brew services start sshguard
```

Open **only** 2222/tcp inbound at the macOS firewall. Everything else
stays blocked. (Or skip all of this and put the Mac behind Tailscale —
preferred.)

### 7. Seed authorized_keys (as stellar-builder)

```bash
mkdir -p ~/.ssh && chmod 700 ~/.ssh
touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys
```

- **Your iPhone key** — plain entry, no `command=` restriction:
  ```
  ssh-ed25519 AAAA...iphone-pubkey... iphone
  ```
- **Container keys** — added *after* first boot of the compose stack
  (see §9 below), each prefixed:
  ```
  command="/Users/stellar-builder/bin/host-build-dispatch",restrict ssh-ed25519 AAAA... qa-agent
  command="/Users/stellar-builder/bin/host-build-dispatch",restrict ssh-ed25519 AAAA... release-agent
  ```

### 8. Build the base image + env files

```bash
cd ~/src/stellar-mls/build/dev-env

docker build --platform=linux/arm64 \
  -f Dockerfile.base -t stellar-mls/dev-env-base:latest .

cp qa.env.example qa.env
cp release.env.example release.env
cp n8n.env.example n8n.env
chmod 600 qa.env release.env n8n.env
# Fill in GITHUB_TOKEN, ANTHROPIC_API_KEY, IPHONE_SSH_PUBKEY in each file.
```

### 9. Bring up the stack + authorize containers

```bash
./bin/up.sh
```

After the first boot, `up.sh` prints each agent's freshly-generated
ed25519 public key. Copy both into
`/Users/stellar-builder/.ssh/authorized_keys`, each line prefixed with
`command="/Users/stellar-builder/bin/host-build-dispatch",restrict`.

Restart the agents so they re-exec sshd cleanly:
```bash
docker compose -f docker-compose.dev.yml restart qa-agent release-agent
```

### 10. Optional: launchd auto-start

```bash
sed -i '' "s|/Users/stellar-builder/Developer/stellar-mls|$HOME/src/stellar-mls|" \
  host/launchd/com.stellar.devenv.plist
cp host/launchd/com.stellar.devenv.plist ~/Library/LaunchAgents/
launchctl load -w ~/Library/LaunchAgents/com.stellar.devenv.plist
```

Requires Docker Desktop (or OrbStack) to auto-start at login.

### 11. n8n credentials + workflows

```bash
cp -R n8n/secrets.example n8n/secrets
chmod 600 n8n/secrets/*.json
# Fill in: ssh-qa-agent.json, ssh-release-agent.json,
#          github-qa-bot.json, github-release-bot.json
#          (see n8n/secrets.example/README.md for field-by-field guide)

./bin/n8n-deploy.sh
```

`n8n-deploy.sh` imports every credential + workflow into
`stellar-n8n`, rewrites `REPLACE_ME` IDs to real ones, activates the
six workflows, and prints each webhook URL.

You still need to create the owner account at
http://127.0.0.1:5678 (UI, one-time) and point a cloudflared tunnel at
`http://stellar-n8n:5678` — set the resulting public URL as
`WEBHOOK_URL` in `n8n.env` before running the deploy script so the
printed webhook URLs are correct. Full playbook: `n8n/README.md`.

### 12. qa-agent SSH trust for onym.chat build hosting (optional)

The `/build android` flow publishes a landing page for each debug
build at `https://onym.chat/build/pr-<N>/<short-sha>/`. qa-agent
reaches the droplet over SSH using the alias `droplet`. If this trust
is not set up, the build still succeeds and the PR comment still
includes the GitHub download link — only the onym.chat link is
omitted.

One-time setup (run on the Mac host, then into qa-agent):

```bash
# 1. Generate a dedicated key inside qa-agent.
docker exec -it stellar-qa-agent bash -lc '
  ssh-keygen -t ed25519 -N "" -f /home/agent/.ssh/droplet -C "qa-agent@droplet"
  cat >> /home/agent/.ssh/config <<CFG
Host droplet
    HostName <droplet-ip-or-dns>
    User root
    IdentityFile /home/agent/.ssh/droplet
    StrictHostKeyChecking accept-new
CFG
  chmod 600 /home/agent/.ssh/config /home/agent/.ssh/droplet
  cat /home/agent/.ssh/droplet.pub
'

# 2. Add the printed pub key to the droplet:
ssh root@<droplet-ip> 'cat >> /root/.ssh/authorized_keys' < <the-pub-key>

# 3. On the droplet, create the persistent build-hosting directory
#    (bind-mounted read-only into nginx by docker-compose.yml).
ssh root@<droplet-ip> 'mkdir -p /opt/onym-chat-builds && chmod 755 /opt/onym-chat-builds'

# 4. Rolling restart the nginx container so the new bind mount is picked up:
ssh root@<droplet-ip> 'cd /opt/onym-chat && docker compose up -d nginx'

# 5. Verify trust from qa-agent:
docker exec stellar-qa-agent ssh -o BatchMode=yes droplet 'echo ok'
```

---

## Verification

Run the structural checklist:

```bash
./bin/doctor.sh
```

Then the end-to-end smoke tests:

1. **SSH in from the iPhone.**
   ```
   ssh qa-agent
   # → tmux session "qa" attaches
   claude --version
   gh auth status
   ```

2. **Rust native build inside the container** — the whole reason we went
   arm64-native:
   ```bash
   cd ~/workspace
   cargo build --release -p <any-core-crate>
   ```

3. **Android JNI delegation** (from inside `qa-agent`):
   ```bash
   cd ~/workspace
   git commit --allow-empty -m "jnilibs smoke"
   git push origin HEAD
   SHA=$(git rev-parse HEAD)
   remote-jnilibs "$SHA"
   file build/android/jniLibs/arm64-v8a/*.so   # → ELF ARM aarch64
   ./gradlew :app:assembleRelease
   ```

4. **iOS delegation** (from inside `qa-agent`):
   ```bash
   remote-xcodebuild "$SHA"    # logs stream over stderr; ARTIFACT: line on success
   ```

5. **End-to-end n8n flow**: open a dummy issue on GitHub, assign
   `@programyzer` (or add the `agent-task` label), watch workflow 01
   open a PR within ~2 min with `@releaseng` auto-requested as
   reviewer. Workflow 04 should then post a Claude review within
   ~60s. Leave a PR review comment — workflow 05 pushes a follow-up
   commit. Comment `/merge` as `@alexpovstin` — workflow 06 merges
   and files a smoke-test issue, then workflow 02 tags the release
   and `release.yml` runs on GitHub. See
   `docs/design-n8n-agent-round-trip.md` for the full verification
   checklist.

---

## Day-to-day commands

```bash
./bin/up.sh                     # bring the stack up (builds on first run)
./bin/down.sh                   # stop + remove containers (keeps volumes)
./bin/reset-agent.sh qa         # nuke qa-agent's volumes and rebuild
./bin/doctor.sh                 # structural + reachability checks
./bin/n8n-deploy.sh             # (re)push workflows + credentials into n8n
docker compose -f docker-compose.dev.yml logs -f qa-agent
```

Reach an agent from the Mac directly (bypass ProxyJump):
```bash
ssh -p 2201 agent@127.0.0.1
ssh -p 2202 agent@127.0.0.1
```

iPhone `~/.ssh/config` (import via iCloud into Blink):
```
Host mac-host
    HostName <public-dns-or-tailscale-name>
    Port 2222
    User stellar-builder

Host qa-agent
    HostName 127.0.0.1
    Port 2201
    User agent
    ProxyJump mac-host
    RequestTTY yes
    RemoteCommand tmux new-session -A -s qa

Host release-agent
    HostName 127.0.0.1
    Port 2202
    User agent
    ProxyJump mac-host
    RequestTTY yes
    RemoteCommand tmux new-session -A -s release
```

---

## What to never do

- **Do not** mount the host repo into the containers. Workspaces are
  cloned into named volumes so the two agents can't stomp each other.
- **Do not** put Match credentials or the Android keystore in any
  `.env` file. They live in the Mac login keychain / GitHub Actions
  secrets respectively.
- **Do not** mount `/var/run/docker.sock` into n8n. n8n talks to
  agents over SSH, not via Docker. This is deliberate — a docker
  socket mount is root on the host.
- **Do not** bypass the forced command by giving a container key
  shell access to `stellar-builder`. The `command=` + `restrict` pin
  is the whole security boundary.
- **Do not** create GitHub Releases manually from the release agent;
  push the tag and let `.github/workflows/release.yml` run.
