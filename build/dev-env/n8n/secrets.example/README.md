# n8n credentials — operator secrets

`./bin/n8n-deploy.sh` imports every JSON file in `../secrets/` (sibling
of this dir, gitignored) as an n8n credential. This folder holds the
templates — copy and fill in real values:

```bash
cp -R build/dev-env/n8n/secrets.example build/dev-env/n8n/secrets
chmod 600 build/dev-env/n8n/secrets/*.json
# then edit each file with real keys / tokens
```

## Required credentials

The four files below map 1:1 to the credential references in the shipped
workflow JSONs. All four are required; missing ones cause the deploy
script to fail early.

| File | n8n credential name | Used by |
|---|---|---|
| `ssh-qa-agent.json` | `qa-agent SSH` | 01, 03, 04, 05, 06 |
| `ssh-release-agent.json` | `release-agent SSH` | 01, 02, 04, 05, 06 |
| `github-qa-bot.json` | `qa-bot GitHub` | 01, 03, 05, 06 |
| `github-release-bot.json` | `release-bot GitHub` | 02 |

Drop additional JSON files into `secrets/` to import extra credentials
(they just won't be referenced by the shipped workflows until a workflow
is updated to point at them).

## Field guide

### SSH credentials (`sshPassword` type, works with key auth too)

- **host** — container hostname on the `orchestration` Docker network.
  `qa-agent` and `release-agent` exactly as in `docker-compose.dev.yml`.
- **port** — always `22` (sshd inside the container).
- **username** — always `agent`.
- **privateKey** — OpenSSH-format private key whose public half lives in
  `qa.env` / `release.env` as `N8N_SSH_PUBKEY`. Generate once:

  ```bash
  ssh-keygen -t ed25519 \
      -f build/dev-env/n8n/secrets/n8n-agent-key -N ""

  # Paste the pubkey into both env files:
  pub=$(cat build/dev-env/n8n/secrets/n8n-agent-key.pub)
  for env in qa.env release.env; do
      # edit build/dev-env/$env and set:
      #   N8N_SSH_PUBKEY="$pub"
      :
  done

  # Embed the private key into the credential JSONs:
  priv=$(cat build/dev-env/n8n/secrets/n8n-agent-key \
           | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read()))')
  # → paste into the "privateKey" field of ssh-qa-agent.json +
  #   ssh-release-agent.json (same key, both agents trust it).
  ```

  The public half goes into `authorized_keys` on first-run of each
  agent (see `entrypoint/first-run.sh`). After editing the env files,
  restart the agents so the keys land in place.
- **passphrase** — empty unless you passed `-N` to `ssh-keygen`.

### GitHub credentials (`githubApi` type — bearer token)

- **server** — `https://api.github.com` for public GitHub.
- **user** — cosmetic label shown in the n8n UI. Not sent to GitHub.
- **accessToken** — bearer token the `n8n-nodes-base.github` node sends
  as `Authorization: Bearer <token>`. Two options:

  1. **Fine-grained PAT** (simplest): create at
     https://github.com/settings/personal-access-tokens/new, scope to
     `rinat-enikeev/stellar-mls`, grant the permissions below, paste the
     `github_pat_...` string.
  2. **GitHub App installation token** (rotates hourly, more involved):
     generate a short-lived token from the App's private key via the
     `/app/installations/{installation_id}/access_tokens` endpoint and
     paste. Re-run `n8n-deploy.sh` when the token is rotated.

  If your n8n build ships the newer `githubAppApi` credential type you
  can substitute that and paste the App's PEM directly — but the shipped
  workflows reference `githubApi`, so rename the `type` field in the
  JSON template before importing and wire the workflows to match.

#### Permissions per credential

| Credential | GitHub permissions |
|---|---|
| `qa-bot GitHub` | Issues (RW), Pull requests (RW), Contents (R), Metadata (R) |
| `release-bot GitHub` | Pull requests (RW), Contents (R), Metadata (R) |

(Both can share one token while you're prototyping; split later as
blast-radius discipline.)

## Rotation

Rotating a secret is just *edit the file → re-run deploy*:

```bash
vim build/dev-env/n8n/secrets/github-qa-bot.json
./bin/n8n-deploy.sh
```

The script upserts by credential name, so IDs stay stable and no
workflow needs re-binding.

## What to never commit

`build/dev-env/.gitignore` excludes `n8n/secrets/` for you. Double-check
before any `git add -A`:

```bash
git check-ignore build/dev-env/n8n/secrets/github-qa-bot.json
# → must print the path; silent exit means NOT ignored
```
