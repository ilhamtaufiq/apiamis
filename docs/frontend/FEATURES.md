# ✨ Fitur Aplikasi - ARUMANIS

## 📋 Overview

ARUMANIS menyediakan berbagai fitur untuk manajemen proyek infrastruktur Air Minum dan Sanitasi. Berikut adalah daftar lengkap fitur yang tersedia.

---

## 🔐 Authentication

### Login
- Login dengan email dan password
- Integrasi dengan Laravel Sanctum
- Remember me functionality
- Session management

### User Profile
- Lihat informasi profil
- Update profil
- Ganti password

---

## 📊 Dashboard

### Statistik Utama
- Total kegiatan
- Total pekerjaan
- Total pagu anggaran
- Total kontrak
- Progress keseluruhan

### Visualisasi
- Chart progress fisik vs keuangan
- Chart pagu per kecamatan
- Chart pekerjaan per kegiatan
- Peta sebaran lokasi pekerjaan

### Quick Actions
- Akses cepat ke fitur utama
- Shortcut menu

---

## 🗂️ Master Data

### Kecamatan
- CRUD data kecamatan
- List semua kecamatan
- Filter dan search

### Desa
- CRUD data desa
- Filter berdasarkan kecamatan
- Relasi dengan kecamatan

### Penyedia/Vendor
- CRUD data penyedia
- Informasi kontak
- Riwayat kontrak

---

## 📋 Kegiatan (Program)

### Manajemen Kegiatan
- CRUD data kegiatan
- Pengaturan tahun anggaran
- Pengaturan pagu kegiatan
- Filter berdasarkan tahun

### Kegiatan-Role
- Assign kegiatan ke role
- Filtering pekerjaan berdasarkan role
- Admin only feature

---

## 🔨 Pekerjaan

### Manajemen Pekerjaan
- CRUD data pekerjaan
- Filter berdasarkan kegiatan, kecamatan, desa
- Search pekerjaan
- Pagination

### Detail Pekerjaan (Tabs)

#### Tab Kontrak
- Informasi kontrak/SPK
- Nomor kontrak, tanggal, nilai
- Data penyedia
- Kode RUP dan Kode Paket SPSE
- Import data dari SPSE

#### Tab Output
- CRUD output pekerjaan
- Nama output, satuan, volume
- Rekapitulasi output

#### Tab Penerima
- CRUD penerima manfaat
- Data NIK, nama, alamat
- Jenis penerima (individual/komunal)
- Statistik penerima

#### Tab Foto
- Upload foto dokumentasi
- Jenis foto (0%, 50%, 100%)
- Metadata koordinat GPS
- Keterangan foto
- Preview foto

#### Tab Berkas
- Upload dokumen
- Jenis berkas
- Download berkas

#### Tab Progress
- Input progress fisik dan keuangan
- Grafik progress timeline
- Export laporan progress
- Input DPA (Daftar Pelaksanaan Anggaran)

#### Tab Berita Acara
- CRUD berita acara
- Template berita acara
- Export berita acara

---

## 📸 Foto Dokumentasi

### Upload Foto
- Multiple file upload
- Auto-resize image
- Extract GPS metadata
- Assign ke pekerjaan

### Galeri Foto
- Grid view foto
- Filter berdasarkan pekerjaan
- Filter berdasarkan jenis foto
- Preview modal

### Export
- Export tabel foto ke PDF
- Include peta lokasi
- Customizable layout

---

## 📄 Berkas/Dokumen

### Manajemen Dokumen
- Upload dokumen (PDF, Word, Excel)
- Kategorisasi dokumen
- Download dokumen
- Preview dokumen

---

## 📈 Laporan Progress

### Input Progress
- Progress fisik (%)
- Progress keuangan (%)
- Tanggal progress
- Keterangan

### Visualisasi
- Line chart progress over time
- Comparison fisik vs keuangan
- Trend analysis

### Export
- Export ke PDF
- Export ke Excel
- Include chart dan data

---

## 👥 User Management

### Users
- CRUD user
- Assign role
- Reset password
- Active/inactive status

### Roles
- CRUD role
- Assign permissions
- Default roles: admin, user

### Permissions
- List permissions
- Assign ke role
- Fine-grained access control

---

## 🔒 Access Control

### Route Permissions
- Define accessible routes per role
- Dynamic route protection
- Default allow/deny

### Menu Permissions
- Define visible menus per role
- Menu ordering
- Icon customization
- Parent-child menu

---

## ⚙️ Settings

### App Settings
- Nama aplikasi
- Logo aplikasi
- Informasi organisasi
- Konfigurasi umum

### Theme
- Light/Dark mode toggle
- Persistent preference
- System preference detection

---

## 📱 Responsive Design

### Desktop
- Full sidebar navigation
- Multi-column layout
- Extended data tables

### Tablet
- Collapsible sidebar
- Adaptive layout
- Touch-friendly controls

### Mobile
- Bottom navigation
- Single column layout
- Swipe gestures
- Pull to refresh

---

## 🎨 UI/UX Features

### Dark Mode
- Toggle via header
- System preference sync
- Persistent storage

### Toast Notifications
- Success messages
- Error messages
- Info notifications
- Auto dismiss

### Loading States
- Skeleton loaders
- Spinner indicators
- Progressive loading

### Form Validation
- Real-time validation
- Error highlighting
- Helper text
- Required field indicators

---

## 📤 Export Features

### PDF Export
- Laporan progress
- Tabel foto
- Berita acara
- Custom header/footer

### Excel Export
- Data pekerjaan
- Laporan progress
- Data penerima

---

## 🔗 Integrasi

### SPSE INAPROC
- Import data kontrak dari SPSE
- Fetch kode RUP
- Fetch data pemenang

### Media Library
- Spatie MediaLibrary integration
- Image optimization
- Multiple collections

---

## 🛡️ Security Features

### Authentication
- Secure session management
- CSRF protection
- Token-based API auth

### Authorization
- Role-based access control
- Route protection
- UI element visibility

### Data Protection
- Input sanitization
- XSS prevention
- SQL injection prevention

---

## 📚 Related Documentation

- [Architecture](./ARCHITECTURE.md)
- [Components](./COMPONENTS.md)
- [Installation Guide](./INSTALLATION.md)
