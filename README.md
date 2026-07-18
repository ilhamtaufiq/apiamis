<div align="center">

<img src="public/logo.png" alt="APIAMIS" width="120" />

# APIAMIS

### API Manajemen Infrastruktur Sanitasi

Backend REST untuk ekosistem **Arumanis** (satu data air minum & sanitasi Kabupaten Cianjur). Sumber kebenaran data, otorisasi, media, dan integrasi untuk portal admin dan panel pengawas.

[![laravel](https://img.shields.io/badge/Laravel-13-ff2d20?style=for-the-badge&labelColor=111111&logo=laravel&logoColor=ff2d20)](https://laravel.com/)
[![php](https://img.shields.io/badge/PHP-8.2+-777BB4?style=for-the-badge&labelColor=111111&logo=php&logoColor=white)](https://www.php.net/)
[![swagger](https://img.shields.io/badge/OpenAPI-Swagger-85EA2D?style=for-the-badge&labelColor=111111&logo=swagger&logoColor=black)](#api--dokumentasi)
[![platform](https://img.shields.io/badge/platform-0.6.0-674bb5?style=for-the-badge&labelColor=111111)](https://github.com/ilhamtaufiq/arumanis)

<p>
  <a href="https://apiamis.cianjur.space/api/documentation"><strong>Swagger UI</strong></a>
  ·
  <a href="https://arumanis.cianjur.space"><strong>Portal Arumanis</strong></a>
  ·
  <a href="https://arumanis.cianjur.space/pengawasan"><strong>Panel pengawas</strong></a>
</p>

| Branch | Framework | Auth | Docs |
|:------:|:---------:|:----:|:----:|
| `main` | Laravel 13 | Sanctum + Spatie Permission | `/api/documentation` |

</div>

---

## Di mana posisi API ini?

```text
┌──────────────────────┐     ┌──────────────────────┐
│  Arumanis (portal)   │     │  Pengawasan (+mobile)│
│  React + Bun BFF     │     │  React + Bun / Expo  │
└──────────┬───────────┘     └──────────┬───────────┘
           │    REST + Sanctum token     │
           └──────────────┬──────────────┘
                          ▼
                 ┌─────────────────┐
                 │     APIAMIS     │  ← repo ini
                 │  Laravel REST   │
                 └────────┬────────┘
              ┌───────────┴───────────┐
              ▼                       ▼
           MySQL                    Redis
                                    (+ queue, cache)
```

Frontend **tidak** jadi sumber otorisasi final. Scope role, validasi bisnis, dan audit ditegakkan di sini.

| Repo | Peran |
|------|--------|
| [apiamis](https://github.com/ilhamtaufiq/apiamis) | Backend (ini) |
| [arumanis](https://github.com/ilhamtaufiq/arumanis) | Admin & operasional |
| [arumanis-pengawasan](https://github.com/ilhamtaufiq/arumanis-pengawasan) | Panel + app lapangan |

---

## Domain API

| Area | Cakupan |
|------|---------|
| **Proyek** | Pekerjaan, kegiatan, kontrak, output, penerima, penyedia, RKA, master fase |
| **Wilayah** | Kecamatan / desa, normalisasi nama |
| **Media** | Berkas & foto (MediaLibrary), geo-fence, ZIP unduhan, konversi LibreOffice |
| **Pengawas** | `user-pekerjaan`, endpoint KPI, progress, checklist, PUSPEN |
| **Analitik** | Dashboard, storage stats, RAB Analyzer (Python), export Excel/PDF |
| **Akses** | Sanctum, Google OAuth, Spatie roles, menu/route permission, impersonate |
| **Ops** | Audit log, client error report, notifikasi, blog/publikasi, WhatsApp bridge, AI (OpenRouter) |

Alur request:

```text
HTTP → routes/api.php → middleware (auth, role, throttle)
     → controller → service / model / scope
     → API Resource → JSON { status, data } (atau resource Laravel)
```

---

## Stack

| Lapisan | Teknologi |
|---------|-----------|
| App | Laravel 13 · PHP 8.2+ |
| Data | MySQL 8 (prod), SQLite (dev opsional) |
| Cache / queue | Redis, database queue |
| Authz | Sanctum · Socialite (Google) · Spatie Permission |
| Media | Spatie MediaLibrary |
| Docs | L5-Swagger (OpenAPI) |
| Export | Maatwebsite Excel · DomPDF · PHPWord |
| RAB | Python 3 (pandas, pdfplumber) |
| Test | PHPUnit |

---

## Mulai lokal

**Butuh:** PHP 8.2+ · Composer 2 · MySQL · Redis (disarankan) · Python 3 (RAB) · LibreOffice (konversi di Docker)

```text
C:\laragon\www\
  apiamis\    ← repo ini
  bun\
  pengawas\
```

```bash
git clone https://github.com/ilhamtaufiq/apiamis.git
cd apiamis
composer install
cp .env.example .env
php artisan key:generate
php artisan migrate --seed
php artisan serve
```

Atau:

```bash
composer run setup
```

API: **http://localhost:8000** atau **http://apiamis.test** (Laragon vhost).

---

## Konfigurasi (.env)

```env
APP_NAME=APIAMIS
APP_URL=http://apiamis.test
APP_DEBUG=true

DB_CONNECTION=mysql
DB_HOST=127.0.0.1
DB_DATABASE=apiamis
DB_USERNAME=root
DB_PASSWORD=

FRONTEND_URL=http://arumanis.test

GOOGLE_CLIENT_ID=
GOOGLE_CLIENT_SECRET=
GOOGLE_REDIRECT_URI="${APP_URL}/api/auth/google/callback"

REDIS_HOST=127.0.0.1
OPENROUTER_API_KEY=
```

| Variabel | Fungsi |
|----------|--------|
| `FRONTEND_URL` | CORS + redirect OAuth portal |
| `GOOGLE_*` | Login sosial |
| `OPENROUTER_API_KEY` | Fitur AI (opsional) |
| Reverb / queue | Realtime & job async (lihat `.env.example`) |

---

## API & Swagger

### Auth

| Method | Endpoint | Keterangan |
|--------|----------|------------|
| `POST` | `/api/auth/login` | Email/password → token |
| `POST` | `/api/auth/logout` | Butuh Bearer |
| `GET` | `/api/auth/me` | Profil sesi |
| `GET` | `/api/auth/google` | Mulai OAuth |

### Prefix resource (contoh)

| Prefix | Domain |
|--------|--------|
| `/api/pekerjaan` | Proyek / paket |
| `/api/kontrak` | Kontrak & addendum |
| `/api/kegiatan` | Program |
| `/api/berkas`, `/api/foto` | Dokumentasi |
| `/api/progress` | Progress fisik |
| `/api/pengawas`, `/api/user-pekerjaan` | Penugasan lapangan |
| `/api/dashboard` | Statistik |

Swagger UI:

```text
/api/documentation
```

Regenerate setelah ubah anotasi:

```bash
php artisan l5-swagger:generate
```

---

## Struktur

```text
app/
  Http/Controllers/    REST per domain
  Http/Middleware/
  Http/Resources/      Serializer response
  Models/
  Services/
  Notifications/
routes/api.php
database/migrations|seeders
storage/app/           Upload & media
```

---

## Deploy

```bash
docker build -t apiamis .
docker run -d -p 8000:8000 apiamis
```

Full stack dari repo frontend:

```bash
cd ../bun
docker compose up -d --build
docker compose --profile tools run --rm migrate
```

| Service | URL lokal tipikal |
|---------|-------------------|
| API | http://localhost:8000/api |
| Frontend | http://localhost:3000 / :5173 |
| MySQL (compose) | localhost:3307 |

Production checklist:

- `APP_DEBUG=false`, `APP_ENV=production`
- `php artisan config:cache` · `route:cache`
- Queue worker hidup jika ada job async
- CORS ketat ke origin portal + panel pengawas

Coolify / PaaS: set secret DB, `APP_KEY`, OAuth, storage volume, dan URL publik `https://apiamis.cianjur.space`.

---

## Kontrak dengan frontend

Ubah shape request/response, permission, atau relasi data:

1. Controller + Resource + policy/scope di **apiamis**
2. Sesuaikan [arumanis](https://github.com/ilhamtaufiq/arumanis) (`src/features/…`, `api-client`)
3. Sesuaikan [arumanis-pengawasan](https://github.com/ilhamtaufiq/arumanis-pengawasan) (+ mobile bila kena)
4. Update Swagger · seed permission bila perlu

Versi platform diselaraskan lewat `platform.version.json` di monorepo frontend (saat ini **0.6.0**).

---

## Lisensi

Ikuti lisensi proyek di repositori (Laravel skeleton + kode domain Arumanis).

<div align="center">

<br />

<img src="public/logo.png" alt="" width="48" />

<sub>API · Air Minum &amp; Sanitasi Kabupaten Cianjur</sub>

</div>
