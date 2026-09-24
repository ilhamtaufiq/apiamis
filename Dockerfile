# syntax=docker/dockerfile:1.7
# Catatan build cepat:
# - Ekstensi PHP via prebuilt binary (mlocati), BUKAN docker-php-ext-install
#   yang mengkompilasi gd/intl dari source (hemat ~4-7 menit di VPS kecil).
# - requirements.txt hanya berisi dep yang benar-benar dipakai
#   (scripts/rag_query.py -> chromadb), dipin ke versi ber-wheel prebuilt.
# - Di Coolify aktifkan Docker Build Cache agar stage yang tidak berubah
#   tidak dibangun ulang tiap deploy.

# Stage 1: PHP dependencies (cache bertahan selama composer.* tidak berubah)
FROM composer:2 AS vendor
WORKDIR /app
COPY composer.json composer.lock ./
ENV COMPOSER_CACHE_DIR=/tmp/composer-cache
RUN --mount=type=cache,target=/tmp/composer-cache \
    composer install --no-dev --no-interaction --no-scripts --prefer-dist --no-progress --ignore-platform-reqs

# Stage 2: WhatsApp Baileys bridge (sidecar dalam container yang sama dengan Laravel)
FROM node:20-bookworm-slim AS whatsapp-bridge
WORKDIR /bridge
COPY docker/whatsapp-bridge/package.json docker/whatsapp-bridge/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm \
    npm ci --omit=dev --no-audit --no-fund
COPY docker/whatsapp-bridge/bridge.mjs docker/whatsapp-bridge/chat-store.mjs ./

# Stage 3: Frontend assets (vite+tailwind sudah prebuilt; tanpa python/make/g++)
FROM node:20-alpine AS asset-builder
WORKDIR /app
COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm \
    npm ci --no-audit --no-fund
COPY vite.config.js ./
COPY resources ./resources
RUN npm run build

# Stage 4: Final production image
FROM php:8.3-apache-bookworm
WORKDIR /var/www/html

# Enable Apache rewrite, headers, and WebSocket proxy for Reverb (/app/*)
RUN a2enmod rewrite headers proxy proxy_http proxy_wstunnel

# Ekstensi PHP sebagai binary prebuilt (detik, bukan menit).
COPY --from=mlocati/php-extension-installer /usr/bin/install-php-extensions /usr/local/bin/
RUN install-php-extensions pdo_mysql mbstring exif pcntl bcmath gd zip intl

# Python hanya untuk venv scripts/rag_query.py (dipanggil ChatRagContextService).
# python3-venv sudah membawa ensurepip, jadi python3-pip sistem tidak diperlukan.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
    python3 python3-venv \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

# requirements.txt dipin (lihat komentar di file) agar selalu memakai wheel
# prebuilt dan layer ini ter-cache selama file tidak berubah.
COPY requirements.txt ./
RUN --mount=type=cache,target=/root/.cache/pip,sharing=locked \
    python3 -m venv venv \
    && ./venv/bin/pip install --prefer-binary -r requirements.txt

# Set PHP configuration for file uploads
RUN echo "upload_max_filesize = 50M" > /usr/local/etc/php/conf.d/uploads.ini \
    && echo "post_max_size = 50M" >> /usr/local/etc/php/conf.d/uploads.ini

# Copy the application code
COPY --chown=www-data:www-data . .

# Copy vendor and built assets from previous stages
COPY --from=vendor --chown=www-data:www-data /app/vendor /var/www/html/vendor
COPY --from=asset-builder --chown=www-data:www-data /app/public/build /var/www/html/public/build
COPY --from=whatsapp-bridge --chown=www-data:www-data /bridge /var/www/html/docker/whatsapp-bridge
COPY --from=whatsapp-bridge /usr/local/bin/node /usr/local/bin/node

# Finalize setup
RUN mkdir -p storage/framework/{cache/data,sessions,views} \
    && mkdir -p storage/logs \
    && mkdir -p bootstrap/cache \
    && mkdir -p storage/ai \
    && chown -R www-data:www-data storage bootstrap/cache \
    && chmod -R 775 storage bootstrap/cache

# Clear any cached files
RUN rm -rf bootstrap/cache/*.php \
    && rm -rf storage/framework/cache/data/* \
    && rm -rf storage/framework/sessions/* \
    && rm -rf storage/framework/views/*

# Apache vhost: Laravel public/ + Reverb proxy (/app, /apps -> :8080)
COPY docker/000-default.conf /etc/apache2/sites-available/000-default.conf

# Copy and make entrypoint executable
COPY docker-entrypoint.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

EXPOSE 80 8080

CMD ["/usr/local/bin/docker-entrypoint.sh"]
