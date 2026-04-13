# n8n workflows for the stellar-mls dev-env

n8n runs in its own container (`stellar-n8n`) on the `orchestration`
docker network alongside `qa-agent` and `release-agent`. It reaches the
agents over SSH at `qa-agent:22` / `release-agent:22` using a dedicated
key pair stored in n8n's encrypted credential store — **never** via the
Docker socket.

Three workflows live under `workflows/`. They're JSON exports of n8n
flows, but the specs below describe what each one does in plain terms so
you (or a future agent) can rebuild them from scratch in the n8n UI.

---

## One-time n8n setup

1. Open http://127.0.0.1:5678 via ProxyJump from the Mac (or iPhone
   through the Mac). Create the owner account.
2. **SSH credential**: inside n8n, create an "SSH" credential named
   `agent-ssh`. Generate a fresh ed25519 key from the n8n UI, then paste
   the corresponding public key into each agent's `.env` as
   `N8N_SSH_PUBKEY=...`. Restart the agents so `first-run.sh` adds it to
   `authorized_keys`.
3. **GitHub App credentials**: register two GitHub Apps (one per role),
   each scoped to the `stellar-mls` repo:
   - **qa-bot**: Contents (RW), Pull requests (RW), Issues (RW)
   - **release-bot**: Contents (RW), Pull requests (R), Metadata (R)

   Store both as n8n "GitHub App" credentials. Webhook URLs (from the
   cloudflared tunnel) go on the qa-bot App only; release-bot doesn't
   need inbound webhooks.
4. **cloudflared tunnel**: create a tunnel pointing at
   `http://stellar-n8n:5678` from the Mac host (or loopback
   `http://127.0.0.1:5678`). Point the tunnel hostname at n8n's webhook
   base URL in n8n's environment (`WEBHOOK_URL=https://<tunnel>/`).

Once credentials exist, import or rebuild the three workflows below.

---

## Workflow 1 — `01-qa-agent-issue`

**Trigger**: GitHub webhook, `issues` event.

**Filter**: `action == "labeled"` AND `label.name == "agent-task"`.

**Steps**:

1. **Extract fields** (Set node):
   - `issue_number = {{ $json.issue.number }}`
   - `issue_title = {{ $json.issue.title }}`
   - `issue_body = {{ $json.issue.body }}`
   - `repo_full_name = {{ $json.repository.full_name }}`

2. **SSH to qa-agent** (SSH node, credential `agent-ssh`, host `qa-agent`,
   port `22`, user `agent`):

   ```bash
   set -euo pipefail
   cd ~/workspace
   git fetch origin main
   git checkout -B "agent/issue-${issue_number}" origin/main

   # Headless Claude Code — no interactive prompts.
   claude --print --dangerously-skip-permissions \
     --system-prompt-file /etc/claude/qa-system.md \
     "$(cat <<'PROMPT'
   Issue #${issue_number}: ${issue_title}

   ${issue_body}

   Implement a fix, write or update tests, commit with a descriptive
   message, and exit. Do not push — the workflow will handle push and PR.
   PROMPT
   )"

   git push -u origin "agent/issue-${issue_number}"
   gh pr create \
     --base main \
     --head "agent/issue-${issue_number}" \
     --title "Fix #${issue_number}: ${issue_title}" \
     --body "Closes #${issue_number}" \
     --label ai-generated
   ```

3. **Comment on issue** (GitHub node, qa-bot credential):
   - Operation: create issue comment
   - Issue number: `{{ $json.issue_number }}`
   - Body: `PR opened: {{ $('SSH qa-agent').item.json.stdout }}`

4. **Error branch**: on any non-zero SSH exit, post a comment to the
   issue with the captured stderr and remove the `agent-task` label.

---

## Workflow 2 — `02-release-merge`

**Trigger**: GitHub webhook, `pull_request` event.

**Filter**: `action == "closed"` AND `pull_request.merged == true`
AND `pull_request.base.ref == "main"`.

**Steps**:

1. **Determine bump** (Function node):
   ```js
   const labels = $json.pull_request.labels.map(l => l.name);
   const bump = labels.includes('release:major') ? 'major'
              : labels.includes('release:minor') ? 'minor'
              : 'patch';
   return [{ json: { bump } }];
   ```

2. **SSH to release-agent** (SSH node, credential `agent-ssh`, host
   `release-agent`):

   ```bash
   set -euo pipefail
   cd ~/workspace
   git fetch origin main
   git checkout main
   git pull --ff-only origin main

   # Compute next version from existing tags.
   LAST=$(git tag --list 'v*' --sort=-v:refname | head -1)
   LAST=${LAST:-v0.0.0}
   IFS=. read -r MA MI PA <<<"${LAST#v}"
   case "${bump}" in
     major) NEW="$((MA+1)).0.0" ;;
     minor) NEW="${MA}.$((MI+1)).0" ;;
     patch) NEW="${MA}.${MI}.$((PA+1))" ;;
   esac

   ./scripts/sync-versions.sh "${NEW}"
   git commit -am "release: v${NEW}"
   git tag "v${NEW}"
   git push origin main --tags
   ```

3. **Post release notice**: nothing. The tag push triggers
   `.github/workflows/release.yml`, which owns publishing from here.

   > **Memory:** don't create GitHub Releases manually — tag push is the
   > entire trigger.

---

## Workflow 3 — `03-pr-build-comment`

**Trigger**: GitHub webhook, `issue_comment` event.

**Filter**: `action == "created"` AND `issue.pull_request` is set
AND body matches `/^\/build (ios|android)\s*$/`.

**Steps**:

1. **Resolve PR head SHA** (GitHub node):
   - Operation: get pull request
   - PR number: `{{ $json.issue.number }}`
   - Extract `head.sha` into a Set node as `sha`.

2. **React to comment** (GitHub node):
   - Operation: create reaction on comment
   - Reaction: `rocket`

3. **SSH to qa-agent** (SSH node):

   ```bash
   set -euo pipefail
   cd ~/workspace
   git fetch origin

   if [ "${target}" = "ios" ]; then
     /usr/local/bin/remote-xcodebuild "${sha}"
   else
     /usr/local/bin/remote-jnilibs "${sha}"
     git checkout "${sha}" -- .
     ./gradlew :app:assembleRelease
     ls -1 clients/android/app/build/outputs/apk/release/*.apk
   fi
   ```

4. **Post result comment** (GitHub node): captured stdout/stderr as
   a PR comment, with `:white_check_mark:` on success or
   `:x:` on failure.

---

## Rebuilding from the JSON exports

Once workflows are configured in the UI, use **Export** on each flow
and drop the files in `workflows/`:

```
workflows/
├── 01-qa-agent-issue.json
├── 02-release-merge.json
└── 03-pr-build-comment.json
```

The compose stack mounts `./n8n/workflows` read-only into the n8n
container at `/workflows` so you can import them by hand via the n8n UI
after a fresh install. n8n does not auto-import on boot — this is
intentional: workflows often need secret references re-bound to the
fresh credential IDs in a new install.
