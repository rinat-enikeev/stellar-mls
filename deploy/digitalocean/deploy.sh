#!/usr/bin/env bash
#
# deploy.sh — Deploy Stellar MLS infrastructure to Digital Ocean
#
# Idempotent: safe to run multiple times. Reuses existing droplet,
# updates DNS, re-uploads config, rebuilds containers.
#
# Usage:
#   ./deploy/digitalocean/deploy.sh
#
# Required (prompted on first run, saved to .env for subsequent runs):
#   DO_API_KEY          — Digital Ocean API token
#   CF_API_TOKEN        — Cloudflare API token (DNS edit permission)
#   DOMAIN              — Domain name (e.g. onym.chat)
#   CERTBOT_EMAIL       — Email for Let's Encrypt
#
# Optional environment variables:
#   DO_REGION           — DO region (default: ams3)
#   DO_DROPLET_SIZE     — Droplet size (default: s-2vcpu-4gb)
#   SSH_KEY_PATH        — Path to SSH private key (default: ~/.ssh/id_ed25519)
#   GIT_REPO            — Git repository URL (default: auto-detect from origin)
#
# Note: Stellar relayer credentials are in relayer/.env (uploaded to server).
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
ENV_FILE="$REPO_ROOT/.env"

# ─── Load saved config from .env ──────────────────────────────────────

if [ -f "$ENV_FILE" ]; then
    set -a
    source "$ENV_FILE"
    set +a
fi

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}==> $*${NC}"; }
ok()    { echo -e "${GREEN}==> $*${NC}"; }
warn()  { echo -e "${YELLOW}==> $*${NC}"; }
err()   { echo -e "${RED}==> ERROR: $*${NC}" >&2; }

prompt_var() {
    local var_name="$1"
    local prompt_msg="$2"
    local is_secret="${3:-false}"

    if [ -z "${!var_name:-}" ]; then
        if [ "$is_secret" = "true" ]; then
            read -rsp "$prompt_msg: " "$var_name"
            echo ""
        else
            read -rp "$prompt_msg: " "$var_name"
        fi
        export "$var_name"
    fi
}

save_env() {
    cat > "$ENV_FILE" <<SAVE_ENV
DO_API_KEY=$DO_API_KEY
CF_API_TOKEN=$CF_API_TOKEN
DOMAIN=$DOMAIN
CERTBOT_EMAIL=$CERTBOT_EMAIL
DO_REGION=$DO_REGION
DO_DROPLET_SIZE=$DO_DROPLET_SIZE
SSH_KEY_PATH=$SSH_KEY_PATH
GIT_REPO=$GIT_REPO
DROPLET_ID=${DROPLET_ID:-}
DROPLET_IP=${DROPLET_IP:-}
SAVE_ENV
}

# ─── Gather Configuration ─────────────────────────────────────────────

echo ""
echo "════════════════════════════════════════════════"
echo "  Stellar MLS — Digital Ocean Deployment"
echo "════════════════════════════════════════════════"
echo ""

prompt_var DO_API_KEY "Digital Ocean API token" true
prompt_var CF_API_TOKEN "Cloudflare API token (DNS edit permission)" true
prompt_var DOMAIN "Domain name (e.g. onym.chat)"
prompt_var CERTBOT_EMAIL "Email for Let's Encrypt SSL certificates"

DO_REGION="${DO_REGION:-ams3}"
DO_DROPLET_SIZE="${DO_DROPLET_SIZE:-s-2vcpu-4gb}"
SSH_KEY_PATH="${SSH_KEY_PATH:-$HOME/.ssh/id_ed25519}"

# Auto-detect git repo — always convert SSH to HTTPS so the droplet can clone without keys
if [ -z "${GIT_REPO:-}" ]; then
    GIT_REPO=$(git remote get-url origin 2>/dev/null || echo "https://github.com/rinat-enikeev/stellar-mls.git")
fi
GIT_REPO=$(echo "$GIT_REPO" | sed -E 's|^git@github\.com:|https://github.com/|; s|\.git$||').git

save_env
ok "Configuration saved to .env"

info "Configuration:"
echo "  Domain:       $DOMAIN"
echo "  Email:        $CERTBOT_EMAIL"
echo "  Region:       $DO_REGION"
echo "  Droplet size: $DO_DROPLET_SIZE"
echo "  Git repo:     $GIT_REPO"
if [ -n "${DROPLET_IP:-}" ]; then
    echo "  Droplet IP:   $DROPLET_IP (existing)"
