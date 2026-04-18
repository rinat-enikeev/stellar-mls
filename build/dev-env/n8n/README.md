# n8n workflows for the stellar-mls dev-env

n8n runs in its own container (`stellar-n8n`) on the `orchestration`
docker network alongside `qa-agent` and `release-agent`. It reaches the
agents over SSH at `qa-agent:22` / `release-agent:22` using a dedicated
key pair stored in n8n's encrypted credential store — **never** via the
Docker socket.

Six workflows live under `workflows/`. They're JSON exports of n8n
flows, but the specs below describe what each one does in plain terms so
you (or a future agent) can rebuild them from scratch in the n8n UI.

See `docs/design-n8n-agent-round-trip.md` in the repo root for the
end-to-end design that ties workflows 01 / 04 / 05 / 06 into a single
loop (issue-assigned → implement → review → address comments → merge →
smoke test).

---

## Automated deploy

`./bin/n8n-deploy.sh` is the one-shot install. It imports every
credential under `n8n/secrets/*.json` and every workflow under
`n8n/workflows/*.json` into the running `stellar-n8n` container,
rewriting the `REPLACE_ME` credential-ID placeholders in each workflow
to match whatever IDs the DB ended up with, and activates everything.
Idempotent via name-based upsert — rerun any time a credential rotates
or a workflow changes.

### First deploy

```bash
# 1. Boot the stack if it isn't already.
./bin/up.sh

# 2. Seed the secrets dir from the templates (gitignored).
cp -R build/dev-env/n8n/secrets.example build/dev-env/n8n/secrets
chmod 600 build/dev-env/n8n/secrets/*.json

# 3. Fill in real values — see n8n/secrets.example/README.md for the
#    field guide (where private keys come from, which PAT scopes to
#    grant, SSH pubkey distribution).
${EDITOR:-vim} build/dev-env/n8n/secrets/*.json

# 4. Deploy.
./bin/n8n-deploy.sh
```

The final output lists every workflow's webhook URL (constructed from
`WEBHOOK_URL` in `n8n.env`). Point your GitHub App webhook at those
paths and subscribe to:

> `issues`, `pull_request`, `pull_request_review`,
> `pull_request_review_comment`, `issue_comment`

### Re-running

Safe and idempotent. Use it when:

- rotating a GitHub PAT or regenerating an SSH key (edit the relevant
  JSON under `n8n/secrets/`, re-run);
- editing a workflow JSON (e.g. tightening a filter, tweaking a prompt);
- adding a new credential file or new workflow JSON.

`./bin/reset-agent.sh` preserves the `n8n-data` volume, so the deploy
doesn't need to run after resetting an agent. If you wipe `n8n-data`
itself (`docker volume rm stellar-devenv_n8n-data`) all IDs reset — the
next deploy inserts everything fresh.

### Required secrets

Four files under `n8n/secrets/`, matching the credential names the
workflow JSONs reference:

| File | n8n credential name |
|---|---|
| `ssh-qa-agent.json` | `qa-agent SSH` |
| `ssh-release-agent.json` | `release-agent SSH` |
| `github-qa-bot.json` | `qa-bot GitHub` |
| `github-release-bot.json` | `release-bot GitHub` |

Full schema and provisioning playbook: `n8n/secrets.example/README.md`.

### cloudflared tunnel

Separate from the deploy script. Create a tunnel pointing at
`http://stellar-n8n:5678` (container network) or
`http://127.0.0.1:5678` (Mac host loopback) and set `WEBHOOK_URL` in
`n8n.env` to the resulting public URL. Each workflow exposes its own
path under `/webhook/<path>` — the script prints the full list at the
end of every run. Use a cloudflared routing rule (or a thin reverse
proxy) to fan the GitHub App's single webhook URL out to the per-path
endpoints if needed.

---

## Manual fallback (UI)

Use this only when `n8n-deploy.sh` can't run (jq missing, n8n CLI
unavailable in the image, etc.). It reproduces what the script does:

1. Open http://127.0.0.1:5678 via ProxyJump and create the owner
   account (CLI deploy can run before this; UI access is independent).
2. **SSH credentials**: create `qa-agent SSH` and `release-agent SSH`
   (`sshPrivateKey` type) with host `qa-agent` / `release-agent`, port
   22, user `agent`. Use the same OpenSSH private key for both; paste
   the matching public half into both agents' `.env` files as
   `N8N_SSH_PUBKEY` and restart the agents.
3. **GitHub credentials**: create `qa-bot GitHub` and `release-bot
   GitHub` (`githubApi` type) with a fine-grained PAT or App
   installation token as `accessToken`.
