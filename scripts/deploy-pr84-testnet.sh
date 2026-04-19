#!/bin/sh
#
# deploy-pr84-testnet.sh — One-shot "bring testnet to the state QA needs for
# PR #84 (governance e2e on-chain creation)" — covers phases 1 and 2 of
# https://github.com/rinat-enikeev/stellar-mls/issues/85:
#
#   Phase 1 (contract):
#     - Deploy a fresh sep-xxxx contract via scripts/deploy_sep_xxxx_testnet.sh
#       using PERSIST_IDENTITY=1 / IDENTITY=ios-testnet-deployer so the
#       contract's stored admin matches DEPLOYER in relayer/.env.
#     - Patch relayer/.env with the new RELAYER_CONTRACT_ID.
#     - Install the two DEV Democracy VKs (UpdateByType(2) for tiers 0/1)
#       via scripts/install-democracy-vks-testnet.sh.
#     - Install the AdminUpdate VK via scripts/install-adminupdate-vk-testnet.sh.
#
#   Phase 2 (relayer redeploy, focused):
#     - Sync /opt/onym-chat/ on the droplet to current origin/main.
#     - Upload the patched relayer/.env.
#     - `docker compose up -d --build --no-deps relayer` so only the relayer
#       container is rebuilt / restarted.
#
#   Phase 3 (smoke):
#     - get_state_v2 returns a V2-shaped record for the deploy group.
#     - get_admin_root on the same (non-Oligarchy) group errors as expected.
#     - create_group_v2 with group_type=1, member_count=1 is rejected before
#       any proof check (PublicInputsMismatch) — proves the V2 ABI is live.
#     - create_group_v2 with group_type=99 is rejected with UnknownGroupType.
#     - create_oligarchy_group with member_count > tier capacity is rejected
#       with MemberCountOutOfRange — proves the oligarchy entrypoint is live.
#     - Relayer HTTPS endpoint responds.
#
# Usage:
#   ./scripts/deploy-pr84-testnet.sh
#   DRY_RUN=1 ./scripts/deploy-pr84-testnet.sh
#   SKIP_CONTRACT=1 ./scripts/deploy-pr84-testnet.sh    # reuse current contract
#   SKIP_RELAYER=1 ./scripts/deploy-pr84-testnet.sh     # don't touch droplet
#   SKIP_SMOKE=1 ./scripts/deploy-pr84-testnet.sh
#
# Required env (auto-sourced from $REPO_ROOT/.env if present):
#   DROPLET_IP, SSH_KEY_PATH, DOMAIN
#
# V-D1: testnet-only — mainnet is refused, same as the VK install scripts.
#
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)"

NETWORK="${NETWORK:-testnet}"
IDENTITY="${IDENTITY:-ios-testnet-deployer}"
ALIAS="${ALIAS:-sep-xxxx-testnet}"
RELAYER_ENV="${RELAYER_ENV:-$REPO_ROOT/relayer/.env}"
REPO_ENV="${REPO_ENV:-$REPO_ROOT/.env}"
DRY_RUN="${DRY_RUN:-0}"
SKIP_CONTRACT="${SKIP_CONTRACT:-0}"
SKIP_RELAYER="${SKIP_RELAYER:-0}"
SKIP_SMOKE="${SKIP_SMOKE:-0}"

# KEEP_ARTIFACTS=1 preserves the deploy script's WORK_DIR so the smoke phase
# can reuse the emitted fixtures (group-id, commitments) without re-running
# the fixture generator.
KEEP_ARTIFACTS=1
export KEEP_ARTIFACTS

# WORK_DIR is shared between the delegated deploy script and our smoke tests.
# Placed under $TMPDIR so it survives the deploy script's own cleanup logic
# (which only deletes when KEEP_ARTIFACTS=0).
WORK_DIR="${WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/pr84-deploy.XXXXXX")}"
export WORK_DIR
FIXTURE_DIR="$WORK_DIR/fixtures"
DEPLOY_LOG="$WORK_DIR/deploy.log"

case "$NETWORK" in
    mainnet|public) echo "error: PR #84 staging script is testnet-only — refusing $NETWORK" >&2; exit 1 ;;
esac

die() {
    echo "error: $1" >&2
    exit 1
}

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

require_cmd stellar
require_cmd cargo
require_cmd ssh
require_cmd scp
require_cmd curl

# ─── Load droplet config from repo .env ──────────────────────────────────

if [ -f "$REPO_ENV" ]; then
    set -a
    # shellcheck disable=SC1090
    . "$REPO_ENV"
    set +a
