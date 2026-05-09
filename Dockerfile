# Stage 1: Build PHP dependencies
FROM composer:2 AS vendor
WORKDIR /app
COPY composer.json composer.lock ./
RUN --mount=type=cache,target=/tmp/cache \
    composer install --optimize-autoloader --no-dev --no-interaction --no-scripts --ignore-platform-reqs

# Stage 2: Build Node.js assets
FROM node:20-alpine AS asset-builder
WORKDIR /app
COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm \
    npm install
COPY . .
RUN npm run build

# Stage 3: Final Production Image
FROM php:8.3-apache
WORKDIR /var/www/html

# Enable Apache rewrite
RUN a2enmod rewrite

# Install minimal runtime dependencies & PHP Extensions
RUN apt-get update && apt-get install -y \
    git curl zip unzip \
    libpng-dev libjpeg-dev libfreetype6-dev \
    libonig-dev libxml2-dev libzip-dev libicu-dev \
    python3 python3-pip python3-venv build-essential \
    gnupg \
    && docker-php-ext-configure gd --with-freetype --with-jpeg \
    && docker-php-ext-install pdo_mysql mbstring exif pcntl bcmath gd zip intl \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

# Install Python requirements globally for system-wide access
RUN pip3 install --no-cache-dir pandas pdfplumber openpyxl --break-system-packages

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

# Copy and make entrypoint executable
COPY docker-entrypoint.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

EXPOSE 80

CMD ["/usr/local/bin/docker-entrypoint.sh"]
