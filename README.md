# APIAMIS

### API Manajemen Infrastruktur Sanitasi

Backend REST untuk ekosistem **Arumanis** (satu data air minum & sanitasi Kabupaten Cianjur). Sumber kebenaran data, otorisasi, media, dan integrasi untuk portal admin dan panel pengawas.

Backend ini sepenuhnya ditulis dengan **Rust** (Axum + SQLx + MySQL). Sumber Laravel dan PHP sudah dihapus dari repo.

| Branch | Bahasa | Framework | Database | Docs |
|:------:|:------:|:---------:|:--------:|:----:|
| `main` | Rust 2021 | Axum 0.8 | MySQL 8 (SQLx) | `/api` |

---

## Di mana posisi API ini?

```text
┌──────────────────────┐     ┌──────────────────────┐
│  Arumanis (portal)   │     │  Pengawasan (+mobile)│
│  React + Bun BFF     │     │  React + Bun / Expo  │
└──────────┬───────────┘     └──────────┬───────────┘
           │    REST + token / cookie    │
           └──────────────┬──────────────┘
                          ▼
                 ┌─────────────────┐
                 │     APIAMIS     │  ← repo ini
                 │   Rust (Axum)   │
                 └────────┬────────┘
                          ▼
                        MySQL
```

Frontend **tidak** jadi sumber otorisasi final. Scope role, validasi bisnis, dan audit ditegakkan di sini.

