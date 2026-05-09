# APIAMIS

**Backend REST API for ARUMANIS (Aplikasi Satu Data Air Minum dan Sanitasi)**

## 📋 Overview

APIAMIS is a Laravel-based REST API designed to power the ARUMANIS frontend system. It manages data for infrastructure projects, including activities, jobs, contracts, and documentation for the "Bidang Air Minum dan Sanitasi".

## 🛠️ Tech Stack

- **Framework**: Laravel 12
- **PHP**: ^8.2
- **Auth**: Laravel Sanctum & Spatie Permission
- **Media**: Spatie MediaLibrary
- **PDF Export**: LibreOffice Headless (Docker fix)
- **RAB Script**: Python 3 (pandas, pdfplumber)
- **DB**: MySQL/PostgreSQL

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

- **RAB Analyzer**: Ekstrak item pekerjaan otomatis dari Excel/PDF proyek MCK via Python.
- **Konversi PDF**: Render dokumen ke PDF pake LibreOffice headless di server (via Docker).
- **Audit Log**: Catat otomatis setiap ada data yang ditambah, diubah, atau dihapus.
- **Validasi Koordinat**: Cek foto di lapangan apakah masuk dalam area proyek (Geo-fencing).
- **Monitoring Storage**: Cek sisa kuota storage buat foto, file, dan database secara real-time.
- **Manajemen Media**: Upload dan simpan dokumen lewat Spatie MediaLibrary.
- **Normalisasi Wilayah**: Sinkronisasi otomatis nama Desa/Kecamatan biar gak ada beda sama GeoJSON map.

## 📝 License

This project is licensed under the [MIT License](LICENSE).