fi

if [ "$SKIP_RELAYER" != "1" ]; then
    [ -n "${DROPLET_IP:-}" ]   || die "DROPLET_IP not set (add to $REPO_ENV or pass as env)"
    [ -n "${SSH_KEY_PATH:-}" ] || die "SSH_KEY_PATH not set"
    [ -f "$SSH_KEY_PATH" ]     || die "SSH key not found at $SSH_KEY_PATH"
    [ -n "${DOMAIN:-}" ]       || die "DOMAIN not set"
fi

[ -f "$RELAYER_ENV" ] || die "relayer env not found at $RELAYER_ENV"

banner() {
    printf '\n=========================================================\n'
    printf '  %s\n' "$1"
    printf '=========================================================\n\n'
}

# ─── Phase 1a: contract deploy ──────────────────────────────────────────

if [ "$SKIP_CONTRACT" != "1" ]; then
    banner "Phase 1a: deploying fresh sep-xxxx contract to $NETWORK"

    if [ "$DRY_RUN" = "1" ]; then
        echo "(dry-run) PERSIST_IDENTITY=1 IDENTITY=$IDENTITY NETWORK=$NETWORK ALIAS=$ALIAS \\"
        echo "          KEEP_ARTIFACTS=1 WORK_DIR=$WORK_DIR \\"
        echo "          $SCRIPT_DIR/deploy_sep_xxxx_testnet.sh"
        # Fabricate a placeholder so later phases can still be dry-run.
        NEW_CONTRACT_ID="CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"
    else
        PERSIST_IDENTITY=1 \
        IDENTITY="$IDENTITY" \
        NETWORK="$NETWORK" \
        ALIAS="$ALIAS" \
        WORK_DIR="$WORK_DIR" \
        KEEP_ARTIFACTS=1 \
            "$SCRIPT_DIR/deploy_sep_xxxx_testnet.sh" 2>&1 | tee "$DEPLOY_LOG"

        NEW_CONTRACT_ID="$(grep -E '^Contract ID: ' "$DEPLOY_LOG" | tail -n1 | awk '{print $3}')"
        [ -n "$NEW_CONTRACT_ID" ] || die "failed to parse Contract ID from $DEPLOY_LOG"
        case "$NEW_CONTRACT_ID" in
            C*) ;;
            *) die "parsed contract id does not start with C: $NEW_CONTRACT_ID" ;;
        esac
        echo
        echo "    -> new contract id: $NEW_CONTRACT_ID"
    fi
else
    banner "Phase 1a: SKIP_CONTRACT=1 — reusing RELAYER_CONTRACT_ID from $RELAYER_ENV"
    NEW_CONTRACT_ID="$(grep -E '^RELAYER_CONTRACT_ID=' "$RELAYER_ENV" | cut -d= -f2-)"
    [ -n "$NEW_CONTRACT_ID" ] || die "no RELAYER_CONTRACT_ID in $RELAYER_ENV"
    echo "    -> reusing contract id: $NEW_CONTRACT_ID"
fi

# ─── Phase 1b: patch relayer/.env ───────────────────────────────────────

if [ "$SKIP_CONTRACT" != "1" ]; then
    banner "Phase 1b: updating $RELAYER_ENV with new contract id"

    if [ "$DRY_RUN" = "1" ]; then
        echo "(dry-run) sed -i '' 's|^RELAYER_CONTRACT_ID=.*|RELAYER_CONTRACT_ID=$NEW_CONTRACT_ID|' $RELAYER_ENV"
    else
        # BSD sed (macOS) needs an empty extension after -i; use a portable
        # in-place edit via a tempfile.
        tmp_env="$(mktemp "${TMPDIR:-/tmp}/relayer-env.XXXXXX")"
        awk -v id="$NEW_CONTRACT_ID" '
            /^RELAYER_CONTRACT_ID=/ { print "RELAYER_CONTRACT_ID=" id; next }
            { print }
        ' "$RELAYER_ENV" > "$tmp_env"

        # If RELAYER_CONTRACT_ID wasn't present, append it.
        if ! grep -qE '^RELAYER_CONTRACT_ID=' "$tmp_env"; then
            printf 'RELAYER_CONTRACT_ID=%s\n' "$NEW_CONTRACT_ID" >> "$tmp_env"
        fi

        mv "$tmp_env" "$RELAYER_ENV"
        echo "    -> $RELAYER_ENV now points to $NEW_CONTRACT_ID"
    fi
fi

# ─── Phase 1c: install Democracy + AdminUpdate VKs ──────────────────────

