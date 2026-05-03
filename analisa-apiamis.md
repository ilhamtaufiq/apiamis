# Analisa Komprehensif: ApiAmis Backend (Laravel 12)

Dokumen ini berisi hasil analisa teknis, katalog fitur, dan rekomendasi pengembangan untuk sistem ApiAmis.

## 1. Arsitektur & Teknologi Utama
Sistem ini dibangun dengan arsitektur **API-First** menggunakan framework modern:
- **Framework**: Laravel 12.x (Latest Stable).
- **Database**: MySQL dengan optimasi Full-text search (pada tabel `pekerjaan` dan `kegiatan`).
- **Security Context**: 
  - **Sanctum 4.0**: Token-based authentication yang ringan.
  - **Socialite**: Integrasi Google OAuth.
  - **Spatie Permission**: Role-based access control (RBAC).

---

## 2. Katalog Fitur Utama

### A. Sistem Autentikasi Modern
- **Hybrid Login**: Menggunakan email/password tradisional dan Google OAuth.
- **Role Impersonation**: Admin dapat "menyamar" sebagai user lain (misal: pengawas) untuk debugging data lapangan tanpa memerlukan password user tersebut.

### B. Security & Data Isolation
- **Dynamic Route Permission**: Middleware `CheckRoutePermission` yang memvalidasi akses berdasarkan endpoint dan method (GET/POST/DELETE) secara dinamis dari database.
- **Row-Level Security (RLS)**: Penggunaan scope `ByUserRole` pada model `Pekerjaan` memastikan pengawas hanya melihat proyek yang ditugaskan kepada mereka.

### C. Innovative Data Processing (Sidecar Integration)
- **RAB Analyzer**: Menggunakan script Node.js sidecar (`analyze-rab.js`) yang diakses via `Process::run` untuk mengekstrak data dari file PDF/Excel RAB yang berat, menjaga kestabilan memory PHP.
- **AI Integration**: Menghubungkan sistem dengan provider AI (MiniMax) untuk fitur chat asisten.

### D. Operasional Lapangan
- **Media Library**: Manajemen foto progress dan berkas kontrak menggunakan Spatie Medialibrary.
- **Document Generation**: Export otomatis untuk Kontrak dan Berita Acara dalam format Word/PDF (Dompdf & PHPWord).
- **Checklist Proyek**: Fitur checklist dinamis untuk memantau status kelengkapan tiap paket pekerjaan.

---

## 3. Analisa SWOC (Strengths, Weaknesses, Opportunities, Challenges)

### Strengths (Kelebihan)
- **Separation of Concerns**: Ekstraksi AI/RAB dipisahkan ke Node.js.
- **Auditability**: Setiap perubahan data penting dicatat dalam `audit_logs`.
- **OpenAPI Ready**: Dokumentasi Swagger terintegrasi langsung dengan kode.

### Weaknesses (Kelemahan)
- **Test Coverage**: Unit & Feature tests masih sangat minim (hanya mencakup base routing).
- **Synchronous Heavy Tasks**: Beberapa proses (seperti generation dokumen) masih berjalan secara synchronous.

### Opportunities (Peluang)
- **Laravel Queues**: Memindahkan proses RAB Analyzer & Export ke background jobs.
- **Predictive Analytics**: Menggunakan data historis dari `SimulationNetwork` untuk memprediksi keterlambatan proyek.

---

## 4. Rekomendasi Strategis

### Jangka Pendek (Quick Wins)
1.  **Refactor Controller**: Memindahkan logika parsing CSV yang berat di `RABAnalyzerController` ke dalam folder `App\Services\RAB`.
2.  **Linting Automation**: Menjalankan `php artisan pint` secara rutin via Git Hook (sudah tersedia di `package.json`).
3.  **Route Cache**: Optimasi performa routing karena banyaknya endpoint yang tersedia.

### Jangka Panjang
1.  **Integration Testing**: Menambahkan testing untuk alur "Happy Path" dari proses pembuatan Kontrak hingga Berita Acara.
2.  **Notification Engine**: Mengaktifkan sistem broadcast real-time (Pusher/Soketi) untuk notifikasi pengawas lapangan.
3.  **Rate Limiting**: Menambahkan throttle khusus untuk endpoint AI Chat guna mengamankan budget token API.

---

## 5. Analisa Controller & Struktur Logic

Berdasarkan audit mendalam terhadap folder `app/Http/Controllers`, berikut adalah temuan mendetail:

### A. Core Business Orchestration (PekerjaanController)
- **Status**: Sangat Kompleks (24KB).
- **Temuan**: Implementasi **Row-Level Security (RLS)** yang matang melalui scope `byUserRole()`. Hal ini mencegah kebocoran data antar pengawas secara otomatis di level query.
- **Saran**: Ekstraksi logika Bulk Import ke `PekerjaanImportService` untuk mengurangi beban baris kode di controller.

### B. State Management & Versioning (SimulationNetworkController)
- **Status**: Advanced Logic.
- **Temuan**: Penggunaan `DB::transaction` untuk menjaga integritas history versi saat editing jaringan simulasi. Validasi data JSON yang nested dilakukan secara ketat.
- **Kelebihan**: Berhasil menerapkan pola *Restore-to-Version* yang handal untuk data non-relasional (JSON).

### C. Dokumen & Sequence (KontrakController)
- **Status**: Service-Oriented.
- **Temuan**: Penggunaan **Dependency Injection (DI)** pada `BeritaAcaraService` dan `DocumentExportService` adalah standar tertinggi yang sudah diterapkan, membuat controller tetap ramping.

### D. Hybrid Integration (RABAnalyzerController)
- **Status**: Data Bridge.
- **Temuan**: Bertindak sebagai jembatan antara PHP dan Node.js Sidecar.
- **Saran**: Logika `cleanNumber()` (pembersihan format mata uang) sebaiknya dipindahkan ke class Helpers agar konsisten digunakan di seluruh sistem.

---

*Analisa ini dihasilkan secara otomatis oleh Horizon (AI Prompt Optimizer) pada 24 April 2026.*
