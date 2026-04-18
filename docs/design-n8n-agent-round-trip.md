# Design: n8n workflows for assign-driven agent round-trip

Status: design / not yet implemented.
Scope: `build/dev-env/n8n/**`, `build/dev-env/n8n.env.example`, `build/dev-env/README.md`.

## Context

The dev-env stack at `build/dev-env/` already runs n8n + two Claude Code agent containers (`qa-agent`, `release-agent`) and ships three inactive workflow templates (`01-qa-agent-issue`, `02-release-merge`, `03-pr-build-comment`). Today the implementer is triggered by a label (`agent-task`); reviewer assignment, comment-driven fixes, and human-initiated merges are not wired.

The goal is a full round-trip:

1. Human creates an issue, **assigns** an agent handle → workflow drives that agent to implement, open a PR, and auto-request review from the *other* configured agent.
2. PR **review-requested** → reviewer agent posts a Claude-produced review.
3. Human leaves a PR comment or inline review comment → implementer agent addresses it and pushes a fix.
4. Existing `/build` comment keeps working (workflow 03).
5. Human comments `/merge` → implementer agent merges the PR and opens a smoke-test issue assigned to the human QA user.

GitHub handles are configurable via `.env`:

- **Implementer agent (default)**: `@programyzer`
- **Reviewer agent (default)**: `@releaseng`
- **Human QA** (receives smoke-test issues): `@alexpovstin`

## Design decisions

### User-map schema — one JSON env var

One `N8N_AGENT_MAP` JSON var in `n8n.env` beats scattered `N8N_USER_MAP_*` vars: single source of truth, trivial to read from a Code node via `JSON.parse($env.N8N_AGENT_MAP)`, easy to diff.

```
N8N_AGENT_MAP={"programyzer":{"role":"implementer","host":"release-agent","port":22,"user":"agent","sshCredentialName":"release-agent SSH","githubCredentialName":"programyzer GitHub"},"releaseng":{"role":"reviewer","host":"qa-agent","port":22,"user":"agent","sshCredentialName":"qa-agent SSH","githubCredentialName":"releaseng GitHub"}}
N8N_HUMAN_QA_LOGIN=alexpovstin
N8N_IMPLEMENTER_DEFAULT=programyzer
N8N_REVIEWER_DEFAULT=releaseng
N8N_BOT_LOGINS=programyzer,releaseng,github-actions[bot]
N8N_MERGE_AUTHORIZED_LOGINS=alexpovstin,programyzer
```

Note: `@programyzer` is a human GitHub account that the implementer agent posts *on behalf of* via its GitHub App credential. It's listed in `N8N_BOT_LOGINS` to prevent the agent reacting to its own comments/commits. Since manual human actions from this handle use the same GitHub user, manual comments from `@programyzer` will NOT trigger auto-fix; human work from this account must use the explicit `/fix` slash-command opt-in path (see Loop prevention).

### n8n ↔ SSH binding — Switch by host

n8n SSH credentials are resolved by credential *name* at design time, not by runtime expression. So dynamic selection uses the existing pattern from the three current workflows: **Webhook → filter IF → Resolve (Code) → Switch on `host` → [SSH qa-agent | SSH release-agent] → Merge → Post back**. The JSON map carries `host`/`port`/`user`/`role`; the `sshCredentialName`/`githubCredentialName` fields document which n8n credential each Switch branch uses.

### GitHub webhook events

Existing subscriptions (`issues`, `pull_request`, `issue_comment`) plus **add `pull_request_review_comment`** (inline diff comments). `pull_request.assigned` and `pull_request.review_requested` are sub-actions of the already-subscribed `pull_request` event — no new subscription needed for those.

### Smoke-test issue lives inside the /merge workflow

Same trigger and auth context as the merge; splitting adds a second SSH hop for zero reuse. Existing `02-release-merge.json` still fires on `pull_request.closed && merged` for tagging — cleanly separated: 06 performs merge + creates the smoke issue, 02 reacts to the resulting merge event and tags.

### Loop prevention

Every new workflow filter has:

- `body.sender.type !== 'Bot'`
- `!N8N_BOT_LOGINS.split(',').includes(body.sender.login)`
- For comment workflows: `!N8N_BOT_LOGINS.split(',').includes(body.comment.user.login)`
- Slash-command workflows require a regex match on the comment body.
- Workflow 05 fires the auto-fix only when the PR author is an agent in the map (opt-in); human PRs accept only explicit `/fix …` comments.

