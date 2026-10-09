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

# Clear and cache config for production
php artisan config:clear
php artisan config:cache
php artisan route:cache
php artisan view:cache

# Run AI Knowledge Indexing (hanya bila script dan venv python tersedia)
if [ -f "scripts/index_knowledge.py" ] && [ -x "venv/bin/python" ]; then
    log "Running AI Knowledge Indexing..."
    ./venv/bin/python scripts/index_knowledge.py || log "AI Indexing failed, but continuing..."
fi

RUST_PID=""

start_rust() {
    if [ -z "${DATABASE_URL:-}" ]; then
        log "Skipping Rust API: DATABASE_URL is not set"
        return
    fi

    log "Starting Rust API on port ${APP_PORT:-8000}..."
    /usr/local/bin/apiamis-api >> /proc/1/fd/2 2>&1 &
    RUST_PID=$!

    sleep 2
    if kill -0 "$RUST_PID" 2>/dev/null; then
        log "Rust API started (pid ${RUST_PID})"
    else
        log "ERROR: Rust API exited immediately — check DATABASE_URL, APP_KEY, and logs"
        RUST_PID=""
    fi
}

start_rust

apache2-foreground &
APACHE_PID=$!

shutdown() {
    kill "$APACHE_PID" 2>/dev/null || true
    if [ -n "$RUST_PID" ]; then
        kill "$RUST_PID" 2>/dev/null || true
    fi
    wait "$APACHE_PID" 2>/dev/null || true
    exit 0
}

trap shutdown SIGTERM SIGINT

wait "$APACHE_PID"