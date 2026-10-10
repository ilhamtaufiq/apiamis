# syntax=docker/dockerfile:1.7
# Image Rust-only (Fase 4 migrasi Laravel): tanpa PHP. Apache hanya melayani /storage dan
# meneruskan /, /up, dan /api ke binary Rust.

# Stage 1: binary Rust (apiamis-api)
FROM rust:1-bookworm AS rust-build
RUN apt-get update && apt-get install -y --no-install-recommends cmake clang \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app/rust
COPY rust/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/rust/target \
    cargo build --release -p api --locked \
    && cp target/release/api /usr/local/bin/apiamis-api

# Stage 2: runtime tanpa PHP
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        apache2 ca-certificates libssl3 curl util-linux \
    && a2enmod proxy proxy_http headers alias \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /var/www/html
COPY docker/000-rust.conf /etc/apache2/sites-available/000-default.conf
COPY --from=rust-build /usr/local/bin/apiamis-api /usr/local/bin/apiamis-api
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

# Aset repo yang dibaca binary Rust saat berjalan: template dokumen kontrak dan GeoJSON desa.
# Lokasinya diberikan lewat env, karena path bawaan di kode mengacu ke folder sumber saat build.
COPY storage/app/templates /var/www/html/storage/app/templates
COPY resources/geojson /var/www/html/resources/geojson
ENV KONTRAK_TEMPLATE_DIR=/var/www/html/storage/app/templates \
    VILLAGE_GEOJSON_PATH=/var/www/html/resources/geojson/id3203_cianjur_simplified.geojson \
    SPAM_IMPORT_DIR=/var/www/html/storage/app/temp

RUN chmod +x /usr/local/bin/docker-entrypoint.sh \
    && mkdir -p /var/www/html/public /var/www/html/storage/app/public /var/www/html/storage/app/temp

# Healthcheck memeriksa API Rust langsung (bukan Apache), supaya container dianggap siap
# hanya bila /api/health di binary Rust menjawab.
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8000/api/health || exit 1

EXPOSE 80
CMD ["/usr/local/bin/docker-entrypoint.sh"]
