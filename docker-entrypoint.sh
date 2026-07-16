#!/bin/bash
set -e

log() {
    echo "[entrypoint] $*"
}

# Create storage directories if they don't exist
mkdir -p /var/www/html/storage/framework/{cache/data,sessions,views}
mkdir -p /var/www/html/storage/logs
mkdir -p /var/www/html/bootstrap/cache
mkdir -p /var/www/html/storage/app/public

# Set permissions
chown -R www-data:www-data /var/www/html/storage /var/www/html/bootstrap/cache
chmod -R 775 /var/www/html/storage /var/www/html/bootstrap/cache

# Create storage link if it doesn't exist
if [ ! -L /var/www/html/public/storage ]; then
    php artisan storage:link
fi

# Auto-enable Reverb when credentials exist but BROADCAST_CONNECTION was not switched (common Coolify oversight).
if [ -n "${REVERB_APP_KEY:-}" ] && [ "${BROADCAST_CONNECTION:-}" != "reverb" ]; then
    log "REVERB_APP_KEY detected — setting BROADCAST_CONNECTION=reverb (was: ${BROADCAST_CONNECTION:-unset})"
    export BROADCAST_CONNECTION=reverb
fi

log "Broadcast driver: ${BROADCAST_CONNECTION:-unset}"
log "Reverb app key: ${REVERB_APP_KEY:+set}${REVERB_APP_KEY:-unset}"

# Clear and cache config for production (after BROADCAST_CONNECTION export above)
php artisan config:clear
php artisan config:cache
php artisan route:cache
php artisan view:cache

# Run AI Knowledge Indexing
if [ -f "scripts/index_knowledge.py" ]; then
    log "Running AI Knowledge Indexing..."
    ./venv/bin/python scripts/index_knowledge.py || log "AI Indexing failed, but continuing..."
fi

REVERB_PID=""
WHATSAPP_PID=""

start_whatsapp_bridge() {
    if [ "${DISABLE_WHATSAPP_BRIDGE:-false}" = "true" ]; then
        log "WhatsApp bridge disabled via DISABLE_WHATSAPP_BRIDGE=true"
        return
    fi

    if [ ! -f "docker/whatsapp-bridge/bridge.mjs" ]; then
        log "Skipping WhatsApp bridge: docker/whatsapp-bridge/bridge.mjs not found"
        return
    fi

    WHATSAPP_AUTH_DIR="${WHATSAPP_AUTH_DIR:-/var/www/html/storage/app/whatsapp-auth}"
    mkdir -p "$WHATSAPP_AUTH_DIR"
    chown -R www-data:www-data "$WHATSAPP_AUTH_DIR"

    WHATSAPP_HOST_BIND="${WHATSAPP_BRIDGE_HOST:-127.0.0.1}"
    WHATSAPP_PORT_BIND="${WHATSAPP_BRIDGE_PORT:-4000}"
    log "Starting WhatsApp bridge on ${WHATSAPP_HOST_BIND}:${WHATSAPP_PORT_BIND}..."

    WHATSAPP_BRIDGE_HOST="${WHATSAPP_HOST_BIND}" \
    WHATSAPP_BRIDGE_PORT="${WHATSAPP_PORT_BIND}" \
    WHATSAPP_BRIDGE_KEY="${WHATSAPP_BRIDGE_KEY:-}" \
    WHATSAPP_AUTH_DIR="${WHATSAPP_AUTH_DIR}" \
    WHATSAPP_LOG_LEVEL="${WHATSAPP_LOG_LEVEL:-silent}" \
    node docker/whatsapp-bridge/bridge.mjs >> /proc/1/fd/2 2>&1 &
    WHATSAPP_PID=$!

    sleep 1
    if kill -0 "$WHATSAPP_PID" 2>/dev/null; then
        log "WhatsApp bridge started (pid ${WHATSAPP_PID})"
    else
        log "ERROR: WhatsApp bridge exited immediately — check storage/logs"
        WHATSAPP_PID=""
    fi
}

start_reverb() {
    if [ "${DISABLE_REVERB:-false}" = "true" ]; then
        log "Reverb disabled via DISABLE_REVERB=true"
        return
    fi

    if [ -z "${REVERB_APP_KEY:-}" ]; then
        log "Skipping Reverb: REVERB_APP_KEY is not set"
        return
    fi

    if [ "${BROADCAST_CONNECTION:-}" != "reverb" ]; then
        log "Skipping Reverb: BROADCAST_CONNECTION is not reverb (${BROADCAST_CONNECTION:-unset})"
        return
    fi

    REVERB_HOST_BIND="${REVERB_SERVER_HOST:-0.0.0.0}"
    REVERB_PORT_BIND="${REVERB_SERVER_PORT:-8080}"
    log "Starting Laravel Reverb on ${REVERB_HOST_BIND}:${REVERB_PORT_BIND}..."

    REVERB_ARGS=(--host="${REVERB_HOST_BIND}" --port="${REVERB_PORT_BIND}")
    if [ -n "${REVERB_HOST:-}" ]; then
        REVERB_ARGS+=(--hostname="${REVERB_HOST}")
    fi

    php artisan reverb:start "${REVERB_ARGS[@]}" >> /proc/1/fd/2 2>&1 &
    REVERB_PID=$!

    sleep 2
    if kill -0 "$REVERB_PID" 2>/dev/null; then
        log "Reverb started (pid ${REVERB_PID})"
    else
        log "ERROR: Reverb process exited immediately — check REVERB_APP_SECRET and storage/logs"
        REVERB_PID=""
    fi
}

start_reverb
start_whatsapp_bridge

apache2-foreground &
APACHE_PID=$!

shutdown() {
    kill "$APACHE_PID" 2>/dev/null || true
    if [ -n "$REVERB_PID" ]; then
        kill "$REVERB_PID" 2>/dev/null || true
    fi
    if [ -n "$WHATSAPP_PID" ]; then
        kill "$WHATSAPP_PID" 2>/dev/null || true
    fi
    wait "$APACHE_PID" 2>/dev/null || true
    exit 0
}

trap shutdown SIGTERM SIGINT

wait "$APACHE_PID"