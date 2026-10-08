# Log Migrasi Backend ke Rust (Axum)

Branch kerja: `rust`. Rencana lengkap ada di dokumen rencana migrasi.

Status: `[x]` selesai, `[~]` sedang dikerjakan, `[ ]` belum dimulai.

Setiap kali ada langkah yang selesai, tambahkan entri di bagian **Riwayat** dan ubah status di **Checklist**.

---

## Checklist

### Fase 0: Spesifikasi dan fixture (2–4 minggu)

- [x] 0.1 Inventaris route dari `routes/api.php` dan `routes/web.php` ke `docs/migration/routes.md`
- [x] 0.2 Inventaris model, observer, cast, listener, event, dan service ke `docs/migration/models.md`
- [x] 0.3 Inventaris data (tabel, kolom terenkripsi, JSON, decimal, soft delete) ke `docs/schema.md`, dari dump struktur MySQL 8.0.30 (111 tabel)
- [~] 0.4 Fixture perilaku untuk endpoint prioritas. Di `rust/fixtures/live/` ada: `up`, daftar `kecamatan`, `desa`, `kegiatan`, dan `pekerjaan` (200 dengan token uji, data pribadi dihapus), serta 401 tanpa token. Belum ada: show per id, `pekerjaan` dengan filter, dan respon 422 dari POST
- [~] 0.5 Dump database staging sebagai data uji tetap. Ada dump data `tbl_kegiatan` (20 baris) dan `tbl_pekerjaan` (558 baris), belum dump lengkap semua tabel bisnis
- [ ] 0.6 Konfirmasi keputusan terbuka (lihat bagian Keputusan)

### Fase 1: Fondasi dan autentikasi (3–4 minggu)

- [x] 1.0 Workspace Cargo di `rust/` dengan crate `api` dan `shared`, `GET /up`, `GET /api/health`, fallback 404 ala Laravel
- [x] 1.1 Konfigurasi dari `.env`, format error 401 dan 422 yang sama dengan Laravel
- [x] 1.2 Middleware: request id, timeout, body limit, CORS (CORS disalin dari `config/cors.php`, termasuk pola `*.pages.dev`)
- [~] 1.3 Dockerfile multi-stage yang hanya membangun binary Rust (ditulis, belum di-build: Docker daemon tidak aktif di sandbox)
- [~] 1.4 CI: `fmt`, `clippy`, `test`, dan job integrasi MySQL (`.github/workflows/rust.yml`). Workflow belum pernah dijalankan di GitHub
- [x] 1.5 Validasi token Sanctum di `crates/auth`: lookup ke `personal_access_tokens`, cek tipe user, hash sha256, kedaluwarsa, dan user pemilik. Diuji 7 test integrasi terhadap data dump asli (MariaDB lokal). Belum diuji end-to-end dengan plain token asli, karena dump hanya menyimpan hash
- [~] 1.6 Login email/password (bcrypt) selesai: `POST /api/auth/login` dengan validasi 422, rate limit 5/menit per IP dan email, token Sanctum, respon `user` dan `token`. Belum: Google OAuth (`/api/auth/google`, callback, dan handoff)
- [x] 1.7 Permission Spatie dan middleware route permission (`crates/auth/src/permission.rs`, `crates/api/src/route_permission.rs`). Matriks keputusan 13 kasus lolos, tes DB terhadap tabel `roles`, `model_has_roles`, dan `route_permissions` lolos, smoke test server cocok dengan 403 Laravel. Maintenance gate sudah ada (lihat 1.9)
- [ ] 1.8 Impersonation dengan penanda token dan audit log
- [x] 1.9 Maintenance gate (`EnsureNotInMaintenance`): flag `app_settings`, bypass email (setting, env, default), path yang dikecualikan. Diuji: user bypass 200, user lain 503 dengan body sama seperti Laravel, tanpa token 503, `/up` 200

### Fase 2: Modul bisnis (4–7 bulan)

