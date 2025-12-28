# APIAMIS

**Backend REST API for ARUMANIS (Aplikasi Satu Data Air Minum dan Sanitasi)**

## 📋 Overview

APIAMIS is a Laravel-based REST API designed to power the ARUMANIS frontend system. It manages data for infrastructure projects, including activities, jobs, contracts, and documentation for the "Bidang Air Minum dan Sanitasi".

## 🛠️ Tech Stack

- **Framework**: Laravel 12
- **PHP**: ^8.2
- **Authentication**: Laravel Sanctum
- **Authorization**: Spatie Laravel Permission
- **Media**: Spatie Laravel MediaLibrary (for file and photo uploads)
- **API Documentation**: L5-Swagger
- **Database**: MySQL/PostgreSQL

## 🚀 Getting Started

### Prerequisites

- PHP 8.2+
- Composer
- MySQL or PostgreSQL
- Laragon/XAMPP (recommended for local development)

### Installation

```bash
# Clone the repository
git clone <repository-url>
cd apiamis

# Install dependencies
composer install

# Environment setup
copy .env.example .env
php artisan key:generate
```

### Database Setup

```bash
# Run migrations and seeders
php artisan migrate --seed
```

### Development

```bash
# Start development server
php artisan serve
```

The API will be available at `http://localhost:8000` or `http://apiamis.test` if using a local domain.

## 📡 API Endpoints

- **Auth**: `/api/auth/login`, `/api/auth/me`
- **Resources**: `/api/pekerjaan`, `/api/kontrak`, `/api/kegiatan`, `/api/kecamatan`, `/api/desa`
- **Map Optimized API**: `/api/foto?latest_only=1` (returns only the most recent photo per job)
- **Documentation**: `/api/documentation` (Swagger UI)

## 📁 Features

- **Audit Logging System**: Automatic event capturing with a specialized Trait for tracking all data mutations (`Auditable`).
- **Geo-Fencing Support**: Server-side infrastructure for validating photo coordinates against administrative boundaries.
- **Media Management**: Automatic handling of photo uploads and document storage via Spatie MediaLibrary.
- **Region Normalization**: Automated name normalization for Kecamatan and Desa to ensure Map-GeoJSON consistency.
- **Reporting**: Structured data for progress tracking and PDF/Excel exports.

## 📝 License

This project is licensed under the [MIT License](LICENSE).
