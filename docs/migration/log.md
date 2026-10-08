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
- [~] 0.4 Fixture perilaku untuk endpoint prioritas. Rekaman tanpa token dari `apiamis.cianjur.space` sudah ada di `rust/fixtures/live/` (up, dan 401 untuk endpoint yang butuh login). Belum ada rekaman dengan token, jadi respon 200 untuk data bisnis belum direkam
- [~] 0.5 Dump database staging sebagai data uji tetap. Ada dump data `tbl_kegiatan` (20 baris) dan `tbl_pekerjaan` (558 baris), belum dump lengkap semua tabel bisnis
- [ ] 0.6 Konfirmasi keputusan terbuka (lihat bagian Keputusan)

### Fase 1: Fondasi dan autentikasi (3–4 minggu)

- [x] 1.0 Workspace Cargo di `rust/` dengan crate `api` dan `shared`, `GET /up`, `GET /api/health`, fallback 404 ala Laravel
- [x] 1.1 Konfigurasi dari `.env`, format error 401 dan 422 yang sama dengan Laravel
- [x] 1.2 Middleware: request id, timeout, body limit, CORS (CORS disalin dari `config/cors.php`, termasuk pola `*.pages.dev`)
- [~] 1.3 Dockerfile multi-stage yang hanya membangun binary Rust (ditulis, belum di-build: Docker daemon tidak aktif di sandbox)
- [~] 1.4 CI: `fmt`, `clippy`, `test`, dan job integrasi MySQL (`.github/workflows/rust.yml`). Workflow belum pernah dijalankan di GitHub
- [x] 1.5 Validasi token Sanctum di `crates/auth`: lookup ke `personal_access_tokens`, cek tipe user, hash sha256, kedaluwarsa, dan user pemilik. Diuji 7 test integrasi terhadap data dump asli (MariaDB lokal). Belum diuji end-to-end dengan plain token asli, karena dump hanya menyimpan hash
- [ ] 1.6 Login email/password (bcrypt) dan Google OAuth
- [ ] 1.7 Permission Spatie dan middleware permission route, dengan matriks test
- [ ] 1.8 Impersonation dengan penanda token dan audit log

### Fase 2: Modul bisnis (4–7 bulan)

- [ ] 2.1 Public API v1 (`/api/public/v1/*`)
- [ ] 2.2 Master data: kecamatan, desa, kegiatan
- [ ] 2.3 Lookup dan konfigurasi
- [ ] 2.4 Tiket dan SpamUnit
- [ ] 2.5 Checklist proyek
- [ ] 2.6 Pekerjaan
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
| 2026-10-08 | Ruang lingkup diperluas | Repo punya lebih banyak modul dari rencana awal: blog, kanban, live chat, procurement SPSE, SIPD, Puspen, tanda tangan PDF, Google Drive, backup, dan WhatsApp. Perlu dimasukkan ke daftar modul Fase 2 |