banner "Phase 1c: installing Democracy VKs (UpdateByType(2) tiers 0,1)"
if [ "$DRY_RUN" = "1" ]; then
    echo "(dry-run) DRY_RUN=1 $SCRIPT_DIR/install-democracy-vks-testnet.sh"
    DRY_RUN=1 NETWORK="$NETWORK" "$SCRIPT_DIR/install-democracy-vks-testnet.sh"
else
    NETWORK="$NETWORK" "$SCRIPT_DIR/install-democracy-vks-testnet.sh"
fi

banner "Phase 1c: installing AdminUpdate VK"
if [ "$DRY_RUN" = "1" ]; then
    echo "(dry-run) DRY_RUN=1 $SCRIPT_DIR/install-adminupdate-vk-testnet.sh"
    DRY_RUN=1 NETWORK="$NETWORK" "$SCRIPT_DIR/install-adminupdate-vk-testnet.sh"
else
    NETWORK="$NETWORK" "$SCRIPT_DIR/install-adminupdate-vk-testnet.sh"
fi

# ─── Phase 2: focused relayer redeploy ──────────────────────────────────

if [ "$SKIP_RELAYER" != "1" ]; then
    banner "Phase 2: redeploying relayer container on $DROPLET_IP"

    SSH_CMD="ssh -i $SSH_KEY_PATH -o StrictHostKeyChecking=accept-new root@$DROPLET_IP"
    SCP_CMD="scp -i $SSH_KEY_PATH -o StrictHostKeyChecking=accept-new"

    if [ "$DRY_RUN" = "1" ]; then
        echo "(dry-run) $SSH_CMD 'cd /opt/onym-chat && git fetch origin && git reset --hard origin/main'"
        echo "(dry-run) $SCP_CMD $RELAYER_ENV root@$DROPLET_IP:/opt/onym-chat/relayer/.env"
        echo "(dry-run) $SSH_CMD \"sed -i 's/^RELAYER_BIND=.*/RELAYER_BIND=0.0.0.0:8080/' /opt/onym-chat/relayer/.env\""
        echo "(dry-run) $SSH_CMD 'cd /opt/onym-chat && docker compose up -d --build --no-deps relayer'"
    else
        echo "==> Syncing /opt/onym-chat/ to origin/main"
        # shellcheck disable=SC2086
        $SSH_CMD 'cd /opt/onym-chat && git fetch origin && git reset --hard origin/main'

        echo "==> Uploading patched relayer/.env"
        # shellcheck disable=SC2086
        $SCP_CMD "$RELAYER_ENV" "root@$DROPLET_IP:/opt/onym-chat/relayer/.env"

        # The droplet binds 0.0.0.0 behind the reverse proxy (see deploy.sh:309).
        # shellcheck disable=SC2086
        $SSH_CMD "sed -i 's/^RELAYER_BIND=.*/RELAYER_BIND=0.0.0.0:8080/' /opt/onym-chat/relayer/.env"

        echo "==> Rebuilding + restarting relayer container only"
        # shellcheck disable=SC2086
        $SSH_CMD 'cd /opt/onym-chat && docker compose up -d --build --no-deps relayer'

        echo "==> Waiting for relayer to come up"
        sleep 5
        status=$(curl -sS -o /dev/null -w '%{http_code}' --max-time 15 -X POST "https://relay.$DOMAIN/" -H 'Content-Type: application/json' -d '{}' || echo "000")
        # A 4xx response from the relayer is fine — it just means the empty
        # body was rejected. What we care about is reachability + TLS.
        case "$status" in
            2*|4*) echo "    ok — relayer responded HTTP $status" ;;
            *) die "relayer at https://relay.$DOMAIN did not respond (HTTP $status)" ;;
        esac
    fi
fi

# ─── Phase 3: smoke tests against new contract ──────────────────────────

if [ "$SKIP_SMOKE" = "1" ]; then
    banner "Phase 3: SKIP_SMOKE=1 — skipping smoke tests"
    echo
    echo "Done. Contract: $NEW_CONTRACT_ID"
    exit 0
fi

banner "Phase 3: smoke tests against $NEW_CONTRACT_ID"

# The smoke tests need the deploy-script group id (the one with a V2 record
# already written by create_group's legacy-entry synthesis) and a dummy
# proof for negative-path tests that fail before proof verification.
if [ "$DRY_RUN" = "1" ] || [ "$SKIP_CONTRACT" = "1" ]; then
    echo "(dry-run / reused-contract) Phase 3 requires fresh fixtures from Phase 1a."
    echo "                            Re-run without SKIP_CONTRACT / DRY_RUN to execute smoke."
    exit 0
