# Rencana menghapus Laravel

Dokumen ini menjelaskan urutan kerja untuk menghapus Laravel dari apiamis, sehingga API berjalan sepenuhnya di Rust. Angka diambil dari kode pada commit `467a77f` (2026-10-09).

## Kondisi sekarang

- Route API: 487 route (termasuk expand `apiResource`). Di `routes.md`: `rust` 404, `parsial` 3, `dihapus` 79, dan `belum` 0.
- Kode Laravel: 63 controller, 34 service, 7 command artisan, 133 migrasi. Tidak ada job dan event, dan tidak ada jadwal di `routes/console.php`.
- Dependensi Laravel yang masih dipakai, dan pengganti Rust-nya:

| Dependensi Laravel | Dipakai untuk | Pengganti di Rust | Status |
|---|---|---|---|
| `laravel/sanctum` | token login dan autentikasi | `auth` crate, `login.rs` | ada, tapi belum ada kedaluwarsa 720 menit |
| `spatie/laravel-permission` | role dan permission | `auth` crate, `permission.rs`, `route_permission.rs` | ada, sebagian |
| `spatie/laravel-medialibrary` | file dan foto | `media.rs` | ada |
| `maatwebsite/excel` | ekspor dan impor Excel | `rust_xlsxwriter`, `calamine` | ada |
| `dompdf/dompdf` | PDF checklist | `pekerjaan_checklist_pdf.rs` (tanpa crate) | ada untuk checklist |
| `phpoffice/phpword` | dokumen kontrak | `kontrak_document.rs` | perlu dicek paritasnya |
| `laravel/socialite` | Google login | `auth_oauth.rs` | ada |
| `league/flysystem-aws-s3-v3` | backup ke S3 | `app_settings_backup.rs` (SigV4) | ada |
| `laravel/reverb` | websocket (2 file) | belum ada | perlu keputusan |
| `darkaonline/l5-swagger` | dokumentasi API | belum ada | perlu keputusan |
| migrasi Laravel (133 file) | schema database | belum ada (lihat Fase 3) | belum |
| tabel `cache`, `sessions` | cache dan sesi | `cache_put` / `cache_pull` | sebagian |
| command artisan (7) | backup, impor, dan lainnya | belum | perlu dipindah |

## Fase 1: selesaikan pemindahan route

Target: `routes.md` tidak lagi berisi `belum`, atau sisanya dicatat sebagai "tetap di Laravel" dengan alasan.

- Route `belum` sudah 0 (lihat `routes.md`). Yang tersisa: 3 `parsial` (`GET /api/pekerjaan/document-register`, `GET /api/document-registers`, dan `POST /api/procurement/spse/kontrak/push`), serta verifikasi paritas runtime terhadap Laravel yang berjalan. Inventaris `routes.md` dibuat secara statis, jadi perlu dicocokkan dengan `php artisan route:list --json`.
- Route yang sengaja tidak dipindah diputuskan satu per satu: procurement kontrak/push (WIP, sudah lolos tes stub), desa profile, dan document-register (parsial).
- Setiap route selesai: tes DB lulus, aturan Apache ditambahkan, `routes.md` dan `log.md` diperbarui.

## Fase 2: pindahkan autentikasi dan permission

Ini prasyarat paling penting. Selama Laravel masih mengurus token dan role, Laravel tidak bisa dihapus.

- Token: tambahkan pengecekan `created_at` terhadap 720 menit di `auth::authenticate`, mengikuti Sanctum. Ini juga menutup celah token impersonate yang tidak kedaluwarsa.
- Permission: pastikan semua role dan permission Spatie (tabel `roles`, `permissions`, `model_has_roles`, `role_has_permissions`) bisa dibaca dan ditulis dari Rust, termasuk pembersihan cache Spatie.
- Verifikasi: bandingkan respons login, `auth/me`, dan route berbasis role antara Laravel dan Rust.

## Fase 3: pindahkan schema ke Rust

