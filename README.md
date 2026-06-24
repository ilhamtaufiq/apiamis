# APIAMIS

**API Manajemen Infrastruktur Sanitasi** — backend REST API untuk ekosistem **Arumanis** (Aplikasi Satu Data Air Minum dan Sanitasi).

APIAMIS menyediakan layanan data, otorisasi, media, dan integrasi untuk manajemen pekerjaan infrastruktur bidang air minum dan sanitasi. API ini menjadi sumber kebenaran (*single source of truth*) bagi aplikasi frontend dan panel pengawasan lapangan.

| | |
|---|---|
| **Framework** | Laravel 13 |
| **PHP** | ^8.2 |
| **Branch aktif** | `main` |
| **Dokumentasi API** | Swagger UI di `/api/documentation` |

---

## Daftar Isi

- [Gambaran Umum](#gambaran-umum)
- [Ekosistem Arumanis](#ekosistem-arumanis)
- [Fitur Utama](#fitur-utama)
- [Arsitektur](#arsitektur)
- [Tech Stack](#tech-stack)
- [Persiapan Lingkungan](#persiapan-lingkungan)
- [Instalasi & Pengembangan](#instalasi--pengembangan)
- [Konfigurasi](#konfigurasi)
- [API & Dokumentasi](#api--dokumentasi)
- [Struktur Proyek](#struktur-proyek)
- [Deployment](#deployment)
- [Repositori Terkait](#repositori-terkait)

---

## Gambaran Umum

APIAMIS menangani seluruh sisi server untuk platform Arumanis:

- Persistensi data pekerjaan, kontrak, dokumen, foto, dan progress
- Autentikasi token (Sanctum) dan otorisasi berbasis role (Spatie Permission)
- Upload, transformasi, dan penyimpanan media
- Validasi bisnis, audit trail, dan pelaporan error klien
- Endpoint khusus pengawas lapangan dan integrasi publik

Frontend **tidak** menyimpan logika bisnis final — semua validasi dan akses data ditegakkan di layer API ini.

---

## Ekosistem Arumanis

```text
┌──────────────────────┐     ┌──────────────────────┐
│      Arumanis        │     │  Arumanis Pengawasan │
│  (Admin & Operasional)│     │  (Panel Pengawas)    │
│   React + Vite       │     │   React + Bun BFF    │
└──────────┬───────────┘     └──────────┬───────────┘
           │         REST / Sanctum       │
           └──────────────┬───────────────┘
                          ▼
                 ┌─────────────────┐
                 │     APIAMIS     │  ← repo ini
                 │  Laravel REST   │
                 └────────┬────────┘
                          │
              ┌───────────┴───────────┐
              ▼                       ▼
        ┌──────────┐            ┌──────────┐
        │  MySQL   │            │  Redis   │
        └──────────┘            └──────────┘
```

| Repo | Peran |
|---|---|
| [apiamis](https://github.com/ilhamtaufiq/apiamis) | Backend REST API (repo ini) |
| [arumanis](https://github.com/ilhamtaufiq/arumanis) | Frontend administrasi & operasional |
| [arumanis-pengawasan](https://github.com/ilhamtaufiq/arumanis-pengawasan) | Panel pengawas lapangan |

---

## Fitur Utama

### Data & Manajemen Proyek
- CRUD pekerjaan, kegiatan, kontrak, output, penerima, dan penyedia
- Master wilayah (kecamatan, desa) dengan normalisasi nama
- Import pekerjaan dari Excel dan template unduhan
- RKA, draft pekerjaan, dan master fase pekerjaan

### Dokumentasi & Media
- Manajemen berkas dan foto proyek via Spatie MediaLibrary
- Validasi koordinat foto (geo-fencing) terhadap area pekerjaan
- Konversi dokumen ke PDF (LibreOffice headless di Docker)
- Download berkas terkumpul per pekerjaan

### Pengawasan & Progress
- Penugasan pengawas lapangan (`user-pekerjaan`)
- Endpoint pengawas dan statistik KPI
- Progress fisik pekerjaan dan checklist
- Integrasi PUSPEN (progress fisik, media share publik)

### Analisis & Laporan
- RAB Analyzer — ekstraksi item pekerjaan dari Excel/PDF via skrip Python
- Dashboard analytics dan statistik storage
- Export kontrak ke Excel
- Audit log otomatis untuk perubahan data

### Keamanan & Administrasi
- Laravel Sanctum (Bearer token) dan Google OAuth
- RBAC dengan Spatie Permission (role & permission middleware)
- Scope data per role pengguna pada query pekerjaan
- Menu permission, kegiatan-role, dan impersonasi admin
- Client error reporting dan data quality monitoring

### Integrasi
- Notifikasi in-app
- Blog/publikasi dan endpoint publik terbatas
- WhatsApp bridge (via frontend Arumanis)
- OpenRouter untuk fitur AI (opsional)

---

## Arsitektur

```text
HTTP Request
  → routes/api.php
  → Middleware (Sanctum, role, throttle)
  → Controller
  → Service / Model / Query scope
  → API Resource (serializer)
  → JSON Response
```

**Prinsip integrasi:**

- Response umumnya dibungkus `{ status, data }` atau Laravel API Resource
- Otorisasi ditegakkan di middleware dan query scope model — bukan hanya di UI
- Perubahan kontrak API harus diselaraskan dengan frontend Arumanis dan panel pengawasan

---

## Tech Stack

| Kategori | Teknologi |
|---|---|
| Framework | Laravel 13 |
| Bahasa | PHP 8.2+ |
| Database | MySQL (production), SQLite (development) |
| Cache / Queue | Redis, database queue |
| Auth | Laravel Sanctum, Laravel Socialite (Google) |
| Authorization | Spatie Laravel Permission |
| Media | Spatie MediaLibrary |
| Dokumentasi API | L5-Swagger (OpenAPI) |
| Export | Maatwebsite Excel, DomPDF, PHPWord |
| Analisis RAB | Python 3 (pandas, pdfplumber) |
| Testing | PHPUnit |

---

## Persiapan Lingkungan

| Kebutuhan | Versi |
|---|---|
| PHP | 8.2+ |
| Composer | 2.x |
| MySQL | 8.x (production) |
| Redis | 7.x (opsional, direkomendasikan) |
| Node.js / Bun | Untuk asset build frontend bawaan Laravel |
| Python 3 | Untuk fitur RAB Analyzer |
| LibreOffice | Untuk konversi dokumen di server (Docker) |

**Layout repositori lokal (disarankan):**

```text
C:\laragon\www\
  apiamis\    # backend — repo ini
  bun\        # frontend Arumanis
  pengawas\   # panel pengawasan
```

---

## Instalasi & Pengembangan

```bash
# Clone repository
git clone https://github.com/ilhamtaufiq/apiamis.git
cd apiamis

# Install dependensi PHP
composer install

# Salin dan konfigurasi environment
cp .env.example .env
php artisan key:generate

# Migrasi dan seed data awal
php artisan migrate --seed

# Jalankan development server
php artisan serve
```

API tersedia di **http://localhost:8000** atau **http://apiamis.test** (jika menggunakan virtual host Laragon).

Setup cepat via Composer script:

```bash
composer run setup
```

---

## Konfigurasi

Variabel environment penting di `.env`:

```env
APP_NAME=APIAMIS
APP_URL=http://apiamis.test
APP_DEBUG=true

DB_CONNECTION=mysql
DB_HOST=127.0.0.1
DB_PORT=3306
DB_DATABASE=apiamis
DB_USERNAME=root
DB_PASSWORD=

# URL frontend untuk CORS dan redirect OAuth
FRONTEND_URL=http://arumanis.test

# Google OAuth (opsional)
GOOGLE_CLIENT_ID=
GOOGLE_CLIENT_SECRET=
GOOGLE_REDIRECT_URI="${APP_URL}/api/auth/google/callback"

# Redis (opsional)
REDIS_HOST=127.0.0.1
REDIS_PORT=6379

# OpenRouter untuk fitur AI (opsional)
OPENROUTER_API_KEY=
```

| Variabel | Deskripsi |
|---|---|
| `FRONTEND_URL` | Origin frontend Arumanis untuk CORS dan callback |
| `GOOGLE_*` | Kredensial OAuth Google untuk login sosial |
| `OPENROUTER_API_KEY` | API key untuk fitur analisis berbasis AI |

---

## API & Dokumentasi

### Autentikasi

| Method | Endpoint | Deskripsi |
|---|---|---|
| `POST` | `/api/auth/login` | Login email/password, mengembalikan token |
| `POST` | `/api/auth/logout` | Logout (perlu Bearer token) |
| `GET` | `/api/auth/me` | Profil user yang sedang login |
| `GET` | `/api/auth/google` | Redirect ke Google OAuth |

### Resource utama

| Prefix | Deskripsi |
|---|---|
| `/api/pekerjaan` | Manajemen pekerjaan/proyek |
| `/api/kontrak` | Data kontrak dan addendum |
| `/api/kegiatan` | Program/kegiatan |
| `/api/berkas`, `/api/foto` | Dokumentasi proyek |
| `/api/progress` | Progress fisik pekerjaan |
| `/api/pengawas` | Data pengawas |
| `/api/user-pekerjaan` | Penugasan pengawas lapangan (admin) |
| `/api/dashboard` | Statistik dashboard |

### Dokumentasi interaktif

Swagger UI tersedia di:

```text
/api/documentation
```

Regenerasi dokumentasi setelah perubahan anotasi controller:

```bash
php artisan l5-swagger:generate
```

---

## Struktur Proyek

```text
app/
├── Http/
│   ├── Controllers/     # REST controllers per domain
│   ├── Middleware/      # Auth, role, custom middleware
│   └── Resources/       # API Resource serializers
├── Models/              # Eloquent models & relasi
├── Services/            # Business logic terpisah
├── Notifications/       # Notifikasi in-app
└── Traits/              # Auditable, shared behavior

routes/
└── api.php              # Definisi seluruh endpoint REST

database/
├── migrations/          # Skema database
└── seeders/             # Data awal

storage/
└── app/                 # File upload & media
```

---

## Deployment

### Docker

```bash
docker build -t apiamis .
docker run -d -p 8000:8000 apiamis
```

### Docker Compose (full stack)

Dari repo frontend [arumanis](https://github.com/ilhamtaufiq/arumanis), jalankan stack lengkap yang mencakup backend ini:

```bash
cd ../bun
docker compose up -d --build
docker compose --profile tools run --rm migrate
```

| Service | URL default |
|---|---|
| Backend API | http://localhost:8000/api |
| Frontend | http://localhost:3000 |
| MySQL | localhost:3307 |

### Production

- Set `APP_DEBUG=false` dan `APP_ENV=production`
- Jalankan `php artisan config:cache` dan `php artisan route:cache`
- Pastikan queue worker aktif jika menggunakan job async
- Konfigurasi CORS agar hanya mengizinkan origin frontend yang valid

---

## Repositori Terkait

| Repo | Peran |
|---|---|
| [apiamis](https://github.com/ilhamtaufiq/apiamis) | Backend REST API (repo ini) |
| [arumanis](https://github.com/ilhamtaufiq/arumanis) | Frontend administrasi |
| [arumanis-pengawasan](https://github.com/ilhamtaufiq/arumanis-pengawasan) | Panel pengawas lapangan |

Perubahan endpoint, permission, atau bentuk response harus diverifikasi di kedua aplikasi frontend sebelum dirilis.