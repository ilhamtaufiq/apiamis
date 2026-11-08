# Kunci API untuk layanan (`/api/public/v1/*`)

Untuk layanan lain yang membaca data apiamis tanpa akun pengguna, mis. arsgis. Terpisah dari token Sanctum:
tidak punya peran pengguna, tidak kedaluwarsa kecuali diatur, bisa dicabut kapan saja, dan hanya bisa membaca
endpoint sesuai scope-nya. Rencana yang lebih luas ada di `rencana_api_publik.md`; ini irisan kecilnya.

## Membuat, melihat, mencabut

Dijalankan di server apiamis (butuh `DATABASE_URL`). Migrasi `0016_public_api_keys` harus sudah berjalan: `apiamis-api migrate`.

```bash
apiamis-api create-api-key --name=arsgis --scopes=spam-units:read,peripaan:read
apiamis-api create-api-key --name=arsgis --scopes=spam-units:read --expires-days=365
apiamis-api list-api-keys
apiamis-api revoke-api-key --id=3
```

Kunci berbentuk `amis_live_` diikuti 64 karakter heksadesimal. **Dicetak sekali saat dibuat.** Yang tersimpan hanya
hash SHA-256 dan 6 karakter awalan untuk dikenali di daftar, jadi kunci yang hilang tidak bisa dipulihkan: cabut dan buat baru.

## Memakai

Kirim di header `X-API-Key`. Jangan taruh di URL.

```bash
curl -H "X-API-Key: amis_live_..." "https://apiamis.cianjur.space/api/public/v1/spam-units?per_page=200"
```

| Endpoint | Scope | Isi |
| --- | --- | --- |
| `GET /api/public/v1/spam-units` | `spam-units:read` | Sama dengan `GET /api/spam-units` (termasuk pengelola/Pokmas) |
| `GET /api/public/v1/peripaan` | `peripaan:read` | Daftar peta perpipaan per pekerjaan, ditambah `unit_spam_ids`. Tanpa nama pengunggah |

- `per_page` dijepit: paling banyak 200 (50 bila `include=geojson`). `-1` (semua) tidak berlaku di sini.
- Berkas KMZ/KML diambil lewat `file_url` pada daftar peripaan.
- Respons 401 untuk kunci yang tidak ada, salah, kedaluwarsa, atau dicabut (sengaja tidak dibedakan). 403 bila kunci sah tetapi tidak punya scope.

## Catatan keamanan

- Kunci `spam-units:read` melihat data yang sama dengan akun pengguna biasa pada rute internal, termasuk nama pengelola.
  Simpan seperti kata sandi (variabel lingkungan, bukan git), dan pakai kunci terpisah per layanan.
- Belum ada pembatasan laju (rate limit) dan belum ada daftar IP yang diizinkan. Keduanya ada di rencana, belum dibangun.
- `last_used_at` diperbarui paling sering sekali per 5 menit, untuk melihat kunci yang sudah tidak dipakai.
- Menambah endpoint publik baru: tambahkan scope di `SCOPES` (`crates/api/src/public_key.rs`) dan periksa scope itu di handler.
