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

# Start Apache
exec apache2-foreground
