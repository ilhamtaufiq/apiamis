# Rencana Implementasi API Publik APIAMIS

Dokumen ini merangkum rencana menjadikan APIAMIS sebagai penyedia API publik yang bisa diakses pihak luar menggunakan API key, tanpa login user frontend.

## Tujuan

- Menyediakan endpoint publik yang stabil dengan autentikasi API key.
- Memisahkan kontrak API publik dari endpoint internal Arumanis.
- Menjaga data sensitif, permission internal, dan aturan visibility `Pekerjaan` tidak terbuka tanpa kontrol.
- Menyediakan manajemen API key untuk admin melalui backend APIAMIS dan frontend Arumanis.

## Repo Terkait

- Backend APIAMIS: `/mnt/c/laragon/www/apiamis`
- Frontend Arumanis: `/mnt/c/laragon/www/bun`

Untuk perubahan kontrak endpoint admin atau UI pengelolaan API key, cek frontend Arumanis sebelum menetapkan bentuk response final.

## Prinsip Desain

API publik harus menjadi lapisan terpisah dari API internal yang sekarang memakai Sanctum.

Route publik disarankan memakai prefix:

```text
/api/public/v1/*
```

Autentikasi disarankan memakai header:

```http
X-API-Key: amis_live_xxxxxxxxx
```

Opsional dapat didukung juga:

```http
Authorization: ApiKey amis_live_xxxxxxxxx
```

Sanctum tetap dipakai untuk frontend internal dan admin. API key publik memiliki lifecycle sendiri: generate, revoke, rotate, scope, expiry, quota, rate limit, dan audit.

## Endpoint Publik Tahap Awal

Mulai dari endpoint read-only yang aman:

```text
GET /api/public/v1/health
GET /api/public/v1/kecamatan
GET /api/public/v1/desa
GET /api/public/v1/kegiatan
GET /api/public/v1/pekerjaan
GET /api/public/v1/pekerjaan/{id}
GET /api/public/v1/progress/pekerjaan/{id}
```

Endpoint berikut tidak dibuka pada tahap awal:

- create/update/delete data
- upload berkas/foto
- export dokumen
- user, role, permission
- audit log
- backup/restore
- WhatsApp bridge
- debug endpoint
- endpoint AI/chat internal

## Model API Key

Buat tabel baru, misalnya `public_api_clients`.

Kolom yang disarankan:

```text
id
name
owner_name
owner_email
key_prefix
key_hash
scopes JSON
rate_limit_per_minute
allowed_ips JSON nullable
is_active boolean
last_used_at nullable
expires_at nullable
created_by nullable
created_at
updated_at
```

Aturan keamanan:

- Plain API key hanya ditampilkan sekali saat dibuat atau di-rotate.
- Database hanya menyimpan hash API key.
- `key_prefix` disimpan untuk lookup cepat dan identifikasi admin.
- Verifikasi hash memakai perbandingan timing-safe seperti `hash_equals`.
- Key yang revoked, inactive, atau expired harus ditolak.

Contoh format key:

```text
amis_live_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
amis_test_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
```

Contoh scope:

```json
[
  "kecamatan:read",
  "desa:read",
  "kegiatan:read",
  "pekerjaan:read",
  "progress:read"
]
```

## Middleware API Key

Buat middleware:

```text
app/Http/Middleware/AuthenticatePublicApiKey.php
```

Tanggung jawab middleware:

1. Membaca API key dari `X-API-Key` atau `Authorization: ApiKey ...`.
2. Validasi format key.
3. Lookup client berdasarkan `key_prefix`.
4. Cocokkan hash API key.
5. Pastikan client aktif.
6. Pastikan key belum expired.
7. Cek allowed IP jika dikonfigurasi.
8. Inject client ke request, misalnya `public_api_client`.
9. Update `last_used_at`.
10. Return error JSON konsisten.

Daftarkan alias middleware di `bootstrap/app.php`, misalnya:

```php
'public.api_key' => \App\Http\Middleware\AuthenticatePublicApiKey::class,
```

Karena `check.route.permission` saat ini dipasang pada seluruh API route, tambahkan pengecualian eksplisit untuk path `api/public/*` agar request publik tidak bercampur dengan permission internal.

## Routing

Route publik dapat ditambahkan di `routes/api.php` atau dipisahkan ke file baru `routes/public_api.php`.

Contoh:

