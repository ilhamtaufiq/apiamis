# 🚀 Panduan Instalasi - APIAMIS

## 📋 Requirements

- **PHP**: ^8.2
- **Composer**: ^2.0
- **MySQL**: ^8.0 atau PostgreSQL ^13
- **Node.js**: ^18.0 (untuk Vite assets)
- **Git**

---

## 📥 Instalasi

### 1. Clone Repository

```bash
git clone <repository-url> apiamis
cd apiamis
```

### 2. Install Dependencies

```bash
# Install PHP dependencies
composer install

# Install Node dependencies (untuk Vite)
npm install
```

### 3. Environment Configuration

```bash
# Copy environment file
cp .env.example .env

# Generate application key
php artisan key:generate
```

### 4. Edit .env File

```env
APP_NAME=APIAMIS
APP_ENV=local
APP_DEBUG=true
APP_URL=http://apiamis.test

# Database
DB_CONNECTION=mysql
DB_HOST=127.0.0.1
DB_PORT=3306
DB_DATABASE=apiamis
DB_USERNAME=root
DB_PASSWORD=

# Sanctum (untuk CORS frontend)
SANCTUM_STATEFUL_DOMAINS=arumanis.test,localhost:5173

# Session
SESSION_DOMAIN=.test
```

### 5. Database Setup

```bash
# Create database
mysql -u root -p -e "CREATE DATABASE apiamis"

# Run migrations
php artisan migrate

# Run seeders
php artisan db:seed
```

### 6. Storage Link

```bash
php artisan storage:link
```

### 7. Generate API Documentation

```bash
php artisan l5-swagger:generate
```

---

## 🖥️ Development Server

### Option 1: PHP Built-in Server

```bash
php artisan serve
# API available at http://localhost:8000
```

### Option 2: Laragon/XAMPP/MAMP

Configure virtual host untuk `apiamis.test`

### Option 3: Laravel Sail (Docker)

```bash
./vendor/bin/sail up
```

### Option 4: Composer Dev Script

```bash
composer dev
# Runs: server + queue + logs + vite
```

---

## ⚙️ Configuration

### CORS (config/cors.php)

```php
return [
    'paths' => ['api/*', 'sanctum/csrf-cookie'],
    'allowed_methods' => ['*'],
    'allowed_origins' => [
        'http://arumanis.test',
        'http://localhost:5173',
        'https://arumanis.ilham.wtf'
    ],
    'allowed_origins_patterns' => [],
    'allowed_headers' => ['*'],
    'exposed_headers' => [],
    'max_age' => 0,
    'supports_credentials' => true,
];
```

### Sanctum (config/sanctum.php)

```php
return [
    'stateful' => explode(',', env('SANCTUM_STATEFUL_DOMAINS', 
        'localhost,localhost:3000,127.0.0.1,127.0.0.1:8000,::1,arumanis.test'
    )),
    'guard' => ['web'],
    'expiration' => null,
];
```

---

## 📝 Default Credentials

Setelah menjalankan seeder:

| Email | Password | Role |
|-------|----------|------|
| admin@example.com | password | admin |

---

## 🐳 Docker Deployment

### Build Image

```bash
docker build -t apiamis .
```

### Run Container

```bash
docker run -d \
  --name apiamis \
  -p 8000:8000 \
  -e DB_HOST=host.docker.internal \
  -e DB_DATABASE=apiamis \
  -e DB_USERNAME=root \
  -e DB_PASSWORD=password \
  apiamis
```

### Using Docker Compose

```yaml
# docker-compose.yml
version: '3.8'

services:
  app:
    build: .
    ports:
      - "8000:8000"
    environment:
      - DB_HOST=db
      - DB_DATABASE=apiamis
      - DB_USERNAME=root
      - DB_PASSWORD=secret
    depends_on:
      - db

  db:
    image: mysql:8
    environment:
      MYSQL_ROOT_PASSWORD: secret
      MYSQL_DATABASE: apiamis
    volumes:
      - db_data:/var/lib/mysql

volumes:
  db_data:
```

---

## 🔄 Update & Maintenance

### Update Dependencies

```bash
composer update
npm update
```

### Clear Cache

```bash
php artisan cache:clear
php artisan config:clear
php artisan route:clear
php artisan view:clear
```

### Fresh Migration

```bash
php artisan migrate:fresh --seed
```

---

## 🧪 Testing

```bash
# Run all tests
php artisan test

# Run specific test
php artisan test --filter=AuthTest

# With coverage
php artisan test --coverage
```

---

## 🔧 Troubleshooting

### Error: SQLSTATE[HY000] [1045] Access denied

Check database credentials in `.env`

### Error: Class not found

```bash
composer dump-autoload
```

### Error: Permission denied on storage

```bash
chmod -R 775 storage bootstrap/cache
chown -R www-data:www-data storage bootstrap/cache
```

### Error: CORS issues

1. Check `SANCTUM_STATEFUL_DOMAINS` in `.env`
2. Check `SESSION_DOMAIN` in `.env`
3. Verify frontend URL is in `config/cors.php`

---

## 📚 Related Documentation

- [Architecture](./ARCHITECTURE.md)
- [API Reference](./API_REFERENCE.md)
- [Database Schema](./DATABASE.md)
