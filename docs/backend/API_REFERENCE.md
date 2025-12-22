# 📡 API Reference - APIAMIS

## 📋 Overview

Base URL:
- **Development**: `http://apiamis.test/api`
- **Production**: `https://apiamis.ilham.wtf/api`

---

## 🔐 Authentication

### Login
```http
POST /api/auth/login
Content-Type: application/json

{
  "email": "user@example.com",
  "password": "password"
}
```

**Response:**
```json
{
  "user": {
    "id": 1,
    "name": "Admin",
    "email": "admin@example.com"
  },
  "token": "1|abc123..."
}
```

### Logout
```http
POST /api/auth/logout
Authorization: Bearer {token}
```

### Get Current User
```http
GET /api/auth/me
Authorization: Bearer {token}
```

---

## 📊 Dashboard

### Get Statistics
```http
GET /api/dashboard/stats
Authorization: Bearer {token}
```

**Response:**
```json
{
  "total_kegiatan": 10,
  "total_pekerjaan": 150,
  "total_pagu": 5000000000,
  "total_kontrak": 120,
  "progress_summary": {...}
}
```

---

## 🗂️ Master Data

### Kecamatan

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/kecamatan` | List semua kecamatan |
| GET | `/api/kecamatan/{id}` | Detail kecamatan |
| POST | `/api/kecamatan` | Tambah kecamatan |
| PUT | `/api/kecamatan/{id}` | Update kecamatan |
| DELETE | `/api/kecamatan/{id}` | Hapus kecamatan |

**Request Body (POST/PUT):**
```json
{
  "nama": "Kecamatan Baru"
}
```

---

### Desa

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/desa` | List semua desa |
| GET | `/api/desa/{id}` | Detail desa |
| GET | `/api/desa/kecamatan/{kecamatanId}` | Desa by kecamatan |
| POST | `/api/desa` | Tambah desa |
| PUT | `/api/desa/{id}` | Update desa |
| DELETE | `/api/desa/{id}` | Hapus desa |

**Request Body (POST/PUT):**
```json
{
  "nama": "Desa Baru",
  "kecamatan_id": 1
}
```

---

### Penyedia

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/penyedia` | List semua penyedia |
| GET | `/api/penyedia/{id}` | Detail penyedia |
| POST | `/api/penyedia` | Tambah penyedia |
| PUT | `/api/penyedia/{id}` | Update penyedia |
| DELETE | `/api/penyedia/{id}` | Hapus penyedia |

**Request Body (POST/PUT):**
```json
{
  "nama": "CV. Penyedia Baru",
  "alamat": "Jl. Contoh No. 1",
  "telepon": "08123456789"
}
```

---

## 📋 Kegiatan (Program)

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/kegiatan` | List semua kegiatan |
| GET | `/api/kegiatan/{id}` | Detail kegiatan |
| GET | `/api/kegiatan/tahun/{tahun}` | Kegiatan by tahun |
| POST | `/api/kegiatan` | Tambah kegiatan |
| PUT | `/api/kegiatan/{id}` | Update kegiatan |
| DELETE | `/api/kegiatan/{id}` | Hapus kegiatan |

**Request Body (POST/PUT):**
```json
{
  "nama": "Kegiatan Baru",
  "tahun": 2024,
  "pagu": 1000000000
}
```

---

## 🔨 Pekerjaan

### CRUD

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/pekerjaan` | List pekerjaan (filtered by role) |
| GET | `/api/pekerjaan/{id}` | Detail pekerjaan |
| GET | `/api/pekerjaan/{id}/media` | Media pekerjaan |
| POST | `/api/pekerjaan` | Tambah pekerjaan |
| PUT | `/api/pekerjaan/{id}` | Update pekerjaan |
| DELETE | `/api/pekerjaan/{id}` | Hapus pekerjaan |

### Filter Routes

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/pekerjaan/kecamatan/{id}` | By kecamatan |
| GET | `/api/pekerjaan/desa/{id}` | By desa |
| GET | `/api/pekerjaan/kegiatan/{id}` | By kegiatan |
| GET | `/api/pekerjaan/kecamatan/{kecId}/desa/{desaId}` | By kecamatan & desa |
| GET | `/api/pekerjaan/stats/pagu-kecamatan/{id}` | Total pagu by kecamatan |
| GET | `/api/pekerjaan/stats/pagu-kegiatan/{id}` | Total pagu by kegiatan |

**Request Body (POST/PUT):**
```json
{
  "nama": "Pembangunan SAB",
  "kegiatan_id": 1,
  "kecamatan_id": 1,
  "desa_id": 1,
  "pagu": 500000000,
  "lokasi": "RT 01 RW 02",
  "koordinat_lat": "-7.12345",
  "koordinat_lng": "110.12345"
}
```

---

## 📝 Kontrak

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/kontrak` | List semua kontrak |
| GET | `/api/kontrak/{id}` | Detail kontrak |
| GET | `/api/kontrak/pekerjaan/{id}` | By pekerjaan |
| GET | `/api/kontrak/kegiatan/{id}` | By kegiatan |
| GET | `/api/kontrak/penyedia/{id}` | By penyedia |
| POST | `/api/kontrak` | Tambah kontrak |
| PUT | `/api/kontrak/{id}` | Update kontrak |
| DELETE | `/api/kontrak/{id}` | Hapus kontrak |

**Request Body (POST/PUT):**
```json
{
  "pekerjaan_id": 1,
  "penyedia_id": 1,
  "nomor_kontrak": "SPK/001/2024",
  "tanggal_kontrak": "2024-01-15",
  "nilai_kontrak": 450000000,
  "tanggal_mulai": "2024-02-01",
  "tanggal_selesai": "2024-05-31"
}
```

---

## 📦 Output

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/output` | List semua output |
| GET | `/api/output/{id}` | Detail output |
| POST | `/api/output` | Tambah output |
| PUT | `/api/output/{id}` | Update output |
| DELETE | `/api/output/{id}` | Hapus output |

**Request Body (POST/PUT):**
```json
{
  "pekerjaan_id": 1,
  "nama": "SR Tersambung",
  "satuan": "SR",
  "volume": 100
}
```

---

## 👥 Penerima Manfaat

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/penerima` | List semua penerima |
| GET | `/api/penerima/{id}` | Detail penerima |
| GET | `/api/penerima/pekerjaan/{id}` | By pekerjaan |
| GET | `/api/penerima/pekerjaan/{id}/stats/komunal` | Statistik komunal |
| POST | `/api/penerima` | Tambah penerima |
| PUT | `/api/penerima/{id}` | Update penerima |
| DELETE | `/api/penerima/{id}` | Hapus penerima |

**Request Body (POST/PUT):**
```json
{
  "pekerjaan_id": 1,
  "nama": "Budi Santoso",
  "nik": "3302012345678901",
  "alamat": "RT 01 RW 02",
  "jenis": "individual"
}
```

---

## 📸 Foto Dokumentasi

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/foto` | List semua foto |
| GET | `/api/foto/{id}` | Detail foto |
| POST | `/api/foto` | Upload foto |
| PUT | `/api/foto/{id}` | Update foto |
| DELETE | `/api/foto/{id}` | Hapus foto |

**Request Body (POST - multipart/form-data):**
```
pekerjaan_id: 1
jenis: progress
keterangan: Foto progress 50%
file: [binary file]
koordinat_lat: -7.12345
koordinat_lng: 110.12345
```

---

## 📄 Berkas/Dokumen

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/berkas` | List semua berkas |
| GET | `/api/berkas/{id}` | Detail berkas |
| POST | `/api/berkas` | Upload berkas |
| PUT | `/api/berkas/{id}` | Update berkas |
| DELETE | `/api/berkas/{id}` | Hapus berkas |

---

## 📈 Progress

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/progress/pekerjaan/{id}` | Laporan progress |
| POST | `/api/progress/pekerjaan/{id}` | Update progress |

**Request Body (POST):**
```json
{
  "tanggal": "2024-03-15",
  "fisik": 45.5,
  "keuangan": 40.0,
  "keterangan": "Target minggu ini tercapai"
}
```

---

## 📜 Berita Acara

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/berita-acara/pekerjaan/{id}` | Get berita acara |
| POST | `/api/berita-acara/pekerjaan/{id}` | Create/Update berita acara |

---

## 👤 User Management

### Users
| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/users` | List users |
| GET | `/api/users/{id}` | Detail user |
| POST | `/api/users` | Tambah user |
| PUT | `/api/users/{id}` | Update user |
| DELETE | `/api/users/{id}` | Hapus user |

### Roles
| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/roles` | List roles |
| GET | `/api/roles/{id}` | Detail role |
| POST | `/api/roles` | Tambah role |
| PUT | `/api/roles/{id}` | Update role |
| DELETE | `/api/roles/{id}` | Hapus role |

### Permissions
| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/permissions` | List permissions |
| GET | `/api/permissions/{id}` | Detail permission |
| POST | `/api/permissions` | Tambah permission |
| PUT | `/api/permissions/{id}` | Update permission |
| DELETE | `/api/permissions/{id}` | Hapus permission |

---

## 🔒 Route Permissions

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/route-permissions` | List route permissions |
| GET | `/api/route-permissions/rules` | Get permission rules |
| GET | `/api/route-permissions/user/accessible` | User accessible routes |
| POST | `/api/route-permissions/check-access` | Check route access |

---

## 📱 Menu Permissions

| Method | Endpoint | Deskripsi |
|--------|----------|-----------|
| GET | `/api/menu-permissions` | List menu permissions |
| GET | `/api/menu-permissions/user/menus` | User menus |

---

## ⚙️ App Settings

| Method | Endpoint | Auth | Deskripsi |
|--------|----------|------|-----------|
| GET | `/api/app-settings` | ❌ | Get app settings (public) |
| POST | `/api/app-settings` | ✅ | Update app settings |

---

## 📦 Response Format

### Success Response
```json
{
  "data": {...},
  "message": "Success"
}
```

### List Response (Paginated)
```json
{
  "data": [...],
  "meta": {
    "current_page": 1,
    "last_page": 10,
    "per_page": 15,
    "total": 150
  },
  "links": {
    "first": "...",
    "last": "...",
    "next": "...",
    "prev": null
  }
}
```

### Error Response
```json
{
  "message": "Error message",
  "errors": {
    "field": ["Validation error"]
  }
}
```

---

## 🔑 HTTP Status Codes

| Code | Deskripsi |
|------|-----------|
| 200 | OK - Request berhasil |
| 201 | Created - Resource berhasil dibuat |
| 204 | No Content - Delete berhasil |
| 400 | Bad Request - Input tidak valid |
| 401 | Unauthorized - Tidak terautentikasi |
| 403 | Forbidden - Tidak memiliki akses |
| 404 | Not Found - Resource tidak ditemukan |
| 422 | Unprocessable Entity - Validasi gagal |
| 500 | Internal Server Error |

---

## 📚 Related Documentation

- [Architecture](./ARCHITECTURE.md)
- [Database Schema](./DATABASE.md)
- [Installation Guide](./INSTALLATION.md)
