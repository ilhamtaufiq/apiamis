#!/bin/bash
set -e

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

# Run AI Knowledge Indexing
if [ -f "scripts/index_knowledge.py" ]; then
    echo "Running AI Knowledge Indexing..."
    ./venv/bin/python scripts/index_knowledge.py || echo "AI Indexing failed, but continuing..."
fi

REVERB_PID=""

if [ "${BROADCAST_CONNECTION:-null}" = "reverb" ]; then
    REVERB_HOST_BIND="${REVERB_SERVER_HOST:-0.0.0.0}"
    REVERB_PORT_BIND="${REVERB_SERVER_PORT:-8080}"
    echo "Starting Laravel Reverb on ${REVERB_HOST_BIND}:${REVERB_PORT_BIND}..."
    REVERB_ARGS=(--host="${REVERB_HOST_BIND}" --port="${REVERB_PORT_BIND}")
    if [ -n "${REVERB_HOST:-}" ]; then
        REVERB_ARGS+=(--hostname="${REVERB_HOST}")
    fi
    php artisan reverb:start "${REVERB_ARGS[@]}" &
    REVERB_PID=$!
fi

apache2-foreground &
APACHE_PID=$!

shutdown() {
    kill "$APACHE_PID" 2>/dev/null || true
    if [ -n "$REVERB_PID" ]; then
        kill "$REVERB_PID" 2>/dev/null || true
    fi
    wait "$APACHE_PID" 2>/dev/null || true
    exit 0
}

trap shutdown SIGTERM SIGINT

wait "$APACHE_PID"