fi
echo ""

# ─── Prerequisites ─────────────────────────────────────────────────────

info "Checking prerequisites..."

if ! command -v doctl &>/dev/null; then
    err "doctl not found. Install it: https://docs.digitalocean.com/reference/doctl/how-to/install/"
    echo ""
    echo "  macOS:  brew install doctl"
    echo "  Linux:  snap install doctl"
    echo ""
    exit 1
fi

if [ ! -f "$SSH_KEY_PATH" ]; then
    warn "SSH key not found at $SSH_KEY_PATH, generating one..."
    ssh-keygen -t ed25519 -f "$SSH_KEY_PATH" -N "" -q
    ok "SSH key generated at $SSH_KEY_PATH"
fi

# ─── Authenticate ──────────────────────────────────────────────────────

info "Authenticating with Digital Ocean..."
doctl auth init --access-token "$DO_API_KEY" 2>/dev/null
ok "Authenticated"

# ─── Upload SSH Key ────────────────────────────────────────────────────

info "Ensuring SSH key is uploaded to Digital Ocean..."
SSH_PUB_KEY=$(cat "${SSH_KEY_PATH}.pub")
SSH_KEY_FINGERPRINT=$(ssh-keygen -lf "${SSH_KEY_PATH}.pub" -E md5 | awk '{print $2}' | sed 's/MD5://')

if doctl compute ssh-key get "$SSH_KEY_FINGERPRINT" &>/dev/null; then
    ok "SSH key already exists on Digital Ocean"
else
    doctl compute ssh-key create "onym-deploy-key" --public-key "$SSH_PUB_KEY" --format ID --no-header
    ok "SSH key uploaded"
fi

# ─── Create or Reuse Droplet ─────────────────────────────────────────

if [ -n "${DROPLET_ID:-}" ]; then
    # Verify the saved droplet still exists
    if doctl compute droplet get "$DROPLET_ID" &>/dev/null; then
        DROPLET_IP=$(doctl compute droplet get "$DROPLET_ID" --format PublicIPv4 --no-header)
        ok "Reusing existing droplet: ID=$DROPLET_ID IP=$DROPLET_IP"
    else
        warn "Saved droplet $DROPLET_ID no longer exists, creating new one..."
        DROPLET_ID=""
        DROPLET_IP=""
    fi
fi

if [ -z "${DROPLET_ID:-}" ]; then
    CLOUD_INIT=$(cat <<'CLOUD_INIT_EOF'
#!/bin/bash
set -euo pipefail
curl -fsSL https://get.docker.com | sh
apt-get install -y docker-compose-plugin git ufw
ufw allow 22/tcp
ufw allow 80/tcp
ufw allow 443/tcp
ufw --force enable
touch /tmp/cloud-init-done
CLOUD_INIT_EOF
)

    DROPLET_NAME="onym-chat-$(date +%s)"
    info "Creating droplet '$DROPLET_NAME'..."
    DROPLET_ID=$(doctl compute droplet create "$DROPLET_NAME" \
        --image ubuntu-24-04-x64 \
        --size "$DO_DROPLET_SIZE" \
        --region "$DO_REGION" \
        --ssh-keys "$SSH_KEY_FINGERPRINT" \
        --user-data "$CLOUD_INIT" \
        --wait \
        --format ID \
        --no-header)

    DROPLET_IP=$(doctl compute droplet get "$DROPLET_ID" --format PublicIPv4 --no-header)
    ok "Droplet created: ID=$DROPLET_ID IP=$DROPLET_IP"

    # Save immediately so we can resume if script fails later
    save_env

    # Wait for cloud-init
    info "Waiting for droplet to finish cloud-init..."
    for i in $(seq 1 60); do
        if ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 \
            -i "$SSH_KEY_PATH" "root@$DROPLET_IP" "test -f /tmp/cloud-init-done" 2>/dev/null; then
            ok "Droplet ready!"
            break
        fi
        if [ "$i" -eq 60 ]; then
            err "Timeout waiting for cloud-init. SSH in manually: ssh -i $SSH_KEY_PATH root@$DROPLET_IP"
            exit 1
        fi
        echo -n "."
        sleep 10
    done
    echo ""
fi

SSH_CMD="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 -i $SSH_KEY_PATH root@$DROPLET_IP"
SCP_CMD="scp -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i $SSH_KEY_PATH"

# ─── Configure DNS (Cloudflare) ───────────────────────────────────────