### Update to workflow 01 — additive, not replace

Change trigger from label-only to **label OR assignee-in-map** so existing behaviour keeps working. Add a Resolve-implementer Code node, Switch by host, and append `--reviewer $REVIEWER_LOGIN` to `gh pr create`.

## Files

All paths relative to the repo root.

### Create

| Path | Purpose |
|---|---|
| `build/dev-env/n8n/workflows/04-pr-review-request.json` | `pull_request.review_requested` → SSH reviewer → Claude posts review via `gh pr review --comment` |
| `build/dev-env/n8n/workflows/05-pr-address-comment.json` | `issue_comment.created` on PR **or** `pull_request_review_comment.created` from human → SSH implementer → Claude commits → push |
| `build/dev-env/n8n/workflows/06-pr-merge-command.json` | `issue_comment` matching `^/merge\b` from authorized human → SSH implementer → `gh pr merge` + `gh issue create` (smoke test, assigned to `N8N_HUMAN_QA_LOGIN`) |

### Modify

| Path | Change |
|---|---|
| `build/dev-env/n8n/workflows/01-qa-agent-issue.json` | Add `assigned` action; filter = label OR assignee-in-map; add Resolve Code node; Switch by host; append `--reviewer` to `gh pr create` |
| `build/dev-env/n8n.env.example` | Append `N8N_AGENT_MAP`, `N8N_HUMAN_QA_LOGIN`, `N8N_IMPLEMENTER_DEFAULT`, `N8N_REVIEWER_DEFAULT`, `N8N_BOT_LOGINS`, `N8N_MERGE_AUTHORIZED_LOGINS` with documented defaults |
| `build/dev-env/n8n/README.md` | New sections: **Agent user map**, **Loop prevention**, workflow specs 04/05/06; update workflow 01 description; update GitHub App event-subscriptions list to include `pull_request_review_comment` |
| `build/dev-env/README.md` | Bump workflow-list line from 3 to 6 |

**No changes** to `Dockerfile.agent`, `docker-compose.dev.yml`, `entrypoint/first-run.sh`, `qa.env.example`, `release.env.example`, or `host/host-build-dispatch.sh`. All new state is in n8n env + n8n-stored credentials.

## Node chains per workflow

### 04 — `04-pr-review-request.json`

Webhook → IF (`action==review_requested`, sender not bot, reviewer in map) → Set (extract PR fields + `reviewerLogin`) → Code (Resolve: look up `reviewerLogin` in map → `host/port/user`) → Code (Encode review prompt to base64) → Switch on `host` → SSH (fetch PR branch, run `claude --print` with prompt + `git diff origin/<base>...HEAD`, `gh pr review <N> --comment --body-file -`) → Merge → HTTP (optional PR ack comment).

Claude prompt template: "Review PR #{N}: {title}. PR description: {body}. Produce concise markdown. Start with one-line verdict (APPROVE / REQUEST_CHANGES / COMMENT). Bullet findings with file:line refs." Appended after the prompt: the raw diff block.

### 05 — `05-pr-address-comment.json`

Two Webhook nodes (one per event type) → Merge append → IF (`action==created`; `issue.pull_request` present when via `issue_comment`; sender + commenter not bots; not a `/build|/merge|/review` slash) → Code (Normalize to unified shape with `prNumber`, `commentBody`, optional `path`/`line`/`diffHunk`) → HTTP GET `/repos/:o/:r/pulls/:n` (for `head.ref`, `user.login`) → Code (Resolve: if PR author is an implementer in map, target that agent; else require `/fix` slash to proceed) → Code (Encode prompt) → Switch on `host` → SSH (checkout branch, `claude --print`, if diff non-empty push to `HEAD:<branch>`) → Merge → HTTP (reply to review comment via `/pulls/:n/comments/:id/replies`, or comment on PR conversation).

Claude prompt template: "You are addressing review feedback on PR #{N}, branch {branch}. Reviewer comment{ at path:line}: --- {body} --- {diff hunk if present}. Make the smallest change that addresses the comment. Commit with message 'address review comment {id}'. Do not rebase. Do not push — the workflow pushes."

### 06 — `06-pr-merge-command.json`

