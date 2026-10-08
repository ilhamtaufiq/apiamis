# Log Migrasi Backend ke Rust (Axum)

Branch kerja: `rust`. Rencana lengkap ada di dokumen rencana migrasi.

Status: `[x]` selesai, `[~]` sedang dikerjakan, `[ ]` belum dimulai.

Setiap kali ada langkah yang selesai, tambahkan entri di bagian **Riwayat** dan ubah status di **Checklist**.

---

## Checklist

### Fase 0: Spesifikasi dan fixture (2–4 minggu)

- [ ] 0.1 Inventaris route dari `routes/api.php` dan `routes/web.php` ke `docs/migration/routes.md`
- [ ] 0.2 Inventaris model, observer, cast, listener, event, dan service ke `docs/migration/models.md`
- [ ] 0.3 Inventaris data (tabel, kolom terenkripsi, JSON, decimal, soft delete) ke `docs/schema.md`
- [ ] 0.4 Fixture perilaku untuk endpoint prioritas (rekam dari staging atau tulis manual)
- [ ] 0.5 Dump database staging sebagai data uji tetap
- [ ] 0.6 Konfirmasi keputusan terbuka (lihat bagian Keputusan)

### Fase 1: Fondasi dan autentikasi (3–4 minggu)

- [x] 1.0 Workspace Cargo di `rust/` dengan crate `api` dan `shared`, `GET /up`, `GET /api/health`, fallback 404 ala Laravel
- [ ] 1.1 Konfigurasi dari `.env`, format error 401 dan 422 yang sama dengan Laravel
- [ ] 1.2 Middleware: request id, timeout, body limit, CORS
- [ ] 1.3 Dockerfile multi-stage yang hanya membangun binary Rust
- [ ] 1.4 CI: `fmt`, `clippy`, `test`, dan job integrasi MySQL
- [ ] 1.5 Validasi token Sanctum di Rust (diuji dengan token dari fixture)
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
