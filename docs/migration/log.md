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
- [ ] 2.4 Tiket dan SpamUnit (terhalang 2.6: `TiketResource` memuat `PekerjaanResource`, SpamUnit memuat `pekerjaan.kegiatan`, `output`, dan `kontrak`)
- [ ] 2.5 Checklist proyek (terhalang 2.6)
- [~] 2.6 Pekerjaan: hanya GET (`/api/pekerjaan`, `/api/pekerjaan/{id}`), dibatasi role `admin`, `manager`, `super-admin`, dan `operator` (fail-closed: role lain 403). Belum dipindah: progres, foto, kontrak, `assignment_sources`, pencarian `kontrak.penyedia`, sort `penerima_count`, dan `summary`. Bagian yang belum dipindah dikirim sebagai null dan respon diberi header `x-partial-response`. Frontend jangan dipindah ke endpoint ini sebelum bagian ini selesai
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
| 2026-10-08 | Ruang lingkup diperluas | Repo punya lebih banyak modul dari rencana awal: blog, kanban, live chat, procurement SPSE, SIPD, Puspen, tanda tangan PDF, Google Drive, backup, dan WhatsApp. Perlu dimasukkan ke daftar modul Fase 2 |
