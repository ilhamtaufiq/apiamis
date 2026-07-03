# syntax=docker/dockerfile:1.7

# Stage 1: Build PHP dependencies
FROM composer:2 AS vendor
WORKDIR /app
COPY composer.json composer.lock ./
ENV COMPOSER_CACHE_DIR=/tmp/composer-cache
RUN --mount=type=cache,target=/tmp/composer-cache \
    composer install --no-dev --no-interaction --no-scripts --prefer-dist --no-progress --ignore-platform-reqs

# Stage 2: Build Node.js assets
FROM node:20-alpine AS asset-builder
WORKDIR /app
# Install Python and build dependencies for @ilhamtaufiq/rab-analyzer post-install scripts
RUN apk add --no-cache python3 make g++ 
COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm \
    npm ci --no-audit --no-fund
COPY vite.config.js ./
COPY resources ./resources
RUN npm run build

# Stage 3: Final Production Image
FROM php:8.3-apache
WORKDIR /var/www/html

# Enable Apache rewrite, headers, and WebSocket proxy for Reverb (/app/*)
RUN a2enmod rewrite headers proxy proxy_http proxy_wstunnel

# Install runtime dependencies & PHP Extensions
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
    libpng-dev libjpeg-dev libfreetype6-dev \
    libonig-dev libxml2-dev libzip-dev libicu-dev \
    python3 python3-pip python3-venv \
    && docker-php-ext-configure gd --with-freetype --with-jpeg \
    && docker-php-ext-install pdo_mysql mbstring exif pcntl bcmath gd zip intl \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

# Copy requirements first for better caching
COPY requirements.txt ./

# Create Python venv and install dependencies
RUN --mount=type=cache,target=/root/.cache/pip,sharing=locked \
    python3 -m venv venv \
    && ./venv/bin/pip install -r requirements.txt

# Set PHP configuration for file uploads
RUN echo "upload_max_filesize = 50M" > /usr/local/etc/php/conf.d/uploads.ini \
    && echo "post_max_size = 50M" >> /usr/local/etc/php/conf.d/uploads.ini

# Copy the application code
COPY --chown=www-data:www-data . .

# Copy vendor and built assets from previous stages
COPY --from=vendor --chown=www-data:www-data /app/vendor /var/www/html/vendor
COPY --from=asset-builder --chown=www-data:www-data /app/public/build /var/www/html/public/build

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

# Configure Apache document root
RUN sed -i 's|/var/www/html|/var/www/html/public|g' /etc/apache2/sites-available/000-default.conf
RUN echo '<Directory /var/www/html/public>\n    AllowOverride All\n</Directory>' >> /etc/apache2/apache2.conf

# Reverb WebSocket reverse proxy (Apache :80 -> Reverb :8080)
COPY docker/apache-reverb-proxy.conf /etc/apache2/conf-available/reverb-proxy.conf
RUN a2enconf reverb-proxy

# Copy and make entrypoint executable
COPY docker-entrypoint.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

EXPOSE 80 8080

CMD ["/usr/local/bin/docker-entrypoint.sh"]
