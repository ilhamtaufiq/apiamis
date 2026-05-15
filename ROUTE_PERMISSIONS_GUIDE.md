# Panduan Sinkronisasi Route Permissions

Dokumen ini menjelaskan cara menggunakan perintah Artisan `app:sync-route-permissions` untuk mengelola izin akses route secara otomatis berdasarkan definisi route di Laravel.

## Deskripsi
Sistem ini menggunakan middleware `CheckRoutePermission` untuk memvalidasi akses user ke API. Perintah sinkronisasi ini membantu mendaftarkan route baru ke dalam database tanpa harus menginputnya secara manual satu per satu.

## Penggunaan Dasar

Untuk mendaftarkan route baru ke dalam sistem permission:

```bash
php artisan app:sync-route-permissions
```

### Opsi Tersedia

| Opsi | Default | Deskripsi |
|------|---------|-----------|
| `--prefix` | `api` | Prefix route yang akan dipindai (misal: `api`, `admin`). |
| `--role` | `admin` | Role default yang akan diberikan akses untuk route baru yang ditemukan. |
| `--clean` | `false` | Jika ditambahkan, akan menghapus data permission untuk route yang sudah tidak ada di kode program. |

## Contoh Skenario

### 1. Mendaftarkan Route Baru
Jika Anda baru saja menambahkan controller atau route baru di `api.php`:

```bash
php artisan app:sync-route-permissions --prefix=api --role=admin
```

### 2. Membersihkan Route Lama
Jika Anda telah menghapus beberapa route dan ingin database tetap bersih:

```bash
php artisan app:sync-route-permissions --clean
```

### 3. Mengatur Route Non-API
Jika Anda memiliki route dengan prefix lain yang juga ingin dilindungi:

```bash
php artisan app:sync-route-permissions --prefix=v1 --role=superadmin
```

## Mekanisme Kerja

1. **Pattern Matching**: Perintah ini secara otomatis mengubah parameter Laravel `{id}` menjadi format `:id` yang didukung oleh pattern matcher `RoutePermission`.
   - Contoh: `/api/blog/{blog}` -> `/api/blog/:blog`
2. **Method Awareness**: Setiap HTTP Method (`GET`, `POST`, `PUT`, `DELETE`) didaftarkan sebagai entry terpisah untuk kontrol yang lebih granular.
3. **Preservasi Data**: Jika route sudah ada di database, sistem **tidak akan** menimpa role yang sudah Anda atur secara manual di dashboard. Ia hanya akan menambahkan route yang benar-benar baru.

## Troubleshooting

- **Route Tidak Terdeteksi**: Pastikan route memiliki prefix yang sesuai dengan opsi `--prefix`.
- **Akses Ditolak Setelah Sync**: Setelah sync, pastikan role user Anda terdaftar di kolom `allowed_roles` pada tabel `route_permissions` untuk route tersebut.
