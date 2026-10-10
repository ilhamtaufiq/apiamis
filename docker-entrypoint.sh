#!/bin/bash
set -e

log() {
    echo "[entrypoint] $*"
}

if [ -z "${DATABASE_URL:-}" ]; then
    log "ERROR: DATABASE_URL belum di-set"
    exit 1
fi

STORAGE=/var/www/html/storage/app/public
mkdir -p "$STORAGE"

# Berkas upload dibaca Apache (www-data) dan ditulis API. Jalankan API sebagai www-data
# dengan umask 0002, supaya berkas baru bisa dibaca dan ditulis user yang sama.
# Berkas lama yang dimiliki user lain (mis. root) dibetulkan kepemilikannya di sini.
find "$STORAGE" ! -user www-data -exec chown www-data:www-data {} + 2>/dev/null || true
API_RUN() {
    umask 0002
    exec setpriv --reuid=www-data --regid=www-data --init-groups "$@"
}

log "Menjalankan migrasi schema..."
(API_RUN /usr/local/bin/apiamis-api migrate) || { log "ERROR: migrasi gagal, API tidak dijalankan"; exit 1; }

log "Menjalankan API Rust di port ${APP_PORT:-8000}..."
(API_RUN /usr/local/bin/apiamis-api) &
API_PID=$!

# Pengganti jadwal Laravel (routes/console.php): hapus aset blog yatim setiap 24 jam.
(
    while true; do
        sleep 86400
        log "cron: blog-assets-cleanup-orphans --hours=24"
        (API_RUN /usr/local/bin/apiamis-api blog-assets-cleanup-orphans --hours=24 --yes) \
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