```php
Route::prefix('public/v1')
    ->middleware(['public.api_key', 'throttle:public-api'])
    ->group(function () {
        Route::get('health', [PublicHealthController::class, 'show']);
        Route::get('kecamatan', [PublicKecamatanController::class, 'index']);
        Route::get('desa', [PublicDesaController::class, 'index']);
        Route::get('kegiatan', [PublicKegiatanController::class, 'index']);
        Route::get('pekerjaan', [PublicPekerjaanController::class, 'index']);
        Route::get('pekerjaan/{pekerjaan}', [PublicPekerjaanController::class, 'show']);
        Route::get('progress/pekerjaan/{pekerjaan}', [PublicProgressController::class, 'showByPekerjaan']);
    });
```

## Public Resources

Jangan memakai resource internal secara langsung untuk API publik bila field-nya belum diaudit.

Buat resource khusus:

```text
app/Http/Resources/Public/KecamatanPublicResource.php
app/Http/Resources/Public/DesaPublicResource.php
app/Http/Resources/Public/KegiatanPublicResource.php
app/Http/Resources/Public/PekerjaanPublicResource.php
app/Http/Resources/Public/ProgressPublicResource.php
```

Resource publik hanya memuat field yang memang aman dan stabil.

Contoh field `Pekerjaan` publik:

```text
id
kode_rekening
nama_paket
kecamatan
desa
kegiatan
pagu
progress
published_at
updated_at
```

Field yang perlu diaudit sebelum dibuka:

- data penerima manfaat detail
- NIP pengawas/pendamping
- nomor kontak
- dokumen/berkas internal
- lokasi presisi jika dianggap sensitif
- audit metadata
- user assignment

## Aturan Publikasi Data

Jangan memakai `Pekerjaan::byUserRole()` untuk API publik karena scope itu bergantung pada user login internal.

Tambahkan aturan publikasi eksplisit, misalnya:

```text
is_public boolean default false
published_at nullable
```

Query publik hanya mengambil data yang sudah dipublikasikan:

```php
Pekerjaan::query()
    ->where('is_public', true)
    ->whereNotNull('published_at');
```

Dengan pendekatan ini, default semua data tidak terbuka sampai admin memutuskan data tersebut layak dipublikasikan.

## Scope dan Authorization API Key

Setiap route publik harus mendefinisikan scope minimal.

Contoh:

```text
GET /api/public/v1/pekerjaan              pekerjaan:read
GET /api/public/v1/pekerjaan/{id}         pekerjaan:read
GET /api/public/v1/progress/pekerjaan/{id} progress:read
GET /api/public/v1/kecamatan              kecamatan:read
GET /api/public/v1/desa                   desa:read
GET /api/public/v1/kegiatan               kegiatan:read
```

Scope bisa dicek di middleware tambahan, misalnya:

```text
public.scope:pekerjaan:read
```

## Rate Limit dan Quota

Buat rate limiter khusus:

```text
public-api
```

Default awal:

```text
60 request per menit per API key
```

Jika `public_api_clients.rate_limit_per_minute` diisi, gunakan nilai dari client.

Header response yang disarankan:

```http
X-RateLimit-Limit: 60
X-RateLimit-Remaining: 59
Retry-After: 30
```

## Audit dan Usage Log

Buat tabel usage log atau agregasi usage.

Opsi log detail:

```text
public_api_request_logs
id
public_api_client_id
method
path
status_code
duration_ms
ip_address
user_agent
created_at
```

Untuk traffic besar, gunakan agregasi harian:

```text
public_api_daily_usages
public_api_client_id
date
total_requests
total_errors
```

Tahap awal cukup log detail dengan retention period yang jelas.

## Endpoint Admin untuk Manajemen API Key

Endpoint ini bukan API publik. Endpoint ini tetap memakai `auth:sanctum` dan `role:admin`.

```text
GET    /api/public-api-clients
POST   /api/public-api-clients
GET    /api/public-api-clients/{id}
PATCH  /api/public-api-clients/{id}
POST   /api/public-api-clients/{id}/rotate-key
POST   /api/public-api-clients/{id}/revoke
DELETE /api/public-api-clients/{id}
GET    /api/public-api-clients/{id}/usage
```

Controller:

```text
app/Http/Controllers/PublicApiClientController.php
```

Fitur minimal admin:

