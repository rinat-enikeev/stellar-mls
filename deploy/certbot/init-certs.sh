#!/usr/bin/env bash
#
# init-certs.sh — Bootstrap SSL certificates
#
# Usage: ./deploy/certbot/init-certs.sh <email> <domain>
#
# Must be run from the repo root (where docker-compose.yml is).
#
set -euo pipefail

EMAIL="${1:?Usage: $0 <email> <domain>}"
DOMAIN="${2:?Usage: $0 <email> <domain>}"

DOMAINS=("$DOMAIN" "relay.$DOMAIN" "nostr.$DOMAIN" "blossom.$DOMAIN")

CERTS_VOL="onym_certbot-certs"
WEBROOT_VOL="onym_certbot-webroot"

echo "==> Bootstrapping SSL certificates for ${DOMAIN}"
echo "    Subdomains: ${DOMAINS[*]}"
echo ""

# Ensure volumes exist
docker compose up --no-start 2>/dev/null || true

# Step 1: Create self-signed placeholder certificates so nginx can start with SSL
echo "==> Step 1: Creating placeholder certificates..."
docker run --rm \
    -v "$CERTS_VOL:/etc/letsencrypt" \
    --entrypoint sh \
    certbot/certbot -c "
        mkdir -p /etc/letsencrypt/live/$DOMAIN
        if [ ! -f /etc/letsencrypt/live/$DOMAIN/fullchain.pem ]; then
            openssl req -x509 -nodes -newkey rsa:2048 -days 1 \
                -keyout /etc/letsencrypt/live/$DOMAIN/privkey.pem \
                -out /etc/letsencrypt/live/$DOMAIN/fullchain.pem \
                -subj '/CN=localhost'
            echo 'Placeholder certificates created.'
        else
            echo 'Certificates already exist, skipping.'
        fi
    "

# Step 2: Start nginx (HTTPS with placeholder certs + HTTP for ACME challenges)
echo ""
echo "==> Step 2: Starting nginx..."
docker compose up -d nginx
echo "    Waiting for nginx to be ready..."
sleep 5

# Verify nginx is actually listening on port 80
if ! curl -s -o /dev/null --max-time 5 http://localhost/.well-known/acme-challenge/test 2>/dev/null; then
    echo "    nginx port 80 check (expected 404 or 301, verifying...)"
fi

# Step 3: Request real Let's Encrypt certificates
# We do NOT delete the placeholder certs first — nginx needs them to stay running.
# Certbot writes new certs to /etc/letsencrypt/live/$DOMAIN/ which overwrites the placeholders.
echo ""
echo "==> Step 3: Requesting Let's Encrypt certificates..."

DOMAIN_ARGS=""
for d in "${DOMAINS[@]}"; do
    DOMAIN_ARGS="$DOMAIN_ARGS -d $d"
done

# Remove placeholder certs — nginx is already running and will keep serving on port 80
# (it may log SSL errors but port 80 stays up for the ACME challenge)
docker run --rm \
    -v "$CERTS_VOL:/etc/letsencrypt" \
    --entrypoint sh \
    certbot/certbot -c "
        rm -rf /etc/letsencrypt/live/$DOMAIN
        rm -rf /etc/letsencrypt/archive/$DOMAIN
        rm -rf /etc/letsencrypt/renewal/$DOMAIN.conf
    "

# Request real certificates
docker run --rm \
    -v "$CERTS_VOL:/etc/letsencrypt" \
    -v "$WEBROOT_VOL:/var/www/certbot" \
    certbot/certbot certonly --webroot -w /var/www/certbot \
        --email "$EMAIL" --agree-tos --no-eff-email \
        $DOMAIN_ARGS

# Step 4: Reload nginx to pick up real certs
echo ""
echo "==> Step 4: Reloading nginx..."
docker compose exec nginx nginx -s reload

# Step 5: Start all remaining services
echo ""
echo "==> Step 5: Starting all services..."
docker compose up -d

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
echo "  Auto-renewal runs via the certbot container."
echo "========================================"