- [ ] 2.1 Public API v1 (`/api/public/v1/*`)
- [~] 2.2 Master data: `kecamatan`, `desa`, dan `kegiatan` sudah di Rust untuk GET list dan show per id. Diuji terhadap fixture produksi dan DB. Belum ada POST, PUT, atau DELETE
- [~] 2.3 Lookup dan konfigurasi: `app-settings` (index publik dan maintenance), `tags` (index dan show), `document-types`, dan `penyedia` (index dengan search dan paginasi, show). Belum: `app-settings` admin (mail, kontrak-templates, backups, storage-stats) karena menyentuh SMTP, filesystem, dan jadwal backup. `master-fase-pekerjaan` tidak dimigrasi (lihat K7)
- [~] 2.4 Tiket dan SpamUnit: Tiket GET (`/api/tiket`, `/api/tiket/{id}`) sudah dipindah (lihat T26). Belum: tulis Tiket (store, update, destroy, bulk-update, komentar). SpamUnit belum dikerjakan sama sekali: respon-nya model Eloquent mentah, dan skema di `docs/schema.md` sudah tidak sesuai migrasi (T27). Perlu dump struktur terbaru sebelum bisa dibuat dengan benar
- [~] 2.5 Checklist proyek: GET `/api/checklist-items` (+ `{id}`), `/api/pekerjaan-checklist` (index dan `history`), dan `/api/post-pekerjaan-checklist` sudah dipindah, dengan gate role yang sama seperti Pekerjaan. Belum: `pekerjaan-checklist/toggle`, ekspor excel dan pdf, dan tulis `checklist-items`
- [~] 2.6 Pekerjaan (GET list selesai, show belum): hanya GET (`/api/pekerjaan`, `/api/pekerjaan/{id}`), dibatasi role `admin`, `manager`, `super-admin`, dan `operator` (fail-closed: role lain 403). Sudah dipindah: progres (`progress_total`, `deviasi`), estimasi saat `summary=1`, hitungan, `kontrak` dengan penyedia dan addendum (keduanya hanya pada daftar terpaginasi, sesuai `per_page=-1`), `registers` dengan `type` saat `summary=1`, `assignment_sources`, pencarian lewat penyedia kontrak, sort `penerima_count`, `output` dan `foto_required_count`/`foto_status` cabang lengkap saat `summary=1` pada daftar terpaginasi. `draft` tidak dimuat di daftar (Laravel memakai `whenLoaded`, jadi key dihilangkan). Belum dipindah: show. `PekerjaanController@show` memakai `PekerjaanDetailResource` (foto, berkas, penerima, progress, kontrak dengan addendum kosong), bukan `PekerjaanResource`. Respon show saat ini masih bentuk daftar dan diberi header `x-partial-response`. Frontend jangan dipindah ke endpoint ini sebelum show selesai (T25)
- [ ] 2.7 Progress dan foto
- [ ] 2.8 SimulationNetwork (versioning JSON)
- [ ] 2.9 Kontrak dan Berita Acara
- [ ] 2.10 Import/export Excel
- [ ] 2.11 OnlyOffice callback
- [ ] 2.12 AI chat (MiniMax)
- [ ] 2.13 Notifikasi real-time

### Fase 3: Cutover

- [ ] 3.1 Paritas: semua fixture lulus
- [ ] 3.2 Staging penuh dengan data salinan produksi
- [ ] 3.3 Shadow traffic (opsional)
- [ ] 3.4 Backup database dan jendela cutover
- [ ] 3.5 Cutover
- [ ] 3.6 Runbook rollback diuji
- [ ] 3.7 Monitoring dan alert

### Fase 4: Dekomisioning

- [ ] 4.1 Hapus kode PHP setelah cutover stabil 2–4 minggu
- [ ] 4.2 Perbarui Docker, README, `AGENTS.md`, `CONTINUITY.md`, dan `analisa-apiamis.md`
- [ ] 4.3 Hapus `docker/whatsapp-bridge` dan `analyze-rab.js`
- [ ] 4.4 Hapus fitur `master-fase-pekerjaan` (controller, model, route, migrasi, seeder, dan prefix di `CheckRoutePermission`) setelah konfirmasi frontend

---

## Temuan yang perlu keputusan

| No | Temuan | Rekomendasi |
| --- | --- | --- |
| T1 | `/api/debug-data` mengembalikan data mentah tanpa role | Hapus, jangan dipindah |
| T2 | Uang disimpan sebagai `float` di `Pekerjaan` dan `Kontrak` | Pakai `rust_decimal`, catat perilaku di fixture |
| T3 | Modul di repo lebih banyak dari rencana | Tambah ke Fase 2, dan tentukan urutan dan prioritasnya |
| T4 | Skema dari migrasi tidak lengkap | Selesai: pakai dump struktur MySQL 8.0.30 (111 tabel) |
| T5 | 29 token di dump mengarah ke user yang sudah tidak ada | Ditolak 401 oleh `authenticate`. Dibersihkan di sumber sebelum cutover |
| T6 | Tabel `users` tidak punya kolom status aktif | Rencana "user nonaktif" di 1.5 tidak bisa diuji. Perlu keputusan kolom atau mekanisme nonaktif |
| T7 | Semua token `abilities = ["*"]` dan tidak ada yang kedaluwarsa | Scope public API belum dipakai token internal. Perlu dipastikan sebelum API key publik dibuat |
| T8 | Dump berisi hash password dan token asli | Tidak dimasukkan ke repo. Hanya fixture struktur (`rust/fixtures/auth_schema.sql`) yang di-commit |
| T9 | `pagu` di `tbl_pekerjaan` 550 dari 558 baris di atas 2^24, dan kolomnya `FLOAT` (single precision). Data sampel saat ini hanya kelipatan 1.000, jadi belum ada angka yang berubah, tetapi input berdigit penuh akan dibulatkan. Contoh: `123456789` menjadi `123456792` | Ubah ke `DECIMAL(15,2)` setelah cutover, atau cukup dicatat sebagai risiko. Cek dengan `docs/migration/checks/pagu_checks.sql` |
| T11 | Produksi `apiamis.cianjur.space` mengembalikan exception Laravel lengkap (29 frame stack trace, path `/var/www/html/...`) untuk 404. Itu tanda `APP_DEBUG=true` | **Segera set `APP_DEBUG=false` di produksi.** Skrip rekaman sekarang tidak menyimpan body seperti itu |
| T12 | Header `x-powered-by: PHP/8.3.35` terbuka ke publik | Hapus header ini dari server atau proxy |
| T13 | Token API dan password akun pernah ditulis di percakapan. Token sudah dicabut, tapi password belum diganti | Ganti password akun. Untuk rekaman berikutnya, pakai akun uji khusus |
| T14 | Data lokal (dump) tidak sama dengan produksi: `tbl_kegiatan` id 29, `sumber_dana` `PAD` di dump dan `DAU` di produksi | Dicatat di `KNOWN_DATA_DRIFT` pada tes paritas. Cek ulang setelah dump lengkap |
| T15 | Timestamp `created_at` di dump lokal 7 jam berbeda dari produksi untuk baris yang sama (id 1 kegiatan) | Tes paritas tidak membandingkan timestamp. Pastikan zona waktu DB produksi sebelum fixture timestamp dipakai |
| T16 | Email default bypass maintenance (`ilhamtaufiq@gmail.com`) tertulis di kode `MaintenanceModeService` dan di Rust | Pindahkan ke konfigurasi, dan jangan jadikan default di kode |
| T17 | Format datetime di `UserResource` (login) belum terverifikasi. Kode memakai Carbon mentah (`.000000Z`), sedangkan resource lain memakai `toIso8601String` (`+00:00`). Juga `avatar_url` hanya dibangun untuk disk `public` | Bandingkan dengan respon login produksi sekali akun uji tersedia |
| T18 | 404 show per id: Rust mengembalikan `{"message":"Not Found."}`. Laravel memakai `ModelNotFoundException` dan bentuk respon bergantung pada `APP_DEBUG`. Karena produksi memakai `APP_DEBUG=true` (T11), respon produksi kemungkinan berbeda | Verifikasi setelah `APP_DEBUG=false`, lalu sesuaikan |
| T19 | `GET /api/app-settings` publik dan mengembalikan semua setting kecuali key yang ada di daftar rahasia (`chat_api_key_*`, `mail_password`, `google_drive_*`, `s3_secret_access_key`). Setting lain (misalnya username atau host) ikut terbaca tanpa login | Tinjau daftar key yang aman, atau batasi endpoint ini ke login. Perilaku dipertahankan dulu agar setara dengan Laravel |
| T20 | Frontend Arumanis (repo terpisah) mungkin masih memakai `master-fase-pekerjaan`. Repo ini tidak memuat frontend-nya | Konfirmasi dulu bahwa frontend tidak memakai endpoint ini sebelum 4.4 |
| T21 | Endpoint Pekerjaan Rust belum memberi progres, foto, kontrak, atau assignment yang sama dengan Laravel. Fail-closed untuk role lain | Jangan dipakai frontend sebelum 2.6 lengkap. Pantau header `x-partial-response` |
| T22 | Urutan link paginasi (`appends`) dan bentuk `per_page=-1` (batas 80) belum diverifikasi terhadap produksi | Verifikasi dengan respon produksi saat akun uji tersedia |
| T23 | Rumus progres (`ProgressTabMetricsService`) dan estimasi sudah diport dan diuji dengan angka yang dihitung dari rumus PHP, tapi belum dibandingkan dengan produksi. Tabel `tbl_progress` dan `pekerjaan_progress_estimasi_history` tidak ada di dump yang tersedia | Cocokkan dengan respon produksi dan data progres asli sebelum modul ini dipakai frontend |
| T24 | Relasi tambahan Pekerjaan (hitungan, kontrak, assignment) dicocokkan dengan kode Laravel dan diuji dengan data buatan di DB lokal, belum dengan respon produksi. Format `addendum_ke` (INT UNSIGNED) dan tanggal sudah dicek terhadap skema dump | Bandingkan dengan respon produksi untuk satu pekerjaan yang punya kontrak dan assignment |
| T25 | **Selesai.** `GET /api/pekerjaan/{id}` dan respon `PUT/PATCH` memakai `PekerjaanDetailResource` (`pekerjaan_detail.rs`): foto, berkas (dengan uploader), kontrak (dengan penyedia, `pekerjaan_ids`, `is_checklist_complete`), output, penerima (PIN), tags, progres, pengawas dan pendamping. Tanpa metrik daftar. Header `x-partial-response` dihapus. Tes `pekerjaan_detail_db` lulus. Belum dibandingkan dengan respon produksi |
| T26 | `TiketResource.pekerjaan` memuat `PekerjaanResource` hanya dengan relasi `pekerjaan`. Hitungan dan foto dibuat sesuai nilai dasar Laravel (tanpa withCount). Rincian ini belum dibandingkan dengan respon produksi. `image_url` mengembalikan string kosong untuk disk selain `public` | Bandingkan satu tiket dengan lampiran dan satu dengan pekerjaan dari produksi |
| T27 | `docs/schema.md` dan `fixtures` tidak sesuai migrasi terbaru untuk SpamUnit. Contoh: `tbl_spam_achievements.sumber` dan `tbl_spam_budgets.pekerjaan_id` ada di migrasi 2026-10-08 tetapi tidak di dokumen. Migrasi kelembagaan (2026-07-10) juga belum tercermin | Buat dump `--no-data` dari DB yang sudah menjalankan semua migrasi, lalu perbarui `docs/schema.md`. Jangan menebak dari migrasi |
| T28 | BFF di repo `arumanis` menangani banyak hal selain auth: SPA dan dist di produksi, SEO `/publikasi` dan `/puspen`, `sitemap.xml`, `/bff/api/*`, `/bff/sipd/*`, Instagram dan webhook Meta, Umami, Broadcasting auth, dan uji koneksi AI. Setelah BFF dihapus, semua ini harus punya penggantinya | Tentukan per fitur: pindah ke Rust, dipertahankan di tempat lain, atau dihentikan. Static file frontend perlu host sendiri (Apache atau nginx) |
| T29 | Laravel `PekerjaanController@update` memakai `$validated['kecamatan_id'] ?? $pekerjaan->kecamatan_id`. Jika kecamatan dikirim null, validasi memakai nilai lama dan lolos, lalu `update()` menulis null. Akibatnya pekerjaan non-konsultan bisa kehilangan kecamatan dan desa tanpa error | Port meniru perilaku Laravel. Perbaikan (misalnya memakai nilai yang dikirim, bukan fallback) perlu keputusan. Tes memakai data yang valid |
| T30 | Slug tag memakai `Str::slug` Laravel. Port hanya menangani ASCII (huruf non-ASCII dibuang, Laravel mentransliterasi, mis. `é` menjadi `e`) | Samakan dengan library transliterasi, atau batasi input nama tag ke ASCII |
| T31 | **Selesai.** `PUT/PATCH /api/pekerjaan/{id}` memeriksa scope `byUserRole()` (403 bila di luar scope). Laravel tetap hanya mengandalkan route permission, jadi ini perbaikan yang disengaja, bukan paritas. Tes: `pekerjaan_write_db` lulus |
| T32 | **Selesai.** `GET /api/pekerjaan` dan `GET /api/pekerjaan/{id}` memakai scope `byUserRole()` (sebelumnya hanya role penuh). `GET /api/foto` juga memakai scope lewat pekerjaan induk. Tes HTTP `pengawas_sees_only_assigned_pekerjaan_over_http` lulus |
| T33 | **Belum terverifikasi.** Sumber `laravel/framework` tidak bisa diambil di sandbox (GitHub 403, `composer install` gagal). Rust tetap mengirim `null` untuk relasi yang tidak ada. Perlu dicek di produksi: `GET /api/foto/{id}` untuk foto tanpa `penerima_id`. Bila Laravel 500, samakan Rust atau perbaiki Laravel |
| T34 | **Selesai.** Thumbnail 120x120 (crop, format sumber) dibuat saat upload di `media.rs` dan ditulis ke `conversions/{nama}-thumb.{ext}`. `foto_thumb_url` memakai thumbnail. Sharpen Spatie belum diport (perbedaan kecil di piksel). Tes `foto_db` memeriksa dimensi 120x120 |
| T35 | **Selesai.** `main.rs` membaca `.env` root repo juga dari lokasi crate, jadi tidak perlu menjalankan dari `rust/`. Bila `APP_KEY` tetap tidak ada, respon foto dengan penerima mengembalikan 500 dengan pesan yang jelas |
| T36 | Baca penerima dan berkas (daftar dan show) tidak memakai scope `byUserRole()`, sama dengan Laravel. Tulis (store, update, destroy, bulk) memakai scope, konsisten dengan T31. Akibatnya user di luar scope bisa membaca data penerima (nik dan alamat ter-mask bila tanpa PIN) dan berkas pekerjaan lain | Putuskan: pasang scope juga pada baca penerima dan berkas (perbaikan keamanan), atau biarkan sesuai Laravel |
| T37 | `PenerimaResource` membandingkan `$pin === AppSetting::getValue(penerima_pin, '123456')`. Bila baris `penerima_pin` ada tetapi bernilai NULL, dan request tanpa PIN, kedua sisi null sehingga data terbuka tanpa PIN. Port Rust meniru ini | Pastikan `app_settings.penerima_pin` di produksi tidak NULL. Perbaiki di Laravel (`=== null` ditolak) bila perlu |
| T38 | **Selesai.** Filter judul berbagi (`applyPengawasSharedBerkasJudulFilter`) diport ke `berkas.rs` (`shared_clause`), termasuk alias, pencocokan awalan, dan bentuk kompak. Pengaturan `pengawas_berkas_show_*` dibaca dari `app_settings` (hanya nilai `1`). Tes unit dan tes DB `pengawas_sees_own_and_shared_titles_only` lulus. Belum ada perbandingan langsung dengan hasil PHP (vendor tidak bisa di-install) |
| T39 | `export-pdf` (K2, tooling dokumen) dan `upload-from-url` (daftar host yang diizinkan, SSRF) belum dipindah. `quick-share` juga belum. Route ini tetap di Laravel lewat Apache | Putuskan K2 dan daftar host, lalu port |
| T40 | `pekerjaan_db::kontrak_addendum_and_assignment_sources` gagal sekali pada run penuh, lalu lulus pada dua run ulang. Test dalam satu binary berbagi data uji (kontrak dan user 5) dan berjalan paralel | Jalankan dengan `--test-threads=1` atau pisahkan data uji per test |
| T10 | Kualitas data: 14 dari 20 kegiatan punya `pagu = 0`, 7 pekerjaan punya `pagu = 0`, dan 15 kegiatan punya total pekerjaan melebihi pagu kegiatan | Perlu konfirmasi dengan pemilik data sebelum dijadikan fixture acuan |

## Keputusan terbuka

| No | Keputusan | Status |
| --- | --- | --- |
| K1 | Fixture direkam dari staging, atau ditulis manual | Menunggu |
| K2 | Dokumen Word/PDF: Typst, Chromium headless, atau edit template XML | Menunggu contoh output |
| K3 | Real-time: protokol Pusher minimal, atau WebSocket baru | Menunggu |
| K4 | Driver queue dan scheduler di produksi (`QUEUE_CONNECTION`) | Menunggu |
| K5 | Target waktu cutover | Menunggu |
| K6 | Penghapusan kode RAB Analyzer dan WhatsApp bridge dari repo | Menunggu konfirmasi |
| K7 | `master-fase-pekerjaan`: tidak dimigrasi, dihapus saat dekomisioning (4.4) | Diputuskan, 2026-10-08 |

---

## Riwayat

| Tanggal | Langkah | Hasil |
| --- | --- | --- |
| 2026-10-08 | Rencana migrasi ke Rust (Axum) tanpa Laravel disusun | Dokumen rencana dikirim ke pengguna. Versi terakhir: RAB Analyzer dan WhatsApp bridge dikeluarkan dari cakupan |
| 2026-10-08 | Branch `claude/dreamy-hopper-g5ggpf` dibuat dari `main` | Commit `5799f47` berisi workspace awal di `rust/` |
| 2026-10-08 | Branch `rust` dibuat dari `5799f47` dan di-push ke `origin/rust` | Branch kerja untuk semua persiapan migrasi |
| 2026-10-08 | Workspace Rust awal diverifikasi | `cargo test --workspace` lolos (4 test), `cargo clippy -D warnings` bersih, `cargo fmt --check` bersih |
| 2026-10-08 | Log migrasi ini dibuat | `docs/migration/log.md` |
| 2026-10-08 | 0.1 Inventaris route | `docs/migration/routes.md`: 486 route, 71 grup. Temuan: `/api/debug-data` mengembalikan data mentah tanpa role |
| 2026-10-08 | 0.2 Inventaris model | `docs/migration/models.md`: 77 model. Temuan: uang sebagian `float`, `Penerima.nik` dan `alamat` terenkripsi, `SpseSession` menyimpan cookie terenkripsi |
| 2026-10-08 | 0.3 Inventaris skema (versi awal, parsial) | `docs/schema.md`: hanya 33 tabel terbaca dari migrasi, sedangkan model merujuk ~80. Tabel inti dibuat di luar migrasi. Perlu `mysqldump --no-data` dari staging |
| 2026-10-08 | 1.1 sampai 1.5 (sebagian) | Format error 401 dan 422, middleware (request id, timeout, body limit, CORS), crate `auth` untuk token Sanctum, workflow CI, dan Dockerfile. Test: 14 lolos. Smoke test server lewat curl OK. Dockerfile belum di-build |
| 2026-10-08 | 0.3 Inventaris skema (final) | `docs/schema.md` dibuat ulang dari dump struktur (`CREATE TABLE`): 111 tabel, 1068 kolom. Temuan: `tbl_pekerjaan.pagu` `float`, `tbl_penerima.nik` dan `alamat` terenkripsi, `tbl_spse_sessions.encrypted_cookies` terenkripsi. Dump tidak berisi data, jadi 0.5 belum |
| 2026-10-08 | 1.5 Lookup token di database | `crates/auth` membaca `personal_access_tokens` dan `users`. 7 test integrasi lolos terhadap dump asli (1.914 token, 38 user) di MariaDB lokal. Job integrasi ditambahkan ke CI |
| 2026-10-08 | 0.5 Data uji dari dump `tbl_kegiatan` dan `tbl_pekerjaan` | Dimuat ke MariaDB lokal. Pengecekan pagu disimpan di `docs/migration/checks/pagu_checks.sql`. Temuan T9 dan T10. Data pribadi (NIP) tidak dicatat dan tidak di-commit |
| 2026-10-08 | 0.4 Skrip rekaman fixture | `rust/fixtures/record.sh`: GET saja, redaksi field sensitif, diuji dengan server lokal palsu. Sandbox tidak bisa menjangkau `apiamis.cianjur.space` (403 dari kebijakan jaringan) |
| 2026-10-08 | 0.4 Rekaman tanpa token dari produksi | `rust/fixtures/live/`: `up` 200 dan 401 untuk kecamatan, desa, kegiatan, pekerjaan. 404 ditemukan membocorkan stack trace (T11), jadi tidak disimpan |
| 2026-10-08 | 0.4 Rekaman dengan token | Daftar `kecamatan` (33), `desa` (15 per halaman), `kegiatan` (15), dan `pekerjaan` (20) direkam. Field pribadi dihapus: NIP, telepon, email, nama PPTK, nama pengawas dan pendamping. Token uji dicabut dan sekarang 401. Dua respon 404 tidak disimpan karena berisi stack trace (T11) |
| 2026-10-08 | 2.2 GET /api/kecamatan di Rust | Endpoint dengan auth token Sanctum, query `tbl_kecamatan` + hitungan `tbl_desa`, dan mapping sama dengan `KecamatanResource`. Uji: mapping cocok 100% dengan fixture live (33 baris), uji DB lolos, smoke test server lokal: 401/401/200. Temuan saat uji: kolom TIMESTAMP harus dibaca sebagai `DateTime<Utc>` |
| 2026-10-08 | 2.2 desa dan kegiatan di Rust | `GET /api/desa` (search, kecamatan_id, paginasi) dan `GET /api/kegiatan` (tahun, per_page=-1, paginasi). Uji: fixture halaman 1 cocok persis (termasuk meta dan links), paritas DB kegiatan cocok kecuali T14. Pagination untuk halaman selain 1 belum diverifikasi terhadap Laravel |
| 2026-10-08 | 1.7 Route permission | Port `CheckRoutePermission`: admin bypass, whitelist, rule `route_permissions` (exact lalu `:param`), admin-only, dan mutasi tanpa rule. Pesan 403 sama dengan Laravel. Catch-all `/api/*` memastikan path yang belum dipindah juga dicek. Batasan: request tanpa token ke route yang belum ada di Rust mendapat 404, bukan 401 |
| 2026-10-08 | 1.9 Maintenance gate | `crates/api/src/maintenance.rs`: flag dan daftar bypass dari `app_settings`, env `MAINTENANCE_BYPASS_EMAILS`, default email. Dipasang sebagai layer terluar sehingga jalan sebelum permission. Smoke test lokal cocok dengan Laravel. Catatan keamanan: email default bypass tertulis di kode (sama dengan Laravel), perlu dipindah ke konfigurasi |
| 2026-10-08 | 1.6 Login email dan password | `POST /api/auth/login`: rate limit sebelum validasi, validasi 422 seperti `ValidationException`, bcrypt (hash `$2y$` dari PHP diverifikasi), maintenance dicek setelah kredensial, token `id\|plain`. Tes: login E2E dan smoke test (422, 200, token dipakai 200, 429 setelah 5 percobaan). Belum: Google OAuth |
| 2026-10-08 | 2.2 show per id | `GET /api/kecamatan/{id}` (dengan `desa` bersarang tanpa key `kecamatan`), `GET /api/desa/{id}`, dan `GET /api/kegiatan/{id}`, semuanya dibungkus `data`. Tes DB dan smoke test: 200 dan 404 (id tidak ada atau tidak valid), 401 tanpa token. Tes DB dipisah per rentang id supaya tidak saling menghapus data |
| 2026-10-08 | 2.3 Lookup dan konfigurasi | `app-settings` (index publik dan maintenance), `tags` (index dengan search, show). Tes DB dan smoke test cocok. Temuan T19 (setting publik) |
| 2026-10-08 | Keputusan K7: `master-fase-pekerjaan` tidak dimigrasi | Hanya dipakai dalam repo ini (controller, model, route, migrasi, seeder, dan prefix permission). Kode dihapus saat dekomisioning, bukan sekarang |
| 2026-10-08 | 2.3 document-types dan penyedia | `GET /api/document-types` (array langsung, datetime Carbon, sama seperti Eloquent), `GET /api/penyedia` (search, `per_page=-1`, paginasi) dan `GET /api/penyedia/{id}` dengan dokumen dari tabel `media`. Tes DB dipisah per tabel. Smoke test cocok. `app-settings` admin ditunda |
| 2026-10-08 | Blokir 2.4 dan 2.5 | Tiket, SpamUnit, dan Checklist bergantung pada `PekerjaanResource` dan relasinya. `PekerjaanResource` memanggil `ProgressTabMetricsService`, `PekerjaanProgressEstimasiSummaryService`, dan metrik foto. Perlu keputusan urutan 2.6 sebelum modul ini bisa dipindah dengan benar |
| 2026-10-08 | 2.6 Pekerjaan (GET, sebagian) | Opsi 1: list dan show dengan gate role. Filter tahun, kecamatan, desa, kegiatan, sub kegiatan, sub bidang, search, sort whitelist. Uji DB dan smoke test: operator 200 dengan paginasi dan header, role tanpa akses 403, tanpa token 401. Angka progres dan kontrak belum ada |
| 2026-10-08 | 2.6 progres | `progress_metrics.rs` (port `ProgressTabMetricsService`) dan `progress_estimasi.rs` (port `PekerjaanProgressEstimasiSummaryService`). Unit test dengan angka yang dihitung manual, tes DB dengan baris `tbl_progress` buatan. `progress_total` dan `deviasi` sekarang terisi di Pekerjaan. Temuan T23 |
| 2026-10-08 | 2.6 summary=1 | Estimasi dipasang di list. Tes DB estimasi (tahun dari kegiatan, tanggal terbaru, deviasi) lolos. Smoke test: summary=1 200 dengan kelima key estimasi. Ditemukan dan diperbaiki: `tahun_anggaran` berjenis SMALLINT UNSIGNED |
| 2026-10-08 | 2.6 relasi Pekerjaan | Hitungan, kontrak dengan penyedia dan addendum, assignment_sources (dihitung untuk non-admin), search penyedia kontrak, sort penerima_count, dan penghapusan key tags/kontrak saat per_page=-1. Tes DB kontrak dan assignment lolos. Smoke test list paged dan unbounded sesuai Laravel. Ditemukan: `addendum_ke` INT UNSIGNED |
| 2026-10-08 | 2.6 summary lengkap (daftar) | Tabel `tbl_output` dan `tbl_document_registers` ditambah ke fixture skema. `resolveFotoMetrics` cabang lengkap dipindah (port + 6 unit test). `output`, `registers` dengan `type`, dan penyedia/addendum per mode dicocokkan dengan Laravel. Tes DB baru lolos (`summary_page_loads_output_foto_metrics_and_registers`) bersama tes lama. Ditemukan: `penerima_is_optional` membutuhkan penerima berbeda (bukan hanya jumlah foto). Ditemukan: show memakai resource lain (T25) |
| 2026-10-08 | 2.4 Tiket (GET) dan 2.5 Checklist (GET) | `tiket.rs`, `checklist.rs`, dan `users.rs` (UserResource untuk relasi). Tes DB `tiket_checklist_db` lolos (scope pemilik, komentar, checklist dengan pembaruan terakhir). Skema di `fixtures/tiket_checklist_schema.sql` dan job CI ditambah. CI `rust` masih merah sebelum commit ini (lihat riwayat sebelumnya) |
| 2026-10-08 | CI dinonaktifkan otomatis | `rust.yml` hanya `workflow_dispatch` atas permintaan pengguna. Run push sebelumnya tetap merah |
| 2026-10-08 | 2.5 history dan post-pekerjaan-checklist (GET) | Port `history` (filter, search, meta tanpa links, per_page 1 sampai 100) dan `post-pekerjaan-checklist`. Tes HTTP lewat router dengan token: admin 200 dan role lain 403. Konteks tes dipisah supaya tes paralel tidak saling menghapus data |
| 2026-10-08 | 2.4 SpamUnit diblokir | Respon mentah Eloquent dan skema tidak terverifikasi (T27). Ditunda sampai dump struktur terbaru tersedia |
| 2026-10-08 | K8: sesi cookie di Rust menggantikan auth BFF | `arumanis_session` (httpOnly, SameSite=Strict, 12 jam) berisi token Sanctum. `require_auth`, route permission, maintenance, dan kecamatan membaca Bearer atau cookie (`crates/api/src/session.rs`). Ditambah `GET /api/auth/me` dan `POST /api/auth/logout`. Tes `session_db` lolos. Semua tes integrasi `api` lolos |
| 2026-10-08 | K9: token tidak lagi di body login Rust | Token hanya di cookie `arumanis_session`. Ini menyimpang dari kontrak Laravel (`{user, token}`). Klien non-browser yang butuh token harus memakai Laravel sampai ada jalur yang disepakati. Tes login dan session lolos |
| 2026-10-08 | Frontend pindah dari BFF untuk auth dan API utama | Repo `arumanis` branch `claude/bff-auth-removal`. Auth dan `/bff/api/*` pindah ke `/api/*`. Laravel menerima cookie sesi lewat middleware `AcceptSessionCookie`. Rust punya `sync-token`. Rule Apache di `rust/deploy/apache-apiamis.conf`. Belum diverifikasi dengan typecheck atau Vitest (dependency Git tidak bisa diunduh di sandbox) |
| 2026-10-08 | K11: Umami dan Instagram dihapus, tidak dimigrasi | Keputusan pengguna. Instagram Hub, webhook Meta, galeri, analytics realtime Umami, dan tracking publik dihapus dari frontend dan BFF di branch `claude/bff-auth-removal`. Embed postingan Instagram di editor publikasi dipertahankan sebagai konten. Belum diverifikasi dengan typecheck |
| 2026-10-08 | K12: efek samping update Pekerjaan dipindah ke Rust | `PUT/PATCH /api/pekerjaan/{id}` di Rust menulis audit (`tbl_audit_logs`), notifikasi database untuk admin lain (`notifications`), dan sync tag. Broadcast realtime belum (menunggu K3). Respon memakai bentuk daftar (T25). Apache meneruskan PUT/PATCH ke Rust di `rust/deploy/apache-apiamis.conf`. Tes DB `pekerjaan_write_db` lolos, dan semua tes DB api lolos |
| 2026-10-08 | Tags ditulis di Rust | `POST /api/tags`, `PUT/PATCH/DELETE /api/tags/{id}` dengan audit created, updated, dan deleted (modul `audit` bersama). Tes DB `tags_write_db` lolos. Apache meneruskan rute ini ke Rust di `rust/deploy/apache-apiamis.conf` |
| 2026-10-08 | K3: realtime dihapus | Keputusan pengguna. Broadcast update pekerjaan (trait dan event `PekerjaanUpdated`) dan kanal `broadcast` notifikasi dihapus dari Laravel. Hook realtime notifikasi di frontend dihapus, badge memakai polling 60 detik. Live chat (Echo) belum disentuh, menunggu keputusan terpisah |
| 2026-10-08 | Ruang lingkup diperluas | Repo punya lebih banyak modul dari rencana awal: blog, kanban, live chat, procurement SPSE, SIPD, Puspen, tanda tangan PDF, Google Drive, backup, dan WhatsApp. Perlu dimasukkan ke daftar modul Fase 2 |
| 2026-10-08 | Scope `byUserRole()` dan `userCanAccess()` diport ke `access.rs` | Aturan: admin/manager/super-admin/operator penuh; pengawas, konsultan_pengawas, tfl hanya `user_pekerjaan`; role lain `user_pekerjaan` atau `kegiatan_role`. Uji DB `access_db` lulus. Belum dipasang ke list/show/update (T31, T32) |
| 2026-10-08 | Validasi koordinat diport ke `koordinat.rs` | Parser koordinat, indeks GeoJSON (`VILLAGE_GEOJSON_PATH`, default `resources/geojson/id3203_cianjur_simplified.geojson`), point-in-polygon dengan lubang dan MultiPolygon. Output `parseKoordinat` dicocokkan dengan PHP. 13 unit test lulus |
| 2026-10-08 | Foto CRUD di Rust: show, store, update (PUT/PATCH dan POST dengan `_method=PUT`), destroy, bulk destroy | Berkas disimpan ke `storage/app/public/{media_id}/` dengan baris `media` (tanpa thumbnail, T34). Otorisasi memakai `access.rs`. Koordinat memakai `koordinat.rs`. Audit dan notifikasi admin ikut ditulis dalam transaksi yang sama. Multipart dengan batas body 51 MB per rute. Tes DB `foto_db` lulus (termasuk berkas 2 MB di atas batas body global). Belum: daftar `GET /api/foto` (perlu `byUserRole` di list) |
| 2026-10-08 | Notifikasi admin dipindah ke `notify.rs` | Dipakai oleh pekerjaan dan foto. Perilaku pekerjaan tidak berubah, tes `pekerjaan_write_db` lulus |
| 2026-10-08 | T31: scope pada update pekerjaan | `pekerjaan_write::update` menolak 403 di luar scope |
| 2026-10-08 | T32: scope pada daftar dan detail pekerjaan, daftar foto | `pekerjaan::index`/`show` memakai `access::restriction` (alias `p`). `foto::index` menyaring lewat pekerjaan induk, dengan filter tahun, search, pekerjaan_id, latest_only, dan paginasi. Tes HTTP pengawas lulus |
| 2026-10-08 | T34: thumbnail foto | `media.rs` membuat thumb 120x120 dengan crate `image`. Tes `foto_db` memakai gambar JPEG/PNG nyata |
| 2026-10-08 | Penerima CRUD di Rust (9 rute) | `penerima.rs`: enkripsi `nik` dan `alamat` dengan `APP_KEY`, masking dan PIN, statistik, dan rekap. Tulis memakai scope (T36). Tes `penerima_db` lulus |
| 2026-10-08 | Berkas CRUD di Rust (6 rute dan bulk) | `berkas.rs`: daftar, jenis dokumen, store, show, update (PUT, PATCH, dan POST `_method=PUT`), destroy, bulk. Pengawas ditolak sementara (T38). Skema `tbl_berkas` ditambahkan ke `rust/fixtures/berkas_schema.sql`. Tes `berkas_db` lulus |
| 2026-10-08 | Audit dan notifikasi dipindah ke `changes.rs` | Dipakai foto dan berkas dengan `Target` (`App\\Models\\Foto`, `App\\Models\\Berkas`) |
| 2026-10-08 | Ruang disk penuh | Sandbox kehabisan kuota tulis. Cache composer (7 GB) dihapus. Tidak ada data repo yang dihapus |
| 2026-10-08 | T38: filter judul berbagi berkas | Pengawas melihat berkas miliknya dan berkas berjudul yang aktif. Penolakan 403 sementara dihapus |
| T41 | `ProgressResource` dan `PengawasResource` di detail: bila relasi kosong, Rust mengirim `null`. Laravel bisa 500 seperti T33 (`$this->progress->id` pada null). Belum diverifikasi | Cek di produksi untuk pekerjaan tanpa progres |
| 2026-10-08 | T25: detail pekerjaan | `show` dan `update` memakai `pekerjaan_detail::build`. Relasi dibandingkan per key dengan Laravel: foto dan berkas tanpa relasi yang tidak dimuat, kontrak tanpa `kegiatan` dan `pekerjaans`, penerima tanpa `pekerjaan`. Tes HTTP lulus |