- membuat client baru
- menampilkan plaintext API key sekali setelah create/rotate
- mengatur nama, owner, scopes, expiry, allowed IP, dan rate limit
- revoke API key
- rotate API key
- aktif/nonaktif client
- melihat `last_used_at`
- melihat usage log

## Integrasi Frontend Arumanis

Repo frontend berada di:

```text
/mnt/c/laragon/www/bun
```

UI admin yang dibutuhkan:

- halaman daftar API clients
- form create API client
- detail API client
- tombol rotate key
- tombol revoke/nonaktifkan
- pengaturan scopes
- pengaturan expiry dan allowed IP
- tampilan last used dan usage
- modal sekali tampil untuk plaintext API key setelah dibuat/rotate

Saat implementasi frontend:

- ikuti pola API client existing di Arumanis
- cek struktur routing dan permission menu
- tambahkan type response yang sesuai
- jangan menyimpan plaintext API key setelah modal ditutup

## Format Error Publik

Gunakan format error konsisten:

```json
{
  "message": "API key tidak valid."
}
```

Contoh status:

```text
401 API key kosong atau tidak valid
403 API key tidak aktif, expired, IP tidak diizinkan, atau scope kurang
404 resource tidak ditemukan atau tidak dipublikasikan
422 parameter query tidak valid
429 rate limit terlampaui
500 error server
```

## Dokumentasi OpenAPI

Tambahkan dokumentasi Swagger/L5-Swagger untuk:

- cara autentikasi API key
- daftar endpoint publik
- daftar scope
- pagination
- filter query
- contoh request/response
- error response
- rate limit

Security scheme:

```yaml
type: apiKey
in: header
name: X-API-Key
```

## Testing

Feature test yang disarankan:

```text
tests/Feature/PublicApi/PublicApiKeyAuthenticationTest.php
tests/Feature/PublicApi/PublicApiScopeTest.php
tests/Feature/PublicApi/PublicPekerjaanApiTest.php
tests/Feature/PublicApi/PublicApiRateLimitTest.php
tests/Feature/PublicApi/PublicApiClientManagementTest.php
```

Kasus wajib:

- request tanpa API key menghasilkan `401`
- API key salah menghasilkan `401`
- API key inactive menghasilkan `403`
- API key expired menghasilkan `403`
- API key tanpa scope menghasilkan `403`
- allowed IP ditolak menghasilkan `403`
- data non-public tidak muncul
- detail non-public menghasilkan `404`
- response publik tidak memuat field sensitif
- pagination stabil
- admin bisa create, rotate, revoke API key
- plaintext key hanya muncul pada create/rotate

## Urutan Implementasi

1. Buat migration, model, factory untuk `PublicApiClient`.
2. Buat service generator dan verifier API key.
3. Buat middleware `AuthenticatePublicApiKey`.
4. Tambah pengecualian `api/public/*` dari route permission internal bila perlu.
5. Buat route group `/api/public/v1`.
6. Buat public controllers read-only.
7. Buat public resources.
8. Tambah field publikasi pada model yang akan dibuka, terutama `Pekerjaan`.
9. Buat admin controller untuk manajemen API key.
10. Tambah rate limiter `public-api`.
11. Tambah usage log.
12. Tambah feature tests.
13. Update dokumentasi OpenAPI.
14. Implementasikan UI admin di frontend Arumanis.

## Risiko dan Mitigasi

| Risiko | Mitigasi |
| --- | --- |
| Data internal bocor lewat resource existing | Gunakan resource publik khusus |
| Endpoint internal tidak sengaja terbuka | Pisahkan prefix `/api/public/v1` dan middleware khusus |
| API key bocor dari database | Simpan hash, bukan plaintext |
| Abuse traffic | Rate limit per API key dan usage log |
| Data `Pekerjaan` melewati visibility internal | Gunakan flag publikasi eksplisit |
| Kontrak API berubah karena frontend internal | Pisahkan controller/resource publik |
| Admin kehilangan key setelah create | Tampilkan plaintext sekali dan sediakan rotate |

## Keputusan Awal yang Direkomendasikan

- API publik read-only pada versi pertama.
- API key dedicated, bukan token login Sanctum.
- Data publik harus opt-in melalui `is_public` dan `published_at`.
- Resource publik dibuat terpisah dari resource internal.
- Admin API key management tetap lewat Sanctum dan role admin.
- Frontend admin dibuat di `/mnt/c/laragon/www/bun` setelah endpoint backend stabil.
