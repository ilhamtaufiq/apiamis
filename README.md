<div align="center">

<img src="docs/assets/logo-arumanis.png" alt="Arumanis" width="120" />

# APIAMIS

### API Manajemen Infrastruktur Sanitasi

Backend REST untuk ekosistem **Arumanis**, satu data air minum dan sanitasi Kabupaten Cianjur.

[![rust](https://img.shields.io/badge/Rust-2021-b7410e?style=for-the-badge&labelColor=111111&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![axum](https://img.shields.io/badge/Axum-0.8-674bb5?style=for-the-badge&labelColor=111111)](https://github.com/tokio-rs/axum)
[![mysql](https://img.shields.io/badge/MySQL-8-4479a1?style=for-the-badge&labelColor=111111&logo=mysql&logoColor=white)](https://www.mysql.com/)
[![license](https://img.shields.io/badge/license-MIT-22c55e?style=for-the-badge&labelColor=111111)](LICENSE)

</div>

---

## Ringkasan

APIAMIS menjadi sumber kebenaran untuk data, otorisasi, berkas, dan integrasi Arumanis. Seluruh backend ditulis dengan Rust (Axum, SQLx, MySQL). Sumber Laravel dan PHP sudah dihapus dari repo ini.

Frontend tidak menjadi sumber otorisasi final. Pembatasan peran, validasi bisnis, dan audit ditegakkan di API ini.

## Posisi dalam ekosistem

```text
┌──────────────────────┐     ┌──────────────────────┐
│  Arumanis (portal)   │     │  Pengawasan (+mobile)│
│  React + Bun BFF     │     │  React + Bun / Expo  │
└──────────┬───────────┘     └──────────┬───────────┘
           │      REST + token / cookie │
           └──────────────┬─────────────┘
                          ▼
                 ┌─────────────────┐
                 │     APIAMIS     │  ← repo ini
                 │   Rust (Axum)   │
                 └────────┬────────┘
                          ▼
                        MySQL
```

| Repo | Peran |
|------|-------|
| [apiamis](https://github.com/ilhamtaufiq/apiamis) | Backend Rust (repo ini) |
| [arumanis](https://github.com/ilhamtaufiq/arumanis) | Portal admin dan operasional |
| [arumanis-pengawasan](https://github.com/ilhamtaufiq/arumanis-pengawasan) | Panel dan aplikasi lapangan |

## Cakupan API

| Area | Isi |
|------|-----|
| **Proyek** | Pekerjaan, kegiatan, kontrak, output, penerima, penyedia, master fase |
| **Wilayah** | Kecamatan dan desa, GeoJSON desa |
| **Dokumen** | Template kontrak, ekspor PPTX dan Excel, ONLYOFFICE, berkas, foto, NPHD dan BA serah terima |
| **Sanitasi dan SPAM** | SPM sanitasi, unit SPAM, capaian, survei lokasi |
| **Pengawasan** | Penugasan, progres, checklist |
| **Analitik** | Dashboard, pencarian global, analisa tabel (admin) |
| **Akses** | Sesi berbasis token dan cookie, Google OAuth, hak akses menu dan route |
| **Operasional** | Audit log, notifikasi, WhatsApp, SPSE, backup |

## Struktur proyek

```text
rust/
  Cargo.toml              workspace
  migrations/             skema SQL (baseline 0000 dan seterusnya)
  crates/
    api/                  binary server Axum: router, handler, migrator, fitur
    auth/                 login dan sesi
    shared/               konfigurasi dan tipe error API
docker/                   konfigurasi Apache (meneruskan ke binary Rust)
resources/geojson/        GeoJSON desa, dibaca saat runtime
storage/app/templates/    template dokumen, dibaca saat runtime
Dockerfile                build multi-stage: binary Rust, lalu image Apache
docker-entrypoint.sh      migrasi, lalu menjalankan API
```

Migrasi dijalankan oleh binary itu sendiri dengan `apiamis-api migrate`. Daftar migrasi ada di `rust/crates/api/src/migrate.rs`.

## Memulai secara lokal

**Prasyarat:** Rust stable, MySQL 8, `cmake`, dan `clang` (dipakai beberapa dependensi).

```bash
cd rust
cp ../.env.rust.example .env      # isi nilai yang bertanda <...>
cargo run -p api -- migrate       # jalankan migrasi
cargo run -p api                  # API di http://localhost:8000
```

### Pengujian dan pemeriksaan kode

```bash
cd rust
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

> Di mesin dengan disk terbatas, build bisa gagal karena kehabisan ruang. Bersihkan `rust/target/debug/incremental` atau jalankan `cargo clean -p api`.

## Konfigurasi

Salin `.env.rust.example` menjadi `.env` untuk lokal. Untuk production, isi variabel yang sama sebagai environment variable di Coolify. Jangan commit berkas yang berisi nilai asli.

| Grup | Variabel | Keterangan |
|------|----------|------------|
| Aplikasi | `APP_PORT`, `APP_URL`, `APP_ENV`, `APP_NAME` | Port bawaan 8000. `APP_NAME` ikut membentuk prefix cache, jadi harus tetap sama. |
| Database | `DATABASE_URL`, `DB_MAX_CONNECTIONS`, `DB_IDLE_TIMEOUT_SECS` | Format `mysql://user:password@host:3306/db` |
| Kunci | `APP_KEY` | Membuka secret terenkripsi (kredensial SPSE, backup Google Drive). Harus sama dengan kunci lama. |
| Autentikasi | `SANCTUM_TOKEN_EXPIRATION`, `SESSION_COOKIE_NAME`, `SESSION_COOKIE_SECURE`, `SESSION_COOKIE_DOMAIN` | Umur token dalam menit (bawaan 720). `SESSION_COOKIE_DOMAIN` opsional, untuk BFF di host lain. |
| Frontend | `FRONTEND_URL`, `PENGAWAS_APP_BASE_URL`, `MAINTENANCE_BYPASS_EMAILS` | CORS dan redirect |
| Google | `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `GOOGLE_REDIRECT_URI` | Login sosial (opsional) |
| Berkas | `PUBLIC_STORAGE_PATH`, `PRIVATE_STORAGE_PATH`, `SPAM_IMPORT_DIR`, `KONTRAK_TEMPLATE_DIR`, `VILLAGE_GEOJSON_PATH` | Diarahkan ke volume di image |
| ONLYOFFICE | `ONLYOFFICE_DOCUMENT_SERVER_URL`, `ONLYOFFICE_JWT_SECRET`, dll. | Dokumen dan PDF (opsional) |
| SPSE | `SPSE_BASE_URL`, `SPSE_LPSE_SLUG`, data PPK | Kontrak dan pengadaan |
| AI | `AI_BASE_URL`, `AI_API_KEY`, `AI_MODEL`, `AI_COMPACT_MODEL`, `AI_NATIVE_TOOLS` | Layanan AI kompatibel OpenAI. `AI_NATIVE_TOOLS=1` memakai function calling. `AI_COMPACT_MODEL` opsional untuk ringkasan memori. |

## Deploy

```bash
docker build -t apiamis .
docker run -d -p 8000:8000 --env-file .env apiamis
```

Container menjalankan `docker-entrypoint.sh`: migrasi dulu, lalu API sebagai `www-data`. Jika migrasi gagal, API tidak dijalankan. Apache di depan meneruskan `/`, `/up`, dan `/api` ke binary Rust, serta melayani `/storage`.

**Checklist production**

- `APP_ENV=production` dan `SESSION_COOKIE_SECURE=true`
- Volume persisten untuk `storage/app/public` dan `storage/app/private`
- CORS hanya ke origin portal dan panel pengawasan
- Migrasi baru (`0010`, `0011`) harus berjalan sebelum API dipakai

Di Coolify, isi semua variabel dari `.env.rust.example`, pasang volume storage, dan arahkan domain ke `https://apiamis.cianjur.space`.

## Mengubah kontrak API

Jika mengubah bentuk request atau response, permission, atau relasi data, urutannya:

1. Ubah handler, query, dan scope akses di **apiamis**.
2. Sesuaikan pemanggil di [arumanis](https://github.com/ilhamtaufiq/arumanis) (`src/features/…`, `api-client`).
3. Sesuaikan [arumanis-pengawasan](https://github.com/ilhamtaufiq/arumanis-pengawasan) bila perlu.
4. Tambahkan migrasi di `rust/migrations` dan daftarkan di `migrate.rs`.

## Lisensi dan atribusi

Kode APIAMIS berlisensi [MIT](LICENSE) · Copyright (c) 2025 ARUMANIS.

APIAMIS dibangun di atas crate open source pihak ketiga. Daftar dependensi langsung beserta versi dan lisensinya ada di [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Terima kasih kepada para pengembang dan komunitas Rust yang membuatnya.