Webhook → IF (`action==created`; PR comment; `^/merge\b`; sender is in `N8N_MERGE_AUTHORIZED_LOGINS`) → Code (parse optional `squash|rebase|merge` method) → HTTP GET PR (head.ref, title, url, user.login) → Code (Resolve implementer: PR author if agent, else `N8N_IMPLEMENTER_DEFAULT`) → Switch on `host` → SSH (`gh pr merge <N> --<method> --delete-branch`, then `gh issue create --title "Smoke test: $PR_TITLE" --assignee $N8N_HUMAN_QA_LOGIN --label smoke-test --body "…$PR_URL…"`, capture created issue URL) → Merge → HTTP (comment merged PR with smoke-test link).

### 01 — modified filter + pipeline

Webhook → IF (action in `[opened,reopened,labeled,assigned]`, AND (label `agent-task` OR any assignee has implementer role in map), AND sender not bot) → existing Extract → **new** Code (Resolve implementer: first matching assignee, else `N8N_IMPLEMENTER_DEFAULT`; set `reviewerLogin = N8N_REVIEWER_DEFAULT`) → existing Encode prompt → **new** Switch on `host` → duplicated SSH nodes (one per credential) with `gh pr create --fill --base main --head $BRANCH --label ai-generated --reviewer "{{ $json.reviewerLogin }}"` → existing comment-on-issue.

## Critical files to read before editing

- `build/dev-env/n8n/workflows/01-qa-agent-issue.json` (pattern for Webhook + Code + SSH + comment-back)
- `build/dev-env/n8n/workflows/03-pr-build-comment.json` (pattern for slash-command IF filter, HTTP Get-PR, reply-on-PR)
- `build/dev-env/n8n.env.example` (env format + comments to match)
- `build/dev-env/n8n/README.md` (tone + section structure to match)
- `build/dev-env/Dockerfile.agent` + `build/dev-env/entrypoint/first-run.sh` (confirms `claude` CLI, `gh` CLI, and `~/workspace` layout available in SSH shell)

## Verification

Manual end-to-end after import; workflow import order does not matter (they are independent).

1. **Env sanity**: `docker exec stellar-n8n node -e 'console.log(JSON.parse(process.env.N8N_AGENT_MAP))'` — must print the parsed object without throwing.
2. **Credentials**: in the n8n UI, confirm two SSH creds (`release-agent SSH`, `qa-agent SSH`) and two GitHub App creds (`programyzer GitHub`, `releaseng GitHub`) with the exact names used in the Switch branches.
3. **App events**: each GitHub App subscribed to `issues`, `pull_request`, `issue_comment`, `pull_request_review`, `pull_request_review_comment`.
4. **Workflow 01 (modified)**: create an issue, assign `@programyzer` (no label) → a PR opens from the release-agent container with `@releaseng` as the reviewer. Also verify the label-only path still works (backward compat).
5. **Workflow 04**: on the auto-requested review from step 4, expect a review comment posted by `@releaseng`'s GitHub App within ~60s, starting with a verdict line.
6. **Workflow 05**: as `@alexpovstin`, leave an inline review comment ("please add a unit test here") → expect a new commit on the PR branch plus a reply linking the SHA. Leave a PR conversation comment starting with `/fix …` → same outcome via the `issue_comment` path. Leave a conversation comment as `@releaseng` or `@programyzer` → workflow must NOT fire (commenter-in-bots guard).
7. **Workflow 03 (unchanged sanity)**: `/build android` from `@alexpovstin` → APK link posted.
8. **Workflow 06**: as `@alexpovstin`, comment `/merge` → PR squash-merged, branch deleted, a new issue "Smoke test: …" assigned to `@alexpovstin`, PR comment links the smoke issue. As a non-authorized user, `/merge` → no action.
9. **Workflow 02 (unchanged sanity)**: the merge in step 8 fires workflow 02 → release tag cut.
10. **Loop sanity**: have `@releaseng` or `@programyzer` post a conversation comment manually → workflow 05 must NOT fire. Have workflow 04's own review comment arrive → no workflow 05 fire.

Optional automated check: add an `N8N_AGENT_MAP` JSON-parse assertion to `build/dev-env/bin/doctor.sh` (if present) or include the `docker exec … JSON.parse` one-liner in the README troubleshooting section.
