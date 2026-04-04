#!/usr/bin/env bash
#
# init-certs.sh — Bootstrap SSL certificates for onym.chat
#
# Usage: ./deploy/certbot/init-certs.sh <email> [domain]
#
# This script:
# 1. Creates self-signed placeholder certs so nginx can start
# 2. Starts nginx + certbot containers
# 3. Obtains real Let's Encrypt certificates
# 4. Reloads nginx with real certs
#
set -euo pipefail

EMAIL="${1:?Usage: $0 <email> [domain]}"
DOMAIN="${2:-onym.chat}"
COMPOSE="docker compose"

DOMAINS=(
    "$DOMAIN"
    "relay.$DOMAIN"
    "nostr.$DOMAIN"
    "blossom.$DOMAIN"
)

CERT_DIR="$(pwd)/deploy/certbot/conf"
WEBROOT_DIR="$(pwd)/deploy/certbot/webroot"

echo "==> Bootstrapping SSL certificates for ${DOMAIN}"
echo "    Subdomains: ${DOMAINS[*]}"
echo ""

# Step 1: Create self-signed placeholder certificates
echo "==> Step 1: Creating placeholder certificates..."

# We need to create certs in the Docker volume, so we use a temp container
$COMPOSE run --rm --entrypoint "" certbot sh -c "
    mkdir -p /etc/letsencrypt/live/${DOMAIN}
    if [ ! -f /etc/letsencrypt/live/${DOMAIN}/fullchain.pem ]; then
        openssl req -x509 -nodes -newkey rsa:2048 -days 1 \
            -keyout /etc/letsencrypt/live/${DOMAIN}/privkey.pem \
            -out /etc/letsencrypt/live/${DOMAIN}/fullchain.pem \
            -subj '/CN=localhost'
        echo 'Placeholder certificates created.'
    else
        echo 'Certificates already exist, skipping placeholder creation.'
    fi
"

# Step 2: Start nginx (it can now start with the placeholder certs)
echo ""
echo "==> Step 2: Starting nginx..."
$COMPOSE up -d nginx
sleep 3

# Step 3: Delete placeholder certs and request real ones
echo ""
echo "==> Step 3: Requesting Let's Encrypt certificates..."

# Build the -d flags for certbot
DOMAIN_ARGS=""
for d in "${DOMAINS[@]}"; do
    DOMAIN_ARGS="$DOMAIN_ARGS -d $d"
done

# Remove placeholder certs
$COMPOSE run --rm --entrypoint "" certbot sh -c "
    rm -rf /etc/letsencrypt/live/${DOMAIN}
    rm -rf /etc/letsencrypt/archive/${DOMAIN}
    rm -rf /etc/letsencrypt/renewal/${DOMAIN}.conf
"

# Request real certificates
$COMPOSE run --rm certbot certonly \
    --webroot \
    -w /var/www/certbot \
    --email "$EMAIL" \
    --agree-tos \
    --no-eff-email \
    --force-renewal \
    $DOMAIN_ARGS

# Step 4: Reload nginx with real certs
echo ""
echo "==> Step 4: Reloading nginx with real certificates..."
$COMPOSE exec nginx nginx -s reload

# Step 5: Start all services
echo ""
echo "==> Step 5: Starting all services..."
$COMPOSE up -d

echo ""
echo "========================================"
echo "  SSL certificates installed!"
echo "========================================"
echo ""
echo "  https://${DOMAIN}"
echo "  https://relay.${DOMAIN}"
echo "  wss://nostr.${DOMAIN}"
echo "  https://blossom.${DOMAIN}"
echo ""
echo "  Certificates will auto-renew via the certbot container."
echo "========================================"
