# Route Permissions

Dokumentasi tentang sistem route permission di APIAMIS dan ARUMANIS.

## Overview

Sistem route permission mengontrol akses ke halaman dan API berdasarkan role user. Sistem ini bekerja di dua layer:

1. **Backend Middleware** (`CheckRoutePermission.php`) - Mengontrol akses API
2. **Frontend Component** (`ProtectedRoute.tsx`) - Mengontrol akses halaman

## Cara Kerja

```
┌─────────────────────────────────────────────────────────────┐
│                    User Request                             │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│  1. Apakah user adalah ADMIN?                               │
│     ✅ YA → Izinkan akses ke semua route                    │
│     ❌ TIDAK → Lanjut ke pengecekan berikutnya              │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│  2. Apakah ada RULE di database untuk route ini?            │
│     ✅ ADA → Cek apakah user memiliki role yang diizinkan   │
│     ❌ TIDAK → Lanjut ke pengecekan berikutnya              │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│  3. Apakah route ini ada di ADMIN_ONLY_ROUTES?              │
│     ✅ YA → Tolak akses (403)                               │
│     ❌ TIDAK → Izinkan akses                                │
└─────────────────────────────────────────────────────────────┘
```

## Admin Only Routes

### Frontend (ProtectedRoute.tsx)

Routes yang hanya bisa diakses admin (halaman):

```typescript
const ADMIN_ONLY_ROUTES = [
    '/kegiatan',
    '/desa',
    '/kecamatan',
    '/kontrak',
    '/output',
    '/penerima',
    '/users',
    '/roles',
    '/permissions',
    '/route-permissions',
    '/menu-permissions',
    '/kegiatan-role',
    '/berkas',
    '/settings',
];
```

### Backend (CheckRoutePermission.php)

Routes yang hanya bisa diakses admin (API management):

```php
private const ADMIN_ONLY_ROUTES = [
    '/users',
    '/roles',
    '/permissions',
    '/route-permissions',
    '/menu-permissions',
];
```

> **PENTING**: Backend TIDAK membatasi route data-fetching seperti `/penyedia`, `/kegiatan`, dll. karena route ini diperlukan oleh komponen untuk mengambil data dropdown.

## Database Route Permissions

Route permissions juga bisa dikonfigurasi melalui database (tabel `route_permissions`):

| Column | Type | Keterangan |
|--------|------|------------|
| `route_path` | string | Path route (contoh: `/pekerjaan/:id`) |
| `route_method` | enum | HTTP method: GET, POST, PUT, PATCH, DELETE |
| `allowed_roles` | json array | Roles yang diizinkan: `["tfl", "admin"]` |
| `is_active` | boolean | Aktif/nonaktif permission |
| `description` | string | Deskripsi permission |

### Contoh Konfigurasi Database

```sql
-- Izinkan role 'tfl' mengakses halaman pekerjaan
INSERT INTO route_permissions (route_path, route_method, allowed_roles, is_active) VALUES
('/pekerjaan', 'GET', '["tfl", "admin"]', 1),
('/pekerjaan/:id', 'GET', '["tfl", "admin"]', 1);
```

## Prioritas Permission

1. **Admin** → Selalu diizinkan
2. **Database Rule** → Jika ada rule, gunakan role dari database
3. **ADMIN_ONLY_ROUTES** → Jika tidak ada rule dan route ada di list, tolak non-admin
4. **Default** → Izinkan akses

## Menambah Permission Baru

### Via Admin Panel
1. Login sebagai admin
2. Akses `/route-permissions`
3. Klik "Tambah"
4. Isi form:
   - Route Path: `/contoh-route`
   - Route Method: `GET`
   - Allowed Roles: pilih roles yang diizinkan
   - Is Active: centang

### Via Code

**Frontend** - Tambahkan ke `ADMIN_ONLY_ROUTES` di `ProtectedRoute.tsx`:
```typescript
const ADMIN_ONLY_ROUTES = [
    // ... existing routes
    '/new-admin-only-route',
];
```

**Backend** - Tambahkan ke `ADMIN_ONLY_ROUTES` di `CheckRoutePermission.php`:
```php
private const ADMIN_ONLY_ROUTES = [
    // ... existing routes
    '/new-admin-only-route',
];
```

## Whitelist Routes

Routes yang selalu bisa diakses (backend):

```php
$whitelistedRoutes = [
    '/auth/me',
    '/auth/logout',
    '/menu-permissions/user/menus',
    '/route-permissions/rules',
    '/route-permissions/user/accessible',
    '/dashboard/stats',
    '/app-settings',
];
```

## Troubleshooting

### User tidak bisa akses halaman
1. Cek apakah user memiliki role yang sesuai
2. Cek apakah ada rule di database untuk route tersebut
3. Cek apakah route ada di `ADMIN_ONLY_ROUTES`

### Tab content tidak tampil
1. Pastikan API endpoint tidak ada di backend `ADMIN_ONLY_ROUTES`
2. Cek apakah ada rule di database yang membatasi akses

### Error "Route ini hanya dapat diakses oleh admin"
- Route ada di `ADMIN_ONLY_ROUTES` dan user bukan admin
- Solusi: Tambahkan rule di database dengan role yang sesuai, atau hapus dari `ADMIN_ONLY_ROUTES`