fi

[ -d "$FIXTURE_DIR" ] || die "fixtures dir missing at $FIXTURE_DIR"
DEPLOY_GROUP_ID="$(tr -d '\n' < "$FIXTURE_DIR/group-id.hex")"
DEPLOY_COMMITMENT="$(tr -d '\n' < "$FIXTURE_DIR/commitment-epoch-0.hex")"
TIER="$(tr -d '\n' < "$FIXTURE_DIR/tier.txt")"

invoke() {
    # Internal helper: run a contract invoke read-only (--send no) and
    # print stdout. Caller decides how to match against expected output.
    stellar contract invoke \
        --id "$NEW_CONTRACT_ID" \
        --source-account "$IDENTITY" \
        --network "$NETWORK" \
        --send no \
        -- "$@" 2>&1
}

expect_contains() {
    needle="$1"
    label="$2"
    shift 2
    out="$("$@" || true)"
    if printf '%s' "$out" | grep -qF "$needle"; then
        echo "    ok — $label (matched '$needle')"
    else
        printf '%s\n' "$out" >&2
        die "$label: output did not contain '$needle'"
    fi
}

echo "==> [1/5] get_state_v2 surfaces V2 record for deploy group"
expect_contains '"group_type"' "get_state_v2 shape" \
    invoke get_state_v2 --group-id "$DEPLOY_GROUP_ID"

echo "==> [2/5] get_admin_root rejects non-Oligarchy group (#22 MissingAdminRoot)"
expect_contains '#22' "get_admin_root error code" \
    invoke get_admin_root --group-id "$DEPLOY_GROUP_ID"

# A sentinel group id that does not exist in storage. The V2 entrypoints
# reject the call before touching storage for our negative tests below,
# so a fresh id is fine even without a valid proof.
SENTINEL_GROUP_ID="$(printf 'pr84-smoke-sentinel' | shasum -a 256 | awk '{print $1}')"

echo "==> [3/5] create_group_v2 group_type=1 member_count=1 is rejected (#10 PublicInputsMismatch)"
expect_contains '#10' "1v1 member_count=1 rejected" \
    invoke create_group_v2 \
        --caller "$(stellar keys public-key "$IDENTITY" | tr -d '\n')" \
        --group-id "$SENTINEL_GROUP_ID" \
        --commitment "$DEPLOY_COMMITMENT" \
        --tier "$TIER" \
        --group-type 1 \
        --member-count 1 \
        --proof-file-path "$FIXTURE_DIR/proof-epoch-0-create.json" \
        --public-inputs-file-path "$FIXTURE_DIR/public-inputs-epoch-0.json"

echo "==> [4/5] create_group_v2 group_type=99 rejected (#18 UnknownGroupType)"
expect_contains '#18' "UnknownGroupType" \
    invoke create_group_v2 \
        --caller "$(stellar keys public-key "$IDENTITY" | tr -d '\n')" \
        --group-id "$SENTINEL_GROUP_ID" \
        --commitment "$DEPLOY_COMMITMENT" \
        --tier "$TIER" \
        --group-type 99 \
        --member-count 1 \
        --proof-file-path "$FIXTURE_DIR/proof-epoch-0-create.json" \
        --public-inputs-file-path "$FIXTURE_DIR/public-inputs-epoch-0.json"

echo "==> [5/5] create_oligarchy_group member_count=9999 rejected (#25 MemberCountOutOfRange)"
expect_contains '#25' "MemberCountOutOfRange" \
    invoke create_oligarchy_group \
        --caller "$(stellar keys public-key "$IDENTITY" | tr -d '\n')" \
        --group-id "$SENTINEL_GROUP_ID" \
        --commitment "$DEPLOY_COMMITMENT" \
        --tier "$TIER" \
        --member-count 9999 \
        --admin-root "$DEPLOY_COMMITMENT" \
        --proof-file-path "$FIXTURE_DIR/proof-epoch-0-create.json" \
        --public-inputs-file-path "$FIXTURE_DIR/public-inputs-epoch-0.json"

banner "Done — testnet is ready for PR #84 QA"
echo "Contract:    $NEW_CONTRACT_ID"
echo "Relayer:     https://relay.$DOMAIN"
echo "Work dir:    $WORK_DIR  (KEEP_ARTIFACTS=1; rm -rf when QA is over)"
echo
echo "Next: update issue #85 checkboxes for Phase 1 and Phase 2, then hand"
echo "off to QA using the #79–#83 issue scripts."