4. **Import workflows**: for each JSON under `workflows/`, click
   Import. After import, open every SSH node (in both branches of each
   `Switch by host` Switch) and every GitHub node, and re-bind the
   credential by name — the imported `id: REPLACE_ME` placeholder
   leaves them unbound. Activate each workflow.

---

## Agent user map (env-driven)

Workflows 01, 04, 05, and 06 route events to the right SSH target and
back-post as the right GitHub identity by consulting env vars set in
`n8n.env`. See `n8n.env.example` for the full set; summary:

| Variable | Purpose |
|---|---|
| `N8N_AGENT_MAP` | JSON map of GitHub login → `{role, host, port, user, sshCredentialName, githubCredentialName}`. Keys must match payload `login` strings. Every host listed here must have a matching branch in each workflow's `Switch by host` node. |
| `N8N_IMPLEMENTER_DEFAULT` | Login used when the payload doesn't pin an implementer (e.g. `/fix` on a human-authored PR). Must be an `implementer` key in the map. |
| `N8N_REVIEWER_DEFAULT` | Login auto-assigned as reviewer when workflow 01 opens a PR. Must be a `reviewer` key in the map. |
| `N8N_HUMAN_QA_LOGIN` | Receives the smoke-test issue created by workflow 06. |
| `N8N_BOT_LOGINS` | CSV of logins whose events must never re-trigger the round-trip. Include every agent-bound handle from `N8N_AGENT_MAP` plus `github-actions[bot]`. |
| `N8N_MERGE_AUTHORIZED_LOGINS` | CSV of logins whose `/merge` comments workflow 06 will honour. |
| `N8N_COMMAND_USERS` | CSV of logins allowed to address agents in PR comments by `@`-mentioning an implementer (see workflow 05). Can include agent handles — loop-proof because bot-posted comments never match the `@<login> <free text>` pattern. |

**Adding a new agent**: add a JSON key to `N8N_AGENT_MAP`; create the
matching n8n SSH + GitHub credentials; add a new branch to the `Switch
by host` node in each of 01/04/05/06; wire its SSH node to the existing
`Merge SSH outputs`. No code changes elsewhere.

Quick sanity check on the running container:

```bash
docker exec stellar-n8n node -e \
  'console.log(JSON.parse(process.env.N8N_AGENT_MAP))'
```

Must print the parsed object without throwing.

---

## Loop prevention

The round-trip is one bad filter from a commenting loop, so every
agent-triggering workflow gates on all four of:

- `body.sender.type !== 'Bot'` — skip GitHub App / bot postings.
- `body.sender.login ∉ N8N_BOT_LOGINS` — skip events whose *cause* is a
  mapped agent handle (guards against the agent triggering itself).
- `body.comment.user.login ∉ N8N_BOT_LOGINS` (comment workflows only)
  — skip when the comment *author* is an agent, even if some human
  action caused the webhook.
- **Slash-command separation**: workflow 05 ignores any comment
  matching `/build|/merge|/review`; workflows 03 and 06 ignore anything
  that doesn't match their specific slash. This means agent narrative
  comments posted by the implementer don't accidentally qualify as
  merge or build commands.

Workflow 05 has one more guard: it *only* runs the auto-fix when one of
these is true (checked in order):

1. The commenter is in `N8N_COMMAND_USERS` AND the comment `@`-mentions
   an implementer login from `N8N_AGENT_MAP` — routes to the mentioned
   agent, using the comment body (minus the mention) as the prompt.
2. The PR author is an implementer in the map (implicit opt-in for
   agent-authored PRs) — routes to the PR author's agent with a generic
   "address review comment" prompt.
3. The comment starts with `/fix` — routes to `N8N_IMPLEMENTER_DEFAULT`.

This keeps the agent from mutating unrelated human PRs based on passing
review comments while still letting authorised humans address specific
agents by name on any PR.

---

## Workflow 1 — `01-qa-agent-issue`

**Trigger**: GitHub webhook, `issues` event.

**Filter** (single boolean expression):
- `action ∈ [opened, reopened, labeled, assigned]`
- sender is not a bot or mapped handle
- AND **either** the issue carries the `agent-task` label **or** one
  of its assignees is an `implementer` in `N8N_AGENT_MAP`.

**Steps**:

1. **Extract issue fields** (Set): `issueNumber`, `issueTitle`,
   `issueBody`, `repoOwner`, `repoName`.
2. **Resolve implementer** (Code): look at the issue's assignees; pick
   the first one that's an `implementer` in `N8N_AGENT_MAP`. Fall back
   to `N8N_IMPLEMENTER_DEFAULT`. Output `host`, `sshPort`, `sshUser`,
   `implementerLogin`, `reviewerLogin` (= `N8N_REVIEWER_DEFAULT`).
3. **Encode prompt** (Code): build the Claude prompt and base64-encode
   it so it survives the SSH command line.