- Ambil schema production dengan `mysqldump --no-data`, lalu jadikan itu file baseline SQL.
- Pakai `sqlx migrate` di crate `api`. Di database production yang sudah ada, baseline ditandai sudah diterapkan, jadi tidak dijalankan ulang.
- Migrasi baru ditulis dalam SQL dan dijalankan Rust saat start. Tabel `migrations` milik Laravel dibiarkan sampai Laravel dihapus.
- `rust/fixtures/` digabung menjadi satu sumber dengan baseline, supaya tidak ada dua versi schema.

## Fase 4: operasional tanpa PHP

- `Dockerfile`: hapus stage PHP, jalankan binary Rust sebagai proses utama. Apache hanya sebagai reverse proxy ke Rust, dan frontend tetap dilayani dari folder statis.
- `docker-entrypoint.sh`: hapus panggilan `artisan`, termasuk `storage:link`, `config:cache`, dan Reverb.
- Command artisan (7): pindahkan ke subcommand Rust atau task dalam proses.
- Websocket Reverb: putuskan apakah dipindah atau dihapus (lihat keputusan di bawah).
- Dokumentasi API (`l5-swagger`): putuskan apakah tetap dibuat dengan alat lain atau dihapus.

## Fase 5: uji paralel lalu cutover

- Jalankan Laravel dan Rust bersamaan di staging, dengan Apache mengarahkan route per route. Bandingkan respons kedua sisi untuk route yang read-only dulu.
- Cutover per kelompok, bukan sekaligus. Rollback cukup dengan mengembalikan aturan Apache ke Laravel, selama Laravel masih ada.
- Setelah semua kelompok stabil di production, baru Laravel dimatikan.

## Fase 6: hapus Laravel

- Hapus `app/`, `routes/` (kecuali yang masih dipakai), `artisan`, `composer.json`, `composer.lock`, `config/` yang Laravel-only, `bootstrap/`, `vendor/`.
- Hapus dependensi PHP dari `Dockerfile` dan `.dockerignore`.
- Perbarui `CLAUDE.md`, `README.md`, dan `AGENTS.md` yang masih menyebut Laravel.

## Keputusan yang sudah diambil

- Websocket Reverb: dihapus total (config, channels, Broadcast::routes, proxy Apache/Docker, dan blok Reverb di entrypoint).
- Dokumentasi API `l5-swagger`: dihapus (paket dan config). Anotasi `@OA` di controller tinggal sebagai komentar sampai `app/` dihapus di fase 6.
- Desa profile: dihapus (route Laravel dan Rust). Modul `raw_model` dipertahankan karena dipakai `spam_units` dan `spam_integration`.
- Procurement kontrak/push: tetap stub. Tes ke SPSE sandbox dilakukan user sendiri.
- Baseline schema: `rust/migrations/0000_baseline.sql` dibuat dari backup production (struktur 110 tabel dan 139 baris `migrations`, tanpa data).

## Keputusan yang perlu Anda ambil

1. **Token kedaluwarsa:** setuju pakai 720 menit seperti Laravel sekarang?
2. **Usulan-kegiatan export:** dibatasi scope seperti `index`, atau dibiarkan sama dengan Laravel?
3. **Procurement kontrak/push:** lanjut dipindah, atau tetap di Laravel?
4. **Desa profile:** dipulihkan seperti sekarang, atau dihapus?
5. **Websocket Reverb:** dipindah ke Rust, diganti mekanisme lain, atau dihapus?
6. **Dokumentasi API:** dibuat ulang, atau cukup `routes.md` dan dokumen ini?
7. **Blog feature:** tetap `is_featured = 1` seperti sekarang (sudah diputuskan).

## Risiko

- Perilaku yang belum diverifikasi terhadap Laravel yang berjalan (tidak ada `vendor/` di mesin ini). Contoh: format tanggal JSON, urutan tanpa `ORDER BY`, dan pesan validasi.
- Tes DB hanya berjalan di MariaDB lokal, belum di production.
- Satu tes handoff pernah gagal sekali saat paralel, penyebabnya belum diketahui.
- Impor spam-units menghapus semua anggaran sebelum mengisi ulang, jadi harus dites dengan data yang benar sebelum dipakai.