info "Configuring DNS for $DOMAIN via Cloudflare..."

CF_API="https://api.cloudflare.com/client/v4"

CF_ZONE_ID=$(curl -s -X GET "$CF_API/zones?name=$DOMAIN" \
    -H "Authorization: Bearer $CF_API_TOKEN" \
    -H "Content-Type: application/json" | \
    python3 -c "import sys,json; print(json.load(sys.stdin)['result'][0]['id'])")

if [ -z "$CF_ZONE_ID" ]; then
    err "Could not find Cloudflare zone for $DOMAIN. Ensure the domain is added to your Cloudflare account."
    exit 1
fi
ok "Cloudflare zone ID: $CF_ZONE_ID"

cf_ensure_record() {
    local name="$1"
    local ip="$2"
    local full_name
    if [ "$name" = "@" ]; then
        full_name="$DOMAIN"
    else
        full_name="${name}.${DOMAIN}"
    fi

    # Check if record already points to the right IP
    local result
    result=$(curl -s -X GET "$CF_API/zones/$CF_ZONE_ID/dns_records?type=A&name=$full_name" \
        -H "Authorization: Bearer $CF_API_TOKEN" \
        -H "Content-Type: application/json")

    local existing_id existing_ip
    existing_id=$(echo "$result" | python3 -c "import sys,json; r=json.load(sys.stdin)['result']; print(r[0]['id'] if r else '')" 2>/dev/null)
    existing_ip=$(echo "$result" | python3 -c "import sys,json; r=json.load(sys.stdin)['result']; print(r[0]['content'] if r else '')" 2>/dev/null)

    if [ -n "$existing_id" ] && [ "$existing_ip" = "$ip" ]; then
        ok "  $full_name -> $ip (already set)"
    elif [ -n "$existing_id" ]; then
        curl -s -X PUT "$CF_API/zones/$CF_ZONE_ID/dns_records/$existing_id" \
            -H "Authorization: Bearer $CF_API_TOKEN" \
            -H "Content-Type: application/json" \
            --data "{\"type\":\"A\",\"name\":\"$full_name\",\"content\":\"$ip\",\"ttl\":300,\"proxied\":false}" >/dev/null
        ok "  Updated $full_name -> $ip"
    else
        curl -s -X POST "$CF_API/zones/$CF_ZONE_ID/dns_records" \
            -H "Authorization: Bearer $CF_API_TOKEN" \
            -H "Content-Type: application/json" \
            --data "{\"type\":\"A\",\"name\":\"$full_name\",\"content\":\"$ip\",\"ttl\":300,\"proxied\":false}" >/dev/null
        ok "  Created $full_name -> $ip"
    fi
}

cf_ensure_record "@" "$DROPLET_IP"
cf_ensure_record "relay" "$DROPLET_IP"
cf_ensure_record "nostr" "$DROPLET_IP"
cf_ensure_record "blossom" "$DROPLET_IP"
cf_ensure_record "push" "$DROPLET_IP"

# ─── Deploy Application ───────────────────────────────────────────────

info "Syncing repository on droplet..."
$SSH_CMD "if [ -d /opt/onym-chat/.git ]; then cd /opt/onym-chat && git fetch origin && git reset --hard origin/main; else git clone $GIT_REPO /opt/onym-chat; fi"

# Overlay local deploy/ and docker-compose.yml on top of the clone
# so uncommitted/unpushed changes are always applied
info "Uploading local config files..."
$SSH_CMD "rm -rf /opt/onym-chat/deploy"
$SCP_CMD -r "$REPO_ROOT/deploy" "root@$DROPLET_IP:/opt/onym-chat/deploy" 2>/dev/null
$SCP_CMD "$REPO_ROOT/docker-compose.yml" "root@$DROPLET_IP:/opt/onym-chat/docker-compose.yml" 2>/dev/null
$SCP_CMD "$REPO_ROOT/relayer/Dockerfile" "root@$DROPLET_IP:/opt/onym-chat/relayer/Dockerfile" 2>/dev/null

info "Uploading relayer .env..."
$SCP_CMD "$REPO_ROOT/relayer/.env" "root@$DROPLET_IP:/opt/onym-chat/relayer/.env" 2>/dev/null
$SSH_CMD "sed -i 's/^RELAYER_BIND=.*/RELAYER_BIND=0.0.0.0:8080/' /opt/onym-chat/relayer/.env"

