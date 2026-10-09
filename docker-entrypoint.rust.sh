#!/bin/bash
set -e

log() {
    echo "[entrypoint] $*"
}

if [ -z "${DATABASE_URL:-}" ]; then
    log "ERROR: DATABASE_URL belum di-set"
    exit 1
fi

mkdir -p /var/www/html/storage/app/public

log "Menjalankan migrasi schema..."
/usr/local/bin/apiamis-api migrate || { log "ERROR: migrasi gagal, API tidak dijalankan"; exit 1; }

log "Menjalankan API Rust di port ${APP_PORT:-8000}..."
/usr/local/bin/apiamis-api &
API_PID=$!

# Pengganti jadwal Laravel (routes/console.php): hapus aset blog yatim setiap 24 jam.
(
    while true; do
        sleep 86400
        log "cron: blog-assets-cleanup-orphans --hours=24"
        /usr/local/bin/apiamis-api blog-assets-cleanup-orphans --hours=24 --yes \
            || log "cron: cleanup gagal, akan dicoba lagi besok"
    done
) &
CRON_PID=$!

apache2ctl -D FOREGROUND &
APACHE_PID=$!

shutdown() {
    kill "$API_PID" "$CRON_PID" "$APACHE_PID" 2>/dev/null || true
    wait 2>/dev/null || true
    exit 0
}
trap shutdown SIGTERM SIGINT

# Keluar bila salah satu proses berhenti, supaya container di-restart orkestrator.
wait -n || true
log "Salah satu proses berhenti, menghentikan container"
shutdown