4. **Switch by host**: one branch per distinct `host` value.
5. **SSH to the resolved agent**:
   ```bash
   set +e
   set -o pipefail
   BRANCH="agent/issue-${issueNumber}"
   cd ~/workspace
   export GH_TOKEN="$(git config --get remote.origin.url | sed -n 's|.*x-access-token:\([^@]*\)@.*|\1|p')"
   git fetch origin main
   git checkout -B "$BRANCH" origin/main
   echo "$promptB64" | base64 -d > /tmp/prompt.txt
   claude --print < /tmp/prompt.txt
   git push -u origin "$BRANCH" --force
   gh pr create --fill --base main --head "$BRANCH" \
     --label ai-generated --reviewer "$reviewerLogin"
   ```
6. **Merge SSH outputs** (single Merge node collecting both branches).
7. **Comment on issue** (GitHub, qa-bot credential): posts the PR URL
   captured from stdout.

The `--reviewer` flag auto-requests review from whichever login is
`N8N_REVIEWER_DEFAULT`, which in turn fires **workflow 04**.

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

2. **SSH to release-agent** (SSH node, credential `release-agent SSH`):

   ```bash
   set -euo pipefail
   cd ~/workspace
   git fetch origin main
   git checkout main
   git pull --ff-only origin main

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

1. **Resolve PR head SHA** (HTTP → GitHub API).
2. **SSH to qa-agent** (credential `qa-agent SSH`):
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
   fi
   ```
3. **Post result comment** with download URL or failure tail.

---

## Workflow 4 — `04-pr-review-request`

**Trigger**: GitHub webhook, `pull_request` event.

**Filter**:
- `action == "review_requested"`
- sender not a bot / mapped handle
- `requested_reviewer.login` is a `reviewer` in `N8N_AGENT_MAP`.

**Steps**:

1. **Extract PR fields** (Set): `prNumber`, `prTitle`, `prBody`,
   `prHeadRef`, `prHeadSha`, `prBaseRef`, `reviewerLogin`, `repoOwner`,
   `repoName`.
2. **Resolve reviewer** (Code): look up `reviewerLogin` in the map;
   output `host`, `sshPort`, `sshUser`.
3. **Encode review prompt** (Code).
4. **Switch by host**.
5. **SSH to the reviewer agent**:
   ```bash
   PR={{ prNumber }}
   BASE="{{ prBaseRef }}"
   cd ~/workspace
   export GH_TOKEN=...  # extracted from remote URL as in workflow 01
   git fetch origin "pull/$PR/head:pr-$PR"
   git fetch origin "$BASE"
   git checkout "pr-$PR"
   echo "$promptB64" | base64 -d > /tmp/review.txt
   echo "=== DIFF ===" >> /tmp/review.txt
   git diff "origin/$BASE...HEAD" >> /tmp/review.txt
   REVIEW=$(claude --print < /tmp/review.txt)
   printf '%s\n' "$REVIEW" > /tmp/review-body.md
   gh pr review "$PR" --comment --body-file /tmp/review-body.md
   ```
6. **No explicit ack comment** — `gh pr review` is itself the
   observable outcome on the PR.

Claude prompt gist: "Review PR #N. Output a one-line verdict
(APPROVE / REQUEST_CHANGES / COMMENT) followed by bullet findings with
`file:line` refs. Diff below after `=== DIFF ===`."

---

## Workflow 5 — `05-pr-address-comment`

**Trigger**: GitHub webhook, **two** paths in one workflow:
- `/webhook/pr-address-comment` for `issue_comment` events
- `/webhook/pr-address-review-comment` for `pull_request_review_comment`

Both feed the same downstream chain via a Merge (append).

**Filter** (all AND):
- `action == "created"`
- When via `issue_comment`: the comment is on a PR (`issue.pull_request`).
- Sender not a Bot, sender login not in `N8N_BOT_LOGINS`.
- Comment author login not in `N8N_BOT_LOGINS`.
- Body does **not** match `/^\/(build|merge|review)\b/i` (those belong
  to workflows 03 / 06 / 04).

**Steps**:

1. **Merge webhooks** (append mode).
2. **Is human PR comment?** (IF, filter above).
3. **Normalize** (Code): unify shape across the two event types:
   `{prNumber, commentId, commentBody, path?, line?, diffHunk?, …}`.
4. **Get PR** (HTTP, githubApi). Reveals `head.ref`, `base.ref`, and
   `user.login` (the PR author).