| Repo | Peran |
|------|--------|
| [apiamis](https://github.com/ilhamtaufiq/apiamis) | Backend Rust (ini) |
| [arumanis](https://github.com/ilhamtaufiq/arumanis) | Admin & operasional |
| [arumanis-pengawasan](https://github.com/ilhamtaufiq/arumanis-pengawasan) | Panel + app lapangan |

---

## Domain API

| Area | Cakupan |
|------|---------|
| **Proyek** | Pekerjaan, kegiatan, kontrak, output, penerima, penyedia, master fase |
| **Wilayah** | Kecamatan / desa, GeoJSON desa |
| **Dokumen** | Template kontrak, ekspor PPTX/Excel, ONLYOFFICE, berkas dan foto |
| **Sanitasi & SPAM** | SPM sanitasi, unit SPAM, capaian, survei lokasi |
| **Pengawas** | Penugasan, progress, checklist |
| **Analitik** | Dashboard, pencarian global, analisa tabel (admin) |
| **Akses** | Sesi berbasis token dan cookie, Google OAuth, hak akses menu dan route |
| **Ops** | Audit log, notifikasi, WhatsApp, SPSE, backup, asisten AI |

---

## Struktur

```text
rust/
  Cargo.toml              workspace
  migrations/             SQL schema (0000 baseline dan seterusnya)
  crates/
    api/                  binary server Axum: router, handler, migrator, fitur
    auth/                 login dan sesi
    shared/               konfigurasi dan tipe error API
docker/                   konfigurasi Apache (meneruskan ke binary Rust)
resources/geojson/        GeoJSON desa, dibaca saat runtime
storage/app/templates/    template dokumen kontrak, dibaca saat runtime
Dockerfile                build multi-stage: binary Rust lalu image Apache
docker-entrypoint.sh      migrasi, lalu menjalankan API
```

Migrasi dijalankan oleh binary sendiri lewat `apiamis-api migrate`. Daftarnya ada di `rust/crates/api/src/migrate.rs`.

---

## Mulai lokal

**Butuh:** Rust stable · MySQL 8 · `cmake` dan `clang` (dipakai beberapa dependency)

```bash
cd rust
cp ../.env.rust.example .env      # isi nilai yang bertanda <...>
cargo run -p api -- migrate        # jalankan migrasi
cargo run -p api                   # API di http://localhost:8000
```

Tes:

```bash
cd rust
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Catatan: di mesin dengan disk terbatas, build bisa gagal karena kehabisan ruang. Bersihkan `rust/target/debug/incremental` atau jalankan `cargo clean -p api`.

---

## Konfigurasi

Salin `.env.rust.example` menjadi `.env` untuk lokal, atau isi sebagai environment variable di Coolify untuk production. Jangan commit file yang berisi nilai asli.

| Grup | Variabel | Keterangan |
|------|----------|------------|
| Aplikasi | `APP_PORT`, `APP_URL`, `APP_ENV`, `APP_NAME` | Port bawaan 8000. `APP_NAME` ikut membentuk prefix cache, jadi harus tetap sama. |
| Database | `DATABASE_URL`, `DB_MAX_CONNECTIONS`, `DB_IDLE_TIMEOUT_SECS` | Format `mysql://user:password@host:3306/db` |
| Kunci | `APP_KEY` | Dipakai membuka secret terenkripsi (kredensial SPSE, backup Google Drive). Harus sama dengan kunci lama. |
| Auth | `SANCTUM_TOKEN_EXPIRATION`, `SESSION_COOKIE_NAME`, `SESSION_COOKIE_SECURE`, `SESSION_COOKIE_DOMAIN` | Umur token dalam menit (bawaan 720). `SESSION_COOKIE_DOMAIN` opsional, untuk BFF di host lain. |
| Frontend | `FRONTEND_URL`, `PENGAWAS_APP_BASE_URL`, `MAINTENANCE_BYPASS_EMAILS` | CORS dan redirect |
| Google | `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `GOOGLE_REDIRECT_URI` | Login sosial, opsional |
| Berkas | `PUBLIC_STORAGE_PATH`, `PRIVATE_STORAGE_PATH`, `SPAM_IMPORT_DIR`, `KONTRAK_TEMPLATE_DIR`, `VILLAGE_GEOJSON_PATH` | Diarahkan ke volume di image |
| ONLYOFFICE | `ONLYOFFICE_DOCUMENT_SERVER_URL`, `ONLYOFFICE_JWT_SECRET`, dll. | Dokumen dan PDF, opsional |
| SPSE | `SPSE_BASE_URL`, `SPSE_LPSE_SLUG`, data PPK | Kontrak dan procurement |
| AI | `AI_BASE_URL`, `AI_API_KEY`, `AI_MODEL`, `AI_COMPACT_MODEL`, `AI_NATIVE_TOOLS` | Layanan AI kompatibel OpenAI. `AI_NATIVE_TOOLS=1` memakai function calling, tanpa itu protokol JSON. `AI_COMPACT_MODEL` opsional untuk ringkasan memori. |

---

## Deploy

```bash
docker build -t apiamis .
docker run -d -p 8000:8000 --env-file .env apiamis
```

Container menjalankan `docker-entrypoint.sh`: migrasi dulu, lalu API sebagai `www-data`. Bila migrasi gagal, API tidak dijalankan. Apache di depan meneruskan `/`, `/up`, dan `/api` ke binary Rust, serta melayani `/storage`.

Production checklist:

- `APP_ENV=production`, `SESSION_COOKIE_SECURE=true`
- Volume persisten untuk `storage/app/public` dan `storage/app/private`
- CORS hanya ke origin portal dan panel pengawas
- Migrasi baru (`0010`, `0011`) harus berjalan sebelum API dipakai

Coolify: set semua variabel dari `.env.rust.example`, pasang volume storage, dan arahkan domain ke `https://apiamis.cianjur.space`.

---

## Kontrak dengan frontend

Ubah shape request/response, permission, atau relasi data:

1. Handler, query, dan scope akses di **apiamis**
2. Sesuaikan [arumanis](https://github.com/ilhamtaufiq/arumanis) (`src/features/…`, `api-client`)
3. Sesuaikan [arumanis-pengawasan](https://github.com/ilhamtaufiq/arumanis-pengawasan) bila perlu
4. Tambahkan migrasi di `rust/migrations` dan daftarkan di `migrate.rs`

---

## Lisensi

Hak cipta ARUMANIS. Lisensi mengikuti repositori.

<div align="center">

<sub>API · Air Minum &amp; Sanitasi Kabupaten Cianjur</sub>

</div>