info "Uploading PN relay config..."
$SSH_CMD "mkdir -p /opt/onym-chat/pn-relay"
if [ -f "$REPO_ROOT/pn-relay/.env" ]; then
    $SCP_CMD "$REPO_ROOT/pn-relay/.env" "root@$DROPLET_IP:/opt/onym-chat/pn-relay/.env" 2>/dev/null
else
    # Create a minimal .env if none exists
    $SSH_CMD "touch /opt/onym-chat/pn-relay/.env"
fi
$SCP_CMD "$REPO_ROOT/pn-relay/Cargo.toml" "root@$DROPLET_IP:/opt/onym-chat/pn-relay/Cargo.toml" 2>/dev/null
$SCP_CMD "$REPO_ROOT/pn-relay/Dockerfile" "root@$DROPLET_IP:/opt/onym-chat/pn-relay/Dockerfile" 2>/dev/null
$SCP_CMD -r "$REPO_ROOT/pn-relay/src" "root@$DROPLET_IP:/opt/onym-chat/pn-relay/src" 2>/dev/null

info "Pulling images and building containers (this may take a few minutes on first run)..."
$SSH_CMD "cd /opt/onym-chat && docker compose pull --ignore-buildable && docker compose build --pull" 2>&1 | tail -5

# ─── Wait for DNS Propagation ─────────────────────────────────────────

info "Waiting for DNS propagation (all 4 subdomains)..."
ALL_DOMAINS=("$DOMAIN" "relay.$DOMAIN" "nostr.$DOMAIN" "blossom.$DOMAIN" "push.$DOMAIN")
MAX_WAIT=60  # 60 x 10s = 10 minutes

for d in "${ALL_DOMAINS[@]}"; do
    for i in $(seq 1 $MAX_WAIT); do
        RESOLVED=$(dig +short "$d" @1.1.1.1 2>/dev/null || true)
        if [ "$RESOLVED" = "$DROPLET_IP" ]; then
            ok "  $d -> $DROPLET_IP"
            break
        fi
        if [ "$i" -eq $MAX_WAIT ]; then
            err "DNS for $d did not propagate within 10 minutes."
            err "Run SSL setup manually after DNS propagates:"
            err "  ssh -i $SSH_KEY_PATH root@$DROPLET_IP"
            err "  cd /opt/onym-chat && bash deploy/certbot/init-certs.sh '$CERTBOT_EMAIL' '$DOMAIN'"
            exit 1
        fi
        echo -n "."
        sleep 10
    done
done
echo ""

# ─── SSL Certificates ─────────────────────────────────────────────────

info "Bootstrapping SSL certificates..."
$SSH_CMD "cd /opt/onym-chat && bash deploy/certbot/init-certs.sh '$CERTBOT_EMAIL' '$DOMAIN'" 2>&1

# ─── Verify ────────────────────────────────────────────────────────────

info "Verifying deployment..."
sleep 5

check_endpoint() {
    local url="$1"
    local label="$2"
    local status
    status=$(curl -o /dev/null -s -w "%{http_code}" --max-time 10 "$url" 2>/dev/null || echo "000")
    if [ "$status" -ge 200 ] && [ "$status" -lt 500 ]; then
        ok "  $label — HTTP $status"
    else
        warn "  $label — HTTP $status (may need a moment to start)"
    fi
}

check_endpoint "https://$DOMAIN" "Website"
check_endpoint "https://relay.$DOMAIN" "Stellar Relayer"
check_endpoint "https://blossom.$DOMAIN" "Blossom Server"
check_endpoint "https://push.$DOMAIN/v1/health" "Push Relay"

echo ""
echo "════════════════════════════════════════════════════════════"
echo ""
echo -e "  ${GREEN}Deployment complete!${NC}"
echo ""
echo "  Website:         https://$DOMAIN"
echo "  Stellar Relayer: https://relay.$DOMAIN"
echo "  Nostr Relay:     wss://nostr.$DOMAIN"
echo "  Blossom Server:  https://blossom.$DOMAIN"
echo "  Push Relay:      https://push.$DOMAIN"
echo ""
echo "  Droplet IP:      $DROPLET_IP"
echo "  SSH:             ssh -i $SSH_KEY_PATH root@$DROPLET_IP"
echo ""
echo "  Configure your mobile apps:"
echo "    Relayer URL:   https://relay.$DOMAIN"
echo "    Nostr relays:  wss://nostr.$DOMAIN"
echo "    Blossom:       https://blossom.$DOMAIN"
echo ""
echo "════════════════════════════════════════════════════════════"