5. **Resolve implementer** (Code): opt-in rule, priority order:
   1. If the commenter is in `N8N_COMMAND_USERS` and the comment
      `@`-mentions an implementer from `N8N_AGENT_MAP` → target that
      agent, and strip the mention from the body to use as the
      free-text instruction.
   2. Else if the PR author is an `implementer` in the map → target
      that agent (legacy auto-fix path for agent-authored PRs).
   3. Else if the comment starts with `/fix` → use
      `N8N_IMPLEMENTER_DEFAULT`.
   4. Otherwise return an empty array and the workflow exits cleanly.

   Output includes `implementerLogin`, `instructionMode` (`mention` /
   `author-implementer` / `fix-slash`) and `instruction` so the next
   node can pick the right prompt template.
6. **Encode fix prompt** (Code): emits a free-text-instruction prompt
   when `instructionMode === 'mention'`, else the generic "address
   review comment" prompt.
7. **Switch by host**.
8. **SSH to the implementer agent**:
   ```bash
   cd ~/workspace
   git fetch origin "$BRANCH"
   git checkout "$BRANCH"
   git reset --hard "origin/$BRANCH"
   echo "$promptB64" | base64 -d > /tmp/fix.txt
   claude --print < /tmp/fix.txt
   if git diff --quiet "origin/$BRANCH..HEAD"; then
     echo "NO_CHANGES=1"
     exit 0
   fi
   git push origin "HEAD:$BRANCH"
   echo "NEW_SHA=$(git rev-parse HEAD)"
   ```
9. **Merge SSH outputs** (single Merge node).
10. **Post result on PR** (GitHub): either `addressed in <short-sha>`,
    `no changes`, or a failure note.

Claude prompt gist: "Address the reviewer comment on PR #N. Make the
smallest change possible. Commit with message `address review comment
<id>`. Do not rebase or push — the workflow pushes."

---

## Workflow 6 — `06-pr-merge-command`

**Trigger**: GitHub webhook, `issue_comment` event.

**Filter**:
- `action == "created"`
- Comment is on a PR (`issue.pull_request` set).
- Sender not a Bot.
- Sender login ∈ `N8N_MERGE_AUTHORIZED_LOGINS`.
- Body matches `/^\/merge\b/i`.

**Steps**:

1. **Parse merge command** (Code): extract optional method
   (`squash|rebase|merge`, default `squash`) from `/merge <method>`.
2. **Get PR** (HTTP, githubApi): for `title`, `html_url`, `head.ref`,
   `user.login`.
3. **Resolve implementer** (Code): PR author if agent, else
   `N8N_IMPLEMENTER_DEFAULT`. Also enrich the output with
   `qaLogin = N8N_HUMAN_QA_LOGIN`.
4. **Switch by host**.
5. **SSH to the implementer agent**:
   ```bash
   PR=<n>
   METHOD=<squash|rebase|merge>
   REPO=<owner/repo>
   gh pr merge "$PR" --"$METHOD" --delete-branch --repo "$REPO"
   # on success, create smoke-test issue:
   gh issue create --repo "$REPO" \
     --title "Smoke test: $PR_TITLE" \
     --assignee "$QA_LOGIN" \
     --label smoke-test \
     --body-file /tmp/smoke-body.md
   ```
   The SSH script prints `MERGE_RC=`, `SMOKE_RC=`, and
   `SMOKE_URL=…/issues/<n>` for the downstream node to parse.
6. **Merge SSH outputs**.
7. **Post merge result** (GitHub): comment on the just-merged PR with
   the smoke-test issue URL (or a failure message).

The actual `gh pr merge` side-effect fires `pull_request.closed` +
`merged == true`, which then flows into **workflow 02** to cut a tag.

---

## Syncing changes back to JSON

Workflows edited in the n8n UI need to be exported back to
`workflows/*.json` so the next `./bin/n8n-deploy.sh` keeps the change
committed. Two options:

```bash
# Single workflow by id (get the id from the UI URL):
docker exec stellar-n8n n8n export:workflow --id=<id> \
    --output=/tmp/wf.json
docker cp stellar-n8n:/tmp/wf.json \
    build/dev-env/n8n/workflows/<file>.json

# All workflows at once (review the diff before committing):
docker exec stellar-n8n n8n export:workflow --all \
    --output=/tmp/all.json
docker cp stellar-n8n:/tmp/all.json /tmp/all.json
# then split by .name into the matching files under workflows/
```

Before committing, replace every concrete credential `id` in the
exported JSON with `REPLACE_ME` (keep the `name`) so the file stays
portable across fresh n8n installs. The deploy script's credential-
rewrite pass re-injects the right ID on each import.

The compose stack also mounts `./n8n/workflows` read-only into the n8n
container at `/workflows` — convenient for hand-importing in the UI
during debugging, but not used by the deploy script (which writes its
own scratch copy to `/tmp/n8n-deploy/` to avoid touching the
bind-mounted source).
