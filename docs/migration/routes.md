# Inventaris Route API (Fase 0.1)

Sumber: `routes/api.php` (branch `rust`, dari `main` SHA `c19fd06`). Dibuat dengan parser statis `routes_inv.py`, karena `vendor/` tidak ada dan `php artisan route:list` tidak bisa dijalankan. Perlu diverifikasi terhadap `php artisan route:list --json` di environment yang punya `vendor/`.

## Ringkasan

- Total route (termasuk hasil expand `apiResource`): **487**
- Dilindungi `auth:sanctum`: **458**
- Dengan `role:admin`: **79**
- Tanpa `auth:sanctum` (publik / OAuth / webhook): **28**
- Jumlah grup modul (segmen pertama setelah `/api/`): **71**

### Status paritas (diperbarui)

- Sudah di Rust: **221** (termasuk dokumen kontrak SPK, cover, ZIP cover, BAP, dan bap-context, serta PDF SPK lewat ONLYOFFICE; termasuk auth, lookup, kecamatan, desa, kegiatan, penyedia, tiket GET, checklist GET, foto dan penerima seluruh rute, berkas kecuali export-pdf dan upload-from-url, kontrak CRUD, relasi pekerjaan/kegiatan/penyedia, excel export, template, dan impor kontrak, addendum dan register-gaps GET, sk, notifikasi, dan master fase pekerjaan)
- Parsial di Rust: **2** (`GET /api/pekerjaan`: relasi daftar belum dibandingkan dengan produksi, lihat T21)
- Belum: **187** (dihitung langsung dari kolom Status; termasuk addendum, export Excel draft, progress estimasi, destroy pekerjaan, dan modul lain yang belum dipindah)
- Dihapus (tidak dimigrasi): **77** (termasuk desa sync-kk, chat AI dan live-chat, panduan CMS, presence, search ai-summary, dan pengaturan AI di app-settings, sesuai keputusan user)

Cara menghitung: cocokkan method dan path (`{param}` dinormalisasi) dengan route yang terdaftar di `rust/crates/api/src/lib.rs`.

## Temuan

1. **`GET /api/debug-data`** (`routes/api.php` baris 471): endpoint debug yang mengembalikan data mentah `tbl_kegiatan` dan `tbl_pekerjaan` serta relasi Eloquent. Hanya `auth:sanctum`, tanpa `role:admin`, dan melewati scope `byUserRole()`. Setiap user yang login bisa membaca semua pekerjaan. **Rekomendasi: hapus, jangan dipindah ke Rust.**
2. **`GET /api/`** dan **`GET /api/user`** memakai closure. `/api/` masih mengembalikan `view('welcome')`, jadi perlu dicek apakah ada view-nya.
3. **Route Blade di `routes/web.php`** (`GET /`) memberi info service dan `docs` Swagger. Tidak termasuk API, tapi perlu dipindah karena dipakai sebagai landing/health.
4. **Route publik (tanpa `auth:sanctum`)** perlu audit validasi dan rate limit satu per satu sebelum dipindah (lihat kolom Middleware).
5. Route `Broadcast::routes()` (channel Reverb) juga ada di grup `auth:sanctum`. Channel didefinisikan di `routes/channels.php` (lihat bagian Lain-lain).

## Daftar route per modul

Kolom **Status** diisi saat modul dipindah ke Rust. Awalnya semua `belum`.

### auth (11)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | POST | `/api/auth/login` | AuthController@login | throttle:login | rust |
| 2 | POST | `/api/auth/handoff` | AuthController@createHandoff | auth:sanctum,throttle:10,1 | rust |
| 3 | POST | `/api/auth/handoff/exchange` | AuthController@exchangeHandoff | throttle:handoff-exchange | rust |
| 4 | GET | `/api/auth/google` | AuthController@redirectToGoogle | - | rust |
| 5 | GET | `/api/auth/google/callback` | AuthController@handleGoogleCallback | - | rust |
| 6 | POST | `/api/auth/logout` | AuthController@logout | auth:sanctum | rust |
| 7 | GET | `/api/auth/me` | AuthController@me | auth:sanctum | rust |
| 8 | PUT | `/api/auth/profile` | AuthController@updateProfile | auth:sanctum | belum |
| 9 | POST | `/api/auth/avatar` | AuthController@uploadAvatar | auth:sanctum | belum |
| 10 | DELETE | `/api/auth/avatar` | AuthController@deleteAvatar | auth:sanctum | belum |
| 11 | POST | `/api/auth/impersonate/{user}` | AuthController@impersonate | auth:sanctum,role:admin | belum |

### app-settings (27)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/app-settings/backups/google-drive/callback` | GoogleDriveBackupController@callback | throttle:20,1 | belum |
| 2 | GET | `/api/app-settings` | AppSettingController@index | - | rust |
| 3 | GET | `/api/app-settings/maintenance` | AppSettingController@maintenanceStatus | - | rust |
| 4 | GET | `/api/app-settings/storage-stats` | AppSettingController@storageStats | auth:sanctum,role:admin | rust |
| 5 | POST | `/api/app-settings` | AppSettingController@store | auth:sanctum,role:admin | rust |
| 6 | POST | `/api/app-settings/test-ai-connection` | AppSettingController@testAiConnection | auth:sanctum,role:admin | dihapus |
| 7 | POST | `/api/app-settings/list-ai-models` | AppSettingController@listAiModels | auth:sanctum,role:admin | dihapus |
| 8 | POST | `/api/app-settings/test-mail-connection` | AppSettingController@testMailConnection | auth:sanctum,role:admin | belum |
| 9 | GET | `/api/app-settings/mail-templates` | AppSettingController@mailTemplates | auth:sanctum,role:admin | belum |
| 10 | POST | `/api/app-settings/mail-templates` | AppSettingController@storeMailTemplates | auth:sanctum,role:admin | belum |
| 11 | POST | `/api/app-settings/mail-templates/{key}/test` | AppSettingController@testMailTemplate | auth:sanctum,role:admin | belum |
| 12 | GET | `/api/app-settings/kontrak-templates` | AppSettingController@kontrakTemplates | auth:sanctum,role:admin | rust |
| 13 | GET | `/api/app-settings/kontrak-templates/{key}/download` | AppSettingController@downloadKontrakTemplate | auth:sanctum,role:admin | rust |
| 14 | GET | `/api/app-settings/backups` | BackupController@index | auth:sanctum,role:admin | rust |
| 15 | POST | `/api/app-settings/backups` | BackupController@store | auth:sanctum,role:admin | belum |
| 16 | GET | `/api/app-settings/backups/jobs/{jobId}` | BackupController@showJob | auth:sanctum,role:admin | rust |
| 17 | DELETE | `/api/app-settings/backups/jobs/{jobId}` | BackupController@cancelJob | auth:sanctum,role:admin | belum |
| 18 | POST | `/api/app-settings/backups/restore` | BackupController@restore | auth:sanctum,role:admin | belum |
| 19 | GET | `/api/app-settings/backups/google-drive/status` | GoogleDriveBackupController@status | auth:sanctum,role:admin | belum |
| 20 | GET | `/api/app-settings/backups/google-drive/connect` | GoogleDriveBackupController@connect | auth:sanctum,role:admin | belum |
| 21 | DELETE | `/api/app-settings/backups/google-drive` | GoogleDriveBackupController@disconnect | auth:sanctum,role:admin | belum |
| 22 | GET | `/api/app-settings/backups/google-drive/jobs/{jobId}` | GoogleDriveBackupController@showUploadJob | auth:sanctum,role:admin | belum |
| 23 | DELETE | `/api/app-settings/backups/google-drive/jobs/{jobId}` | GoogleDriveBackupController@cancelUploadJob | auth:sanctum,role:admin | belum |
| 24 | POST | `/api/app-settings/backups/{filename}/google-drive` | GoogleDriveBackupController@upload | auth:sanctum,role:admin | belum |
| 25 | POST | `/api/app-settings/backups/s3/test` | BackupController@testS3Connection | auth:sanctum,role:admin | belum |
| 26 | GET | `/api/app-settings/backups/{filename}` | BackupController@download | auth:sanctum,role:admin | rust |
| 27 | DELETE | `/api/app-settings/backups/{filename}` | BackupController@destroy | auth:sanctum,role:admin | rust |

### panduan (2)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/panduan` | PanduanPageController@publicIndex | - | dihapus |
| 2 | GET | `/api/panduan/{slug}` | PanduanPageController@publicShow | - | dihapus |

### blog (15)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/blog` | BlogController@index | - | belum |
| 2 | GET | `/api/blog/comments` | BlogCommentController@adminIndex | auth:sanctum | belum |
| 3 | GET | `/api/blog/{blog}` | BlogController@show | - | belum |
| 4 | GET | `/api/blog/{blog}/comments` | BlogCommentController@index | - | belum |
| 5 | GET | `/api/blog/{blog}/comments/thread/{comment}` | BlogCommentController@thread | - | belum |
| 6 | GET | `/api/blog/{blog}/comments/count` | BlogCommentController@count | - | belum |
| 7 | POST | `/api/blog/{blog}/comments` | BlogCommentController@store | auth:sanctum,throttle:blog-comments | belum |
| 8 | PUT | `/api/blog/comments/{comment}` | BlogCommentController@update | auth:sanctum,throttle:blog-comments | belum |
| 9 | DELETE | `/api/blog/comments/{comment}` | BlogCommentController@destroy | auth:sanctum | belum |
| 10 | POST | `/api/blog/upload-video` | BlogController@uploadVideo | auth:sanctum | belum |
| 11 | POST | `/api/blog/{blog}/feature` | BlogController@feature | auth:sanctum | belum |
| 12 | DELETE | `/api/blog/{blog}/feature` | BlogController@unfeature | auth:sanctum | belum |
| 13 | POST | `/api/blog` | BlogController@store | auth:sanctum | belum |
| 14 | PUT/PATCH | `/api/blog/{id}` | BlogController@update | auth:sanctum | belum |
| 15 | DELETE | `/api/blog/{id}` | BlogController@destroy | auth:sanctum | belum |

### public (11)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/public/puspen/progress-fisik` | PuspenProgressFisikController@publicIndex | - | dihapus |
| 2 | GET | `/api/public/spam-units/stats` | SpamUnitController@publicStats | - | rust |
| 3 | GET | `/api/public/spam-units/map-stats` | SpamUnitController@publicMapStats | - | rust |
| 4 | GET | `/api/public/spam-kelembagaan/form/{token}` | SpamKelembagaanShareController@publicShow | throttle:60,1 | belum |
| 5 | POST | `/api/public/spam-kelembagaan/form/{token}` | SpamKelembagaanShareController@publicSubmit | throttle:10,1 | belum |
| 6 | GET | `/api/public/spm-sanitasi/stats` | SpmSanitasiController@publicStats | - | belum |
| 7 | GET | `/api/public/spm-sanitasi/map-stats` | SpmSanitasiController@publicMapStats | - | belum |
| 8 | POST | `/api/public/contact` | ContactController@store | throttle:contact-inquiries | belum |
| 9 | GET | `/api/public/puspen/media-shares/{shareToken}` | PuspenMediaShareController@publicShow | - | dihapus |
| 10 | GET | `/api/public/puspen/media-shares/{shareToken}/preview/{media}` | PuspenMediaShareController@publicPreview | - | dihapus |
| 11 | GET | `/api/public/puspen/media-shares/{shareToken}/download` | PuspenMediaShareController@publicDownload | - | dihapus |

### onlyoffice (5)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | POST | `/api/onlyoffice/callback` | OnlyOfficeController@callback | - | rust |
| 2 | GET | `/api/onlyoffice/health` | OnlyOfficeController@health | - | rust |
| 3 | GET | `/api/onlyoffice/media/{media}/download` | OnlyOfficeController@download | - | rust |
| 4 | GET | `/api/onlyoffice/media/{media}/config` | OnlyOfficeController@config | auth:sanctum | rust |
| 5 | GET | `/api/onlyoffice/temp/{file}` | OnlyOfficeController@tempDownload (`onlyoffice.temp.download`, bertanda tangan) | signed | rust (token HMAC Rust, dipakai untuk SPK PDF) |

###  (1)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/` | closure | auth:sanctum | belum |

### client-error-reports (1)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | POST | `/api/client-error-reports` | ClientErrorReportController@store | auth:sanctum | rust |

### berita-acara (2)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/berita-acara/sequence` | BeritaAcaraController@getSequence | auth:sanctum | rust |
| 2 | POST | `/api/berita-acara/sequence` | BeritaAcaraController@updateSequence | auth:sanctum | rust |

### pekerjaan (19)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/pekerjaan/document-register` | PekerjaanController@documentRegister | auth:sanctum | belum |
| 2 | GET | `/api/pekerjaan/kecamatan/{kecamatanId}` | PekerjaanController@byKecamatan | auth:sanctum | belum |
| 3 | GET | `/api/pekerjaan/desa/{desaId}` | PekerjaanController@byDesa | auth:sanctum | rust |
| 4 | GET | `/api/pekerjaan/kegiatan/{kegiatanId}` | PekerjaanController@byKegiatan | auth:sanctum | belum |
| 5 | GET | `/api/pekerjaan/kecamatan/{kecamatanId}/desa/{desaId}` | PekerjaanController@byKecamatanDesa | auth:sanctum | belum |
| 6 | GET | `/api/pekerjaan/stats/pagu-kecamatan/{kecamatanId}` | PekerjaanController@totalPaguByKecamatan | auth:sanctum | belum |
| 7 | GET | `/api/pekerjaan/stats/pagu-kegiatan/{kegiatanId}` | PekerjaanController@totalPaguByKegiatan | auth:sanctum | belum |
| 8 | POST | `/api/pekerjaan/import` | PekerjaanController@import | auth:sanctum | belum |
| 9 | GET | `/api/pekerjaan/import/template` | PekerjaanController@downloadTemplate | auth:sanctum | belum |
| 10 | GET | `/api/pekerjaan` | PekerjaanController@index | auth:sanctum | parsial |
| 11 | POST | `/api/pekerjaan` | PekerjaanController@store | auth:sanctum | belum |
| 12 | GET | `/api/pekerjaan/{id}` | PekerjaanController@show | auth:sanctum | rust |
| 13 | PUT/PATCH | `/api/pekerjaan/{id}` | PekerjaanController@update | auth:sanctum | rust |
| 14 | DELETE | `/api/pekerjaan/{id}` | PekerjaanController@destroy | auth:sanctum | belum |
| 15 | GET | `/api/pekerjaan/{pekerjaan}/media` | PekerjaanController@media | auth:sanctum | belum |
| 16 | GET | `/api/pekerjaan/{pekerjaan}/download-all-berkas` | PekerjaanController@downloadAllBerkas | auth:sanctum | belum |
| 17 | POST | `/api/pekerjaan/{pekerjaan}/berkas/quick-share` | BerkasController@quickShareForPekerjaan | auth:sanctum | dihapus |
| 18 | GET | `/api/pekerjaan/{pekerjaanId}/progress-estimasi` | PekerjaanProgressEstimasiController@show | auth:sanctum | belum |
| 19 | PUT | `/api/pekerjaan/{pekerjaanId}/progress-estimasi` | PekerjaanProgressEstimasiController@update | auth:sanctum | belum |

### menu-permissions (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/menu-permissions/user/menus` | MenuPermissionController@getUserMenus | auth:sanctum | belum |
| 2 | GET | `/api/menu-permissions` | MenuPermissionController@index | auth:sanctum | belum |
| 3 | POST | `/api/menu-permissions` | MenuPermissionController@store | auth:sanctum | belum |
| 4 | GET | `/api/menu-permissions/{id}` | MenuPermissionController@show | auth:sanctum | belum |
| 5 | PUT/PATCH | `/api/menu-permissions/{id}` | MenuPermissionController@update | auth:sanctum | belum |
| 6 | DELETE | `/api/menu-permissions/{id}` | MenuPermissionController@destroy | auth:sanctum | belum |

### sipd-pekerjaan-links (3)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/sipd-pekerjaan-links` | SipdPekerjaanLinkController@index | auth:sanctum | rust |
| 2 | PUT | `/api/sipd-pekerjaan-links` | SipdPekerjaanLinkController@upsert | auth:sanctum | rust |
| 3 | DELETE | `/api/sipd-pekerjaan-links` | SipdPekerjaanLinkController@destroy | auth:sanctum | rust |

### kegiatan-role (3)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/kegiatan-role` | KegiatanRoleController@index | auth:sanctum,role:admin | rust |
| 2 | POST | `/api/kegiatan-role` | KegiatanRoleController@store | auth:sanctum,role:admin | rust |
| 3 | DELETE | `/api/kegiatan-role/{kegiatanRoleId}` | KegiatanRoleController@destroy | auth:sanctum,role:admin | rust |

### user-pekerjaan (8)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/user-pekerjaan` | UserPekerjaanController@index | auth:sanctum,role:admin | belum |
| 2 | POST | `/api/user-pekerjaan` | UserPekerjaanController@store | auth:sanctum,role:admin | belum |
| 3 | DELETE | `/api/user-pekerjaan/{id}` | UserPekerjaanController@destroy | auth:sanctum,role:admin | belum |
| 4 | GET | `/api/user-pekerjaan/user/{userId}` | UserPekerjaanController@byUser | auth:sanctum,role:admin | belum |
| 5 | GET | `/api/user-pekerjaan/pekerjaan/{pekerjaanId}` | UserPekerjaanController@byPekerjaan | auth:sanctum,role:admin | belum |
| 6 | GET | `/api/user-pekerjaan/available-users` | UserPekerjaanController@availableUsers | auth:sanctum,role:admin | belum |
| 7 | GET | `/api/user-pekerjaan/completeness-gaps` | UserPekerjaanController@completenessGaps | auth:sanctum,role:admin | belum |
| 8 | POST | `/api/user-pekerjaan/broadcast-reminders` | UserPekerjaanController@broadcastReminders | auth:sanctum,role:admin | belum |

### data-quality (3)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/data-quality/stats` | DataQualityController@getStats | auth:sanctum,role:admin | rust |
| 2 | GET | `/api/data-quality/items` | DataQualityController@getItems | auth:sanctum,role:admin | rust |
| 3 | GET | `/api/data-quality/action-inbox` | DataQualityController@getActionInbox | auth:sanctum,role:admin | rust |

### audit-logs (2)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/audit-logs` | AuditLogController@index | auth:sanctum,role:admin | rust |
| 2 | GET | `/api/audit-logs/{auditLog}` | AuditLogController@show | auth:sanctum,role:admin | rust |

### admin (7)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/admin/panduan` | PanduanPageController@index | auth:sanctum,role:admin | dihapus |
| 2 | POST | `/api/admin/panduan` | PanduanPageController@store | auth:sanctum,role:admin | dihapus |
| 3 | POST | `/api/admin/panduan/seed` | PanduanPageController@seedDefaults | auth:sanctum,role:admin | dihapus |
| 4 | GET | `/api/admin/panduan/{panduan}` | PanduanPageController@show | auth:sanctum,role:admin | dihapus |
| 5 | PUT | `/api/admin/panduan/{panduan}` | PanduanPageController@update | auth:sanctum,role:admin | dihapus |
| 6 | PATCH | `/api/admin/panduan/{panduan}` | PanduanPageController@update | auth:sanctum,role:admin | dihapus |
| 7 | DELETE | `/api/admin/panduan/{panduan}` | PanduanPageController@destroy | auth:sanctum,role:admin | dihapus |

### error-logs (10)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/error-logs` | ClientErrorReportController@index | auth:sanctum,role:admin | belum |
| 2 | POST | `/api/error-logs/bulk/resolve` | ClientErrorReportController@bulkResolve | auth:sanctum,role:admin | belum |
| 3 | POST | `/api/error-logs/bulk/reopen` | ClientErrorReportController@bulkReopen | auth:sanctum,role:admin | belum |
| 4 | POST | `/api/error-logs/bulk/delete` | ClientErrorReportController@bulkDestroy | auth:sanctum,role:admin | belum |
| 5 | POST | `/api/error-logs/empty` | ClientErrorReportController@destroyAll | auth:sanctum,role:admin | belum |
| 6 | DELETE | `/api/error-logs/bulk` | ClientErrorReportController@bulkDestroy | auth:sanctum,role:admin | belum |
| 7 | DELETE | `/api/error-logs/empty` | ClientErrorReportController@destroyAll | auth:sanctum,role:admin | belum |
| 8 | GET | `/api/error-logs/{errorLog}` | ClientErrorReportController@show | auth:sanctum,role:admin | belum |
| 9 | POST | `/api/error-logs/{errorLog}/resolve` | ClientErrorReportController@resolve | auth:sanctum,role:admin | belum |
| 10 | POST | `/api/error-logs/{errorLog}/reopen` | ClientErrorReportController@reopen | auth:sanctum,role:admin | belum |

### sk (5)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/sk` | SkController@index | auth:sanctum,role:admin | rust |
| 2 | POST | `/api/sk` | SkController@store | auth:sanctum,role:admin | rust |
| 3 | GET | `/api/sk/{id}` | SkController@show | auth:sanctum,role:admin | rust |
| 4 | PUT/PATCH | `/api/sk/{id}` | SkController@update | auth:sanctum,role:admin | rust |
| 5 | DELETE | `/api/sk/{id}` | SkController@destroy | auth:sanctum,role:admin | rust |

### user (1)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/user` | closure | auth:sanctum | rust |

### dashboard (3)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/dashboard/stats` | DashboardController@stats | auth:sanctum | rust |
| 2 | GET | `/api/dashboard/analytics` | AnalyticsController@stats | auth:sanctum | rust |
| 3 | GET | `/api/dashboard/executive-progress` | DashboardController@executiveProgress | auth:sanctum | rust |

### presence (2)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | POST | `/api/presence/heartbeat` | UserPresenceController@heartbeat | auth:sanctum | dihapus |
| 2 | GET | `/api/presence/online` | UserPresenceController@index | auth:sanctum | dihapus |

### search (2)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/search` | SearchController@index | auth:sanctum | rust |
| 2 | POST | `/api/search/ai-summary` | SearchAiSummaryController@stream | auth:sanctum,throttle:30,1 | dihapus |

### spam-units (17)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/spam-units/stats` | SpamUnitController@stats | auth:sanctum | rust |
| 2 | GET | `/api/spam-units/integration/output-options` | SpamUnitController@integrationOutputOptions | auth:sanctum | rust |
| 3 | GET | `/api/spam-units/integration` | SpamUnitController@integration | auth:sanctum | rust |
| 4 | GET | `/api/spam-units/integration/desa/{desaId}` | SpamUnitController@integrationByDesa | auth:sanctum | rust |
| 5 | GET | `/api/spam-units/air-minum-pekerjaan` | SpamUnitController@airMinumPekerjaan | auth:sanctum | rust |
| 6 | POST | `/api/spam-units/{unitSpam}/pekerjaan` | SpamUnitController@attachPekerjaan | auth:sanctum | rust |
| 7 | DELETE | `/api/spam-units/{unitSpam}/pekerjaan/{pekerjaanId}` | SpamUnitController@detachPekerjaan | auth:sanctum | rust |
| 8 | POST | `/api/spam-units/{unitSpam}/sync-pekerjaan` | SpamUnitController@syncPekerjaan | auth:sanctum | rust |
| 9 | POST | `/api/spam-units/{unitSpam}/achievements` | SpamUnitController@addAchievement | auth:sanctum | rust |
| 10 | POST | `/api/spam-units/{unitSpam}/budgets` | SpamUnitController@addBudget | auth:sanctum | rust |
| 11 | DELETE | `/api/spam-units/{unitSpam}/budgets/{budgetId}` | SpamUnitController@deleteBudget | auth:sanctum | rust |
| 12 | POST | `/api/spam-units/import` | SpamUnitController@import | auth:sanctum | belum |
| 13 | GET | `/api/spam-units` | SpamUnitController@index | auth:sanctum | rust |
| 14 | POST | `/api/spam-units` | SpamUnitController@store | auth:sanctum | rust |
| 15 | GET | `/api/spam-units/{id}` | SpamUnitController@show | auth:sanctum | rust |
| 16 | PUT/PATCH | `/api/spam-units/{id}` | SpamUnitController@update | auth:sanctum | rust |
| 17 | DELETE | `/api/spam-units/{id}` | SpamUnitController@destroy | auth:sanctum | rust |

### spam-kelembagaan (8)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/spam-kelembagaan/share-links` | SpamKelembagaanShareController@indexLinks | auth:sanctum | belum |
| 2 | POST | `/api/spam-kelembagaan/share-links` | SpamKelembagaanShareController@storeLink | auth:sanctum | belum |
| 3 | PUT | `/api/spam-kelembagaan/share-links/{shareLink}` | SpamKelembagaanShareController@updateLink | auth:sanctum | belum |
| 4 | DELETE | `/api/spam-kelembagaan/share-links/{shareLink}` | SpamKelembagaanShareController@destroyLink | auth:sanctum | belum |
| 5 | GET | `/api/spam-kelembagaan/submissions` | SpamKelembagaanShareController@indexSubmissions | auth:sanctum | belum |
| 6 | GET | `/api/spam-kelembagaan/submissions/{submission}` | SpamKelembagaanShareController@showSubmission | auth:sanctum | belum |
| 7 | POST | `/api/spam-kelembagaan/submissions/{submission}/approve` | SpamKelembagaanShareController@approveSubmission | auth:sanctum | belum |
| 8 | POST | `/api/spam-kelembagaan/submissions/{submission}/reject` | SpamKelembagaanShareController@rejectSubmission | auth:sanctum | belum |

### spm-sanitasi (15)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/spm-sanitasi/stats` | SpmSanitasiController@stats | auth:sanctum | belum |
| 2 | GET | `/api/spm-sanitasi/capaian` | SpmSanitasiController@capaian | auth:sanctum | belum |
| 3 | GET | `/api/spm-sanitasi/integration` | SpmSanitasiController@integration | auth:sanctum | belum |
| 4 | GET | `/api/spm-sanitasi/integration/desa/{desaId}` | SpmSanitasiController@integrationByDesa | auth:sanctum | belum |
| 5 | GET | `/api/spm-sanitasi/mck-pekerjaan` | SpmSanitasiController@mckPekerjaan | auth:sanctum | belum |
| 6 | POST | `/api/spm-sanitasi/{spmSanitasi}/pekerjaan` | SpmSanitasiController@attachPekerjaan | auth:sanctum | belum |
| 7 | DELETE | `/api/spm-sanitasi/{spmSanitasi}/pekerjaan/{pekerjaanId}` | SpmSanitasiController@detachPekerjaan | auth:sanctum | belum |
| 8 | GET | `/api/spm-sanitasi/export` | SpmSanitasiController@export | auth:sanctum | belum |
| 9 | GET | `/api/spm-sanitasi/import/template` | SpmSanitasiController@downloadTemplate | auth:sanctum | belum |
| 10 | POST | `/api/spm-sanitasi/import` | SpmSanitasiController@import | auth:sanctum | belum |
| 11 | GET | `/api/spm-sanitasi` | SpmSanitasiController@index | auth:sanctum | belum |
| 12 | POST | `/api/spm-sanitasi` | SpmSanitasiController@store | auth:sanctum | belum |
| 13 | GET | `/api/spm-sanitasi/{id}` | SpmSanitasiController@show | auth:sanctum | belum |
| 14 | PUT/PATCH | `/api/spm-sanitasi/{id}` | SpmSanitasiController@update | auth:sanctum | belum |
| 15 | DELETE | `/api/spm-sanitasi/{id}` | SpmSanitasiController@destroy | auth:sanctum | belum |

### survey-lokasi (9)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/survey-lokasi/stats` | SurveyLokasiController@stats | auth:sanctum | belum |
| 2 | GET | `/api/survey-lokasi` | SurveyLokasiController@index | auth:sanctum | belum |
| 3 | POST | `/api/survey-lokasi` | SurveyLokasiController@store | auth:sanctum | belum |
| 4 | GET | `/api/survey-lokasi/{id}` | SurveyLokasiController@show | auth:sanctum | belum |
| 5 | PUT/PATCH | `/api/survey-lokasi/{id}` | SurveyLokasiController@update | auth:sanctum | belum |
| 6 | DELETE | `/api/survey-lokasi/{id}` | SurveyLokasiController@destroy | auth:sanctum | belum |
| 7 | POST | `/api/survey-lokasi/{survey_lokasi}/verifikasi` | SurveyLokasiController@verifikasi | auth:sanctum,role:admin | belum |
| 8 | POST | `/api/survey-lokasi/{survey_lokasi}/foto` | SurveyLokasiController@uploadFoto | auth:sanctum | belum |
| 9 | DELETE | `/api/survey-lokasi/{survey_lokasi}/foto/{mediaId}` | SurveyLokasiController@deleteFoto | auth:sanctum | belum |

### survey-tugas (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/survey-tugas` | SurveyTugasController@index | auth:sanctum | belum |
| 2 | GET | `/api/survey-tugas/{id}` | SurveyTugasController@show | auth:sanctum | belum |
| 3 | POST | `/api/survey-tugas` | SurveyTugasController@store | auth:sanctum,role:admin | belum |
| 4 | PUT | `/api/survey-tugas/{survey_tugas}` | SurveyTugasController@update | auth:sanctum,role:admin | belum |
| 5 | PATCH | `/api/survey-tugas/{survey_tugas}` | SurveyTugasController@update | auth:sanctum,role:admin | belum |
| 6 | DELETE | `/api/survey-tugas/{survey_tugas}` | SurveyTugasController@destroy | auth:sanctum,role:admin | belum |

### kecamatan (5)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/kecamatan` | KecamatanController@index | auth:sanctum | rust |
| 2 | POST | `/api/kecamatan` | KecamatanController@store | auth:sanctum | rust |
| 3 | GET | `/api/kecamatan/{id}` | KecamatanController@show | auth:sanctum | rust |
| 4 | PUT/PATCH | `/api/kecamatan/{id}` | KecamatanController@update | auth:sanctum | rust |
| 5 | DELETE | `/api/kecamatan/{id}` | KecamatanController@destroy | auth:sanctum | rust |

### desa (8)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | POST | `/api/desa/sync-kk` | DesaController@syncKk | auth:sanctum | dihapus |
| 2 | GET | `/api/desa/{desa}/profile` | DesaController@profile | auth:sanctum | belum |
| 3 | GET | `/api/desa` | DesaController@index | auth:sanctum | rust |
| 4 | POST | `/api/desa` | DesaController@store | auth:sanctum | rust |
| 5 | GET | `/api/desa/{id}` | DesaController@show | auth:sanctum | rust |
| 6 | PUT/PATCH | `/api/desa/{id}` | DesaController@update | auth:sanctum | rust |
| 7 | DELETE | `/api/desa/{id}` | DesaController@destroy | auth:sanctum | rust |
| 8 | GET | `/api/desa/kecamatan/{kecamatanId}` | DesaController@byKecamatan | auth:sanctum | rust |

### penyedia (5)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/penyedia` | PenyediaController@index | auth:sanctum | rust |
| 2 | POST | `/api/penyedia` | PenyediaController@store | auth:sanctum | rust |
| 3 | GET | `/api/penyedia/{id}` | PenyediaController@show | auth:sanctum | rust |
| 4 | PUT/PATCH | `/api/penyedia/{id}` | PenyediaController@update | auth:sanctum | rust |
| 5 | DELETE | `/api/penyedia/{id}` | PenyediaController@destroy | auth:sanctum | rust |

### kegiatan (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/kegiatan` | KegiatanController@index | auth:sanctum | rust |
| 2 | POST | `/api/kegiatan` | KegiatanController@store | auth:sanctum | rust |
| 3 | GET | `/api/kegiatan/{id}` | KegiatanController@show | auth:sanctum | rust |
| 4 | PUT/PATCH | `/api/kegiatan/{id}` | KegiatanController@update | auth:sanctum | rust |
| 5 | DELETE | `/api/kegiatan/{id}` | KegiatanController@destroy | auth:sanctum | rust |
| 6 | GET | `/api/kegiatan/tahun/{tahun}` | KegiatanController@byTahun | auth:sanctum | rust |

### kontrak (21)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/kontrak/export/excel` | KontrakController@exportExcel | auth:sanctum | rust |
| 2 | GET | `/api/kontrak/export-all-covers` | KontrakController@exportAllCovers | auth:sanctum | rust |
| 3 | GET | `/api/kontrak/pekerjaan/{pekerjaanId}` | KontrakController@byPekerjaan | auth:sanctum | rust |
| 4 | GET | `/api/kontrak/kegiatan/{kegiatanId}` | KontrakController@byKegiatan | auth:sanctum | rust |
| 5 | GET | `/api/kontrak/penyedia/{penyediaId}` | KontrakController@byPenyedia | auth:sanctum | rust |
| 6 | GET | `/api/kontrak/{id}/export` | KontrakController@export | auth:sanctum | rust |
| 7 | GET | `/api/kontrak/{kontrak}/addendums` | KontrakAddendumController@index | auth:sanctum | rust |
| 8 | GET | `/api/kontrak/{kontrak}/addendum-register-gaps` | KontrakAddendumController@registerGapsForKontrak | auth:sanctum | rust |
| 9 | POST | `/api/kontrak/{kontrak}/addendums` | KontrakAddendumController@store | auth:sanctum | rust |
| 10 | POST | `/api/kontrak/{kontrak}/addendum-numbers` | KontrakAddendumController@generateNumbers | auth:sanctum | rust |
| 11 | POST | `/api/kontrak/import` | KontrakController@import | auth:sanctum | rust |
| 12 | GET | `/api/kontrak/import/template` | KontrakController@downloadTemplate | auth:sanctum | rust |
| 13 | GET | `/api/kontrak` | KontrakController@index | auth:sanctum | rust |
| 14 | POST | `/api/kontrak` | KontrakController@store | auth:sanctum | rust |
| 15 | GET | `/api/kontrak/{id}` | KontrakController@show | auth:sanctum | rust |
| 16 | PUT/PATCH | `/api/kontrak/{id}` | KontrakController@update | auth:sanctum | rust |
| 17 | DELETE | `/api/kontrak/{id}` | KontrakController@destroy | auth:sanctum | rust |
| 18 | GET | `/api/kontrak/{kontrak}/export` | KontrakController@exportDoc | auth:sanctum | rust (tidak terpanggil di Laravel: ditutup rute 6, jadi tidak dipindah terpisah) |
| 19 | GET | `/api/kontrak/{kontrak}/export-cover` | KontrakController@exportCover | auth:sanctum | rust |
| 20 | GET | `/api/kontrak/{kontrak}/bap-context` | KontrakController@bapContext | auth:sanctum | rust |
| 21 | GET | `/api/kontrak/{kontrak}/export-bap` | KontrakController@exportBAP | auth:sanctum | rust |

### kontrak-addendums (13)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/kontrak-addendums/register-gaps` | KontrakAddendumController@registerGaps | auth:sanctum | rust |
| 2 | POST | `/api/kontrak-addendums/register-gaps/{registerId}/notify-pengawas` | KontrakAddendumController@notifyRegisterGapPengawas | auth:sanctum | dihapus |
| 3 | GET | `/api/kontrak-addendums` | KontrakAddendumController@all | auth:sanctum | rust |
| 4 | GET | `/api/kontrak-addendums/{kontrakAddendum}` | KontrakAddendumController@show | auth:sanctum | rust |
| 5 | PUT | `/api/kontrak-addendums/{kontrakAddendum}` | KontrakAddendumController@update | auth:sanctum | rust |
| 6 | DELETE | `/api/kontrak-addendums/{kontrakAddendum}` | KontrakAddendumController@destroy | auth:sanctum | rust |
| 7 | POST | `/api/kontrak-addendums/{kontrakAddendum}/submit` | KontrakAddendumController@submit | auth:sanctum | rust |
| 8 | POST | `/api/kontrak-addendums/{kontrakAddendum}/process` | KontrakAddendumController@process | auth:sanctum | rust |
| 9 | POST | `/api/kontrak-addendums/{kontrakAddendum}/approve` | KontrakAddendumController@approve | auth:sanctum | rust |
| 10 | POST | `/api/kontrak-addendums/{kontrakAddendum}/override-kelengkapan` | KontrakAddendumController@overrideKelengkapan | auth:sanctum | rust |
| 11 | POST | `/api/kontrak-addendums/{kontrakAddendum}/reject` | KontrakAddendumController@reject | auth:sanctum | rust |
| 12 | POST | `/api/kontrak-addendums/{kontrakAddendum}/upload` | KontrakAddendumController@upload | auth:sanctum | rust |
| 13 | PUT | `/api/kontrak-addendums/{kontrakAddendum}/attachment-numbers` | KontrakAddendumController@updateAttachmentNumbers | auth:sanctum | rust |

### procurement (14)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/procurement/spse/status` | SpseProcurementController@sessionStatus | auth:sanctum | rust |
| 2 | POST | `/api/procurement/spse/session` | SpseProcurementController@saveSession | auth:sanctum | rust |
| 3 | DELETE | `/api/procurement/spse/session` | SpseProcurementController@revokeSession | auth:sanctum | rust |
| 4 | POST | `/api/procurement/spse/sync` | SpseProcurementController@sync | auth:sanctum | rust |
| 5 | GET | `/api/procurement/spse/sync/runs` | SpseProcurementController@syncRuns | auth:sanctum | rust |
| 6 | GET | `/api/procurement/spse/staging` | SpseProcurementController@staging | auth:sanctum | rust |
| 7 | GET | `/api/procurement/spse/staging/{id}` | SpseProcurementController@stagingDetail | auth:sanctum | rust |
| 8 | POST | `/api/procurement/spse/staging/apply` | SpseProcurementController@applyStaging | auth:sanctum | rust |
| 9 | POST | `/api/procurement/spse/staging/map` | SpseProcurementController@mapStaging | auth:sanctum | rust |
| 10 | POST | `/api/procurement/spse/staging/promote-draft` | SpseProcurementController@promoteStaging | auth:sanctum | rust |
| 11 | GET | `/api/procurement/spse/packages/{kode_paket}/documents` | SpseProcurementController@packageDocuments | auth:sanctum | rust |
| 12 | POST | `/api/procurement/spse/packages/import-documents` | SpseProcurementController@importPackageDocuments | auth:sanctum | rust |
| 13 | POST | `/api/procurement/spse/packages/download-zip` | SpseProcurementController@downloadPackageZip | auth:sanctum | rust |
| 14 | POST | `/api/procurement/spse/kontrak/push` | SpseProcurementController@pushKontrak | auth:sanctum | belum |

### penerima (9)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/penerima/summary` | PenerimaController@summary | auth:sanctum | rust |
| 2 | GET | `/api/penerima/rekap` | PenerimaController@rekap | auth:sanctum | rust |
| 3 | GET | `/api/penerima` | PenerimaController@index | auth:sanctum | rust |
| 4 | POST | `/api/penerima` | PenerimaController@store | auth:sanctum | rust |
| 5 | GET | `/api/penerima/{id}` | PenerimaController@show | auth:sanctum | rust |
| 6 | PUT/PATCH | `/api/penerima/{id}` | PenerimaController@update | auth:sanctum | rust |
| 7 | DELETE | `/api/penerima/{id}` | PenerimaController@destroy | auth:sanctum | rust |
| 8 | GET | `/api/penerima/pekerjaan/{pekerjaanId}` | PenerimaController@byPekerjaan | auth:sanctum | rust |
| 9 | GET | `/api/penerima/pekerjaan/{pekerjaanId}/stats/komunal` | PenerimaController@komunalCount | auth:sanctum | rust |

### berkas (9)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/berkas/jenis-dokumen` | BerkasController@jenisDokumen | auth:sanctum | rust |
| 2 | POST | `/api/berkas/upload-from-url` | BerkasController@uploadFromUrl | auth:sanctum | rust |
| 3 | GET | `/api/berkas/{berkas}/export-pdf` | BerkasController@convertToPdf | auth:sanctum | rust |
| 4 | DELETE | `/api/berkas/bulk` | BerkasController@bulkDestroy | auth:sanctum | rust |
| 5 | GET | `/api/berkas` | BerkasController@index | auth:sanctum | rust |
| 6 | POST | `/api/berkas` | BerkasController@store | auth:sanctum | rust |
| 7 | GET | `/api/berkas/{id}` | BerkasController@show | auth:sanctum | rust |
| 8 | PUT/PATCH | `/api/berkas/{id}` | BerkasController@update | auth:sanctum | rust |
| 9 | DELETE | `/api/berkas/{id}` | BerkasController@destroy | auth:sanctum | rust |

### peripaan (3)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/peripaan` | PetaPeripaanController@index | auth:sanctum | rust |
| 2 | POST | `/api/peripaan` | PetaPeripaanController@store | auth:sanctum | rust |
| 3 | DELETE | `/api/peripaan/{id}` | PetaPeripaanController@destroy | auth:sanctum | rust |

### koordinat (1)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | POST | `/api/koordinat/validate` | KoordinatValidationController@validateKoordinat | auth:sanctum | rust |

### foto (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | DELETE | `/api/foto/bulk` | FotoController@bulkDestroy | auth:sanctum | rust |
| 2 | GET | `/api/foto` | FotoController@index | auth:sanctum | rust |
| 3 | POST | `/api/foto` | FotoController@store | auth:sanctum | rust |
| 4 | GET | `/api/foto/{id}` | FotoController@show | auth:sanctum | rust |
| 5 | PUT/PATCH | `/api/foto/{id}` | FotoController@update | auth:sanctum | rust |
| 6 | DELETE | `/api/foto/{id}` | FotoController@destroy | auth:sanctum | rust |

### user-drive (8)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/user-drive` | UserDriveController@index | auth:sanctum | belum |
| 2 | POST | `/api/user-drive/folders` | UserDriveController@storeFolder | auth:sanctum | belum |
| 3 | POST | `/api/user-drive/files` | UserDriveController@storeFile | auth:sanctum | belum |
| 4 | DELETE | `/api/user-drive/bulk` | UserDriveController@bulkDestroy | auth:sanctum | belum |
| 5 | POST | `/api/user-drive/{userDriveItem}/share` | UserDriveController@share | auth:sanctum | belum |
| 6 | GET | `/api/user-drive/{userDriveItem}` | UserDriveController@show | auth:sanctum | belum |
| 7 | PUT | `/api/user-drive/{userDriveItem}` | UserDriveController@rename | auth:sanctum | belum |
| 8 | DELETE | `/api/user-drive/{userDriveItem}` | UserDriveController@destroy | auth:sanctum | belum |

### users (5)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/users` | UserController@index | auth:sanctum | rust |
| 2 | POST | `/api/users` | UserController@store | auth:sanctum | rust |
| 3 | GET | `/api/users/{id}` | UserController@show | auth:sanctum | rust |
| 4 | PUT/PATCH | `/api/users/{id}` | UserController@update | auth:sanctum | rust |
| 5 | DELETE | `/api/users/{id}` | UserController@destroy | auth:sanctum | rust |

### roles (5)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/roles` | RoleController@index | auth:sanctum | rust |
| 2 | POST | `/api/roles` | RoleController@store | auth:sanctum | rust |
| 3 | GET | `/api/roles/{id}` | RoleController@show | auth:sanctum | rust |
| 4 | PUT/PATCH | `/api/roles/{id}` | RoleController@update | auth:sanctum | rust |
| 5 | DELETE | `/api/roles/{id}` | RoleController@destroy | auth:sanctum | rust |

### permissions (10)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/permissions` | PermissionController@index | auth:sanctum | belum |
| 2 | POST | `/api/permissions` | PermissionController@store | auth:sanctum | belum |
| 3 | GET | `/api/permissions/{id}` | PermissionController@show | auth:sanctum | belum |
| 4 | PUT/PATCH | `/api/permissions/{id}` | PermissionController@update | auth:sanctum | belum |
| 5 | DELETE | `/api/permissions/{id}` | PermissionController@destroy | auth:sanctum | belum |
| 6 | GET | `/api/permissions` | PermissionController@index | auth:sanctum | belum |
| 7 | POST | `/api/permissions` | PermissionController@store | auth:sanctum | belum |
| 8 | GET | `/api/permissions/{id}` | PermissionController@show | auth:sanctum | belum |
| 9 | PUT/PATCH | `/api/permissions/{id}` | PermissionController@update | auth:sanctum | belum |
| 10 | DELETE | `/api/permissions/{id}` | PermissionController@destroy | auth:sanctum | belum |

### route-permissions (9)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | POST | `/api/route-permissions/check-access` | RoutePermissionController@check | auth:sanctum | belum |
| 2 | GET | `/api/route-permissions/rules` | RoutePermissionController@rules | auth:sanctum | belum |
| 3 | GET | `/api/route-permissions/user/accessible` | RoutePermissionController@accessible | auth:sanctum | belum |
| 4 | POST | `/api/route-permissions/sync` | RoutePermissionController@sync | auth:sanctum,role:admin | belum |
| 5 | GET | `/api/route-permissions` | RoutePermissionController@index | auth:sanctum | belum |
| 6 | POST | `/api/route-permissions` | RoutePermissionController@store | auth:sanctum | belum |
| 7 | GET | `/api/route-permissions/{id}` | RoutePermissionController@show | auth:sanctum | belum |
| 8 | PUT/PATCH | `/api/route-permissions/{id}` | RoutePermissionController@update | auth:sanctum | belum |
| 9 | DELETE | `/api/route-permissions/{id}` | RoutePermissionController@destroy | auth:sanctum | belum |

### tags (5)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/tags` | TagController@index | auth:sanctum | rust |
| 2 | POST | `/api/tags` | TagController@store | auth:sanctum | rust |
| 3 | GET | `/api/tags/{id}` | TagController@show | auth:sanctum | rust |
| 4 | PUT/PATCH | `/api/tags/{id}` | TagController@update | auth:sanctum | rust |
| 5 | DELETE | `/api/tags/{id}` | TagController@destroy | auth:sanctum | rust |

### pengawas (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/pengawas/statistics` | PengawasController@statistics | auth:sanctum | rust |
| 2 | GET | `/api/pengawas` | PengawasController@index | auth:sanctum | rust |
| 3 | POST | `/api/pengawas` | PengawasController@store | auth:sanctum | rust |
| 4 | GET | `/api/pengawas/{id}` | PengawasController@show | auth:sanctum | rust |
| 5 | PUT/PATCH | `/api/pengawas/{id}` | PengawasController@update | auth:sanctum | rust |
| 6 | DELETE | `/api/pengawas/{id}` | PengawasController@destroy | auth:sanctum | rust |

### draft-pekerjaan (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/draft-pekerjaan/export/excel` | DraftPekerjaanController@exportExcel | auth:sanctum | rust |
| 2 | GET | `/api/draft-pekerjaan` | DraftPekerjaanController@index | auth:sanctum | rust |
| 3 | POST | `/api/draft-pekerjaan` | DraftPekerjaanController@store | auth:sanctum | rust |
| 4 | GET | `/api/draft-pekerjaan/{id}` | DraftPekerjaanController@show | auth:sanctum | rust |
| 5 | PUT/PATCH | `/api/draft-pekerjaan/{id}` | DraftPekerjaanController@update | auth:sanctum | rust |
| 6 | DELETE | `/api/draft-pekerjaan/{id}` | DraftPekerjaanController@destroy | auth:sanctum | rust |

### checklist-items (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/checklist-items` | ChecklistItemController@index | auth:sanctum | rust |
| 2 | POST | `/api/checklist-items` | ChecklistItemController@store | auth:sanctum | rust |
| 3 | GET | `/api/checklist-items/{id}` | ChecklistItemController@show | auth:sanctum | rust |
| 4 | PUT/PATCH | `/api/checklist-items/{id}` | ChecklistItemController@update | auth:sanctum | rust |
| 5 | DELETE | `/api/checklist-items/{id}` | ChecklistItemController@destroy | auth:sanctum | rust |
| 6 | POST | `/api/checklist-items/reorder` | ChecklistItemController@reorder | auth:sanctum | rust |

### pekerjaan-checklist (5)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/pekerjaan-checklist` | PekerjaanChecklistController@index | auth:sanctum | rust |
| 2 | POST | `/api/pekerjaan-checklist/toggle` | PekerjaanChecklistController@toggle | auth:sanctum | rust |
| 3 | GET | `/api/pekerjaan-checklist/history` | PekerjaanChecklistController@history | auth:sanctum | rust |
| 4 | GET | `/api/pekerjaan-checklist/export/excel` | PekerjaanChecklistController@exportExcel | auth:sanctum | rust |
| 5 | GET | `/api/pekerjaan-checklist/export/pdf` | PekerjaanChecklistController@exportPdf | auth:sanctum | belum |

### post-pekerjaan-checklist (1)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/post-pekerjaan-checklist` | PostPekerjaanChecklistController@index | auth:sanctum | rust |

### output (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/output/summary` | OutputController@summary | auth:sanctum | rust |
| 2 | GET | `/api/output` | OutputController@index | auth:sanctum | rust |
| 3 | POST | `/api/output` | OutputController@store | auth:sanctum | rust |
| 4 | GET | `/api/output/{id}` | OutputController@show | auth:sanctum | rust |
| 5 | PUT/PATCH | `/api/output/{id}` | OutputController@update | auth:sanctum | rust |
| 6 | DELETE | `/api/output/{id}` | OutputController@destroy | auth:sanctum | rust |

### kanban (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/kanban/board` | KanbanController@board | auth:sanctum | belum |
| 2 | POST | `/api/kanban/cards` | KanbanController@storeCard | auth:sanctum | belum |
| 3 | POST | `/api/kanban/cards/from-tiket` | KanbanController@importFromTiket | auth:sanctum | belum |
| 4 | PUT | `/api/kanban/cards/{card}` | KanbanController@updateCard | auth:sanctum | belum |
| 5 | PATCH | `/api/kanban/cards/{card}/move` | KanbanController@moveCard | auth:sanctum | belum |
| 6 | DELETE | `/api/kanban/cards/{card}` | KanbanController@destroyCard | auth:sanctum | belum |

### tiket (7)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | POST | `/api/tiket/bulk-update` | TiketController@bulkUpdate | auth:sanctum | rust |
| 2 | GET | `/api/tiket` | TiketController@index | auth:sanctum | rust |
| 3 | POST | `/api/tiket` | TiketController@store | auth:sanctum | rust |
| 4 | GET | `/api/tiket/{id}` | TiketController@show | auth:sanctum | rust |
| 5 | PUT/PATCH | `/api/tiket/{id}` | TiketController@update | auth:sanctum | rust |
| 6 | DELETE | `/api/tiket/{id}` | TiketController@destroy | auth:sanctum | rust |
| 7 | POST | `/api/tiket/{tiket}/comments` | TiketCommentController@store | auth:sanctum | rust |

### usulan-kegiatan (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/usulan-kegiatan/export-excel` | UsulanKegiatanController@exportExcel | auth:sanctum | belum |
| 2 | GET | `/api/usulan-kegiatan` | UsulanKegiatanController@index | auth:sanctum | belum |
| 3 | POST | `/api/usulan-kegiatan` | UsulanKegiatanController@store | auth:sanctum | belum |
| 4 | GET | `/api/usulan-kegiatan/{id}` | UsulanKegiatanController@show | auth:sanctum | belum |
| 5 | PUT/PATCH | `/api/usulan-kegiatan/{id}` | UsulanKegiatanController@update | auth:sanctum | belum |
| 6 | DELETE | `/api/usulan-kegiatan/{id}` | UsulanKegiatanController@destroy | auth:sanctum | belum |

### progress (2)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/progress/pekerjaan/{pekerjaanId}` | ProgressController@report | auth:sanctum | rust |
| 2 | POST | `/api/progress/pekerjaan/{pekerjaanId}` | ProgressController@store | auth:sanctum | rust |

### puspen (14)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/puspen/progress-fisik` | PuspenProgressFisikController@index | auth:sanctum | dihapus |
| 2 | POST | `/api/puspen/progress-fisik/bulk-update` | PuspenProgressFisikController@bulkUpdate | auth:sanctum | dihapus |
| 3 | GET | `/api/puspen/pengawas-kpi` | PuspenPengawasKpiController@index | auth:sanctum | dihapus |
| 4 | GET | `/api/puspen/pengawas-kpi/notes-report` | PuspenPengawasKpiController@notesReport | auth:sanctum | dihapus |
| 5 | GET | `/api/puspen/pengawas-kpi/{user}` | PuspenPengawasKpiController@show | auth:sanctum | dihapus |
| 6 | GET | `/api/puspen/pekerjaan/{pekerjaan}/review-notes` | PuspenReviewNoteController@index | auth:sanctum | dihapus |
| 7 | POST | `/api/puspen/pekerjaan/{pekerjaan}/review-notes` | PuspenReviewNoteController@store | auth:sanctum | dihapus |
| 8 | DELETE | `/api/puspen/review-notes/{puspenReviewNote}` | PuspenReviewNoteController@destroy | auth:sanctum | dihapus |
| 9 | GET | `/api/puspen/media-library` | PuspenMediaShareController@mediaLibrary | auth:sanctum | dihapus |
| 10 | DELETE | `/api/puspen/media` | PuspenMediaShareController@destroyMedia | auth:sanctum | dihapus |
| 11 | GET | `/api/puspen/media-shares` | PuspenMediaShareController@index | auth:sanctum | dihapus |
| 12 | POST | `/api/puspen/media-shares` | PuspenMediaShareController@store | auth:sanctum | dihapus |
| 13 | PUT/PATCH | `/api/puspen/media-shares/{id}` | PuspenMediaShareController@update | auth:sanctum | dihapus |
| 14 | DELETE | `/api/puspen/media-shares/{id}` | PuspenMediaShareController@destroy | auth:sanctum | dihapus |

### master-fase-pekerjaan (5)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/master-fase-pekerjaan` | MasterFasePekerjaanController@index | auth:sanctum | rust |
| 2 | POST | `/api/master-fase-pekerjaan` | MasterFasePekerjaanController@store | auth:sanctum | rust |
| 3 | GET | `/api/master-fase-pekerjaan/{id}` | MasterFasePekerjaanController@show | auth:sanctum | rust |
| 4 | PUT/PATCH | `/api/master-fase-pekerjaan/{id}` | MasterFasePekerjaanController@update | auth:sanctum | rust |
| 5 | DELETE | `/api/master-fase-pekerjaan/{id}` | MasterFasePekerjaanController@destroy | auth:sanctum | rust |

### document-types (4)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/document-types` | DocumentRegisterController@types | auth:sanctum | rust |
| 2 | POST | `/api/document-types` | DocumentRegisterController@storeType | auth:sanctum | rust |
| 3 | PUT | `/api/document-types/{id}` | DocumentRegisterController@updateType | auth:sanctum | rust |
| 4 | DELETE | `/api/document-types/{id}` | DocumentRegisterController@destroyType | auth:sanctum | rust |

### document-registers (4)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/document-registers` | DocumentRegisterController@index | auth:sanctum | parsial |
| 2 | POST | `/api/document-registers` | DocumentRegisterController@store | auth:sanctum | rust |
| 3 | PUT | `/api/document-registers/{id}` | DocumentRegisterController@update | auth:sanctum | rust |
| 4 | DELETE | `/api/document-registers/{id}` | DocumentRegisterController@destroy | auth:sanctum | rust |

### debug-data (1)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/debug-data` | closure | auth:sanctum | dihapus |

### notifications (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/notifications` | NotificationController@index | auth:sanctum | rust |
| 2 | POST | `/api/notifications/{id}/read` | NotificationController@markAsRead | auth:sanctum | rust |
| 3 | POST | `/api/notifications/mark-all-read` | NotificationController@markAllAsRead | auth:sanctum | rust |
| 4 | POST | `/api/notifications/broadcast` | NotificationController@sendBroadcast | auth:sanctum,role:admin | rust |
| 5 | GET | `/api/notifications/broadcast-history` | NotificationController@getBroadcastHistory | auth:sanctum,role:admin | rust |
| 6 | DELETE | `/api/notifications/broadcast/{id}` | NotificationController@deleteBroadcast | auth:sanctum,role:admin | rust |

### events (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | POST | `/api/events/{event}/upload` | EventController@upload | auth:sanctum | belum |
| 2 | GET | `/api/events` | EventController@index | auth:sanctum | belum |
| 3 | POST | `/api/events` | EventController@store | auth:sanctum | belum |
| 4 | GET | `/api/events/{id}` | EventController@show | auth:sanctum | belum |
| 5 | PUT/PATCH | `/api/events/{id}` | EventController@update | auth:sanctum | belum |
| 6 | DELETE | `/api/events/{id}` | EventController@destroy | auth:sanctum | belum |

### simulation-networks (11)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/simulation-networks` | SimulationNetworkController@index | auth:sanctum | dihapus |
| 2 | POST | `/api/simulation-networks` | SimulationNetworkController@store | auth:sanctum | dihapus |
| 3 | GET | `/api/simulation-networks/{id}` | SimulationNetworkController@show | auth:sanctum | dihapus |
| 4 | PUT/PATCH | `/api/simulation-networks/{id}` | SimulationNetworkController@update | auth:sanctum | dihapus |
| 5 | DELETE | `/api/simulation-networks/{id}` | SimulationNetworkController@destroy | auth:sanctum | dihapus |
| 6 | GET | `/api/simulation-networks/{id}/versions` | SimulationNetworkController@versions | auth:sanctum | dihapus |
| 7 | GET | `/api/simulation-networks/{id}/versions/{version}` | SimulationNetworkController@showVersion | auth:sanctum | dihapus |
| 8 | POST | `/api/simulation-networks/{id}/versions/{version}/restore` | SimulationNetworkController@restoreVersion | auth:sanctum | dihapus |
| 9 | POST | `/api/simulation-networks/{id}/results` | SimulationNetworkController@saveResults | auth:sanctum | dihapus |
| 10 | POST | `/api/simulation-networks/{id}/duplicate` | SimulationNetworkController@duplicate | auth:sanctum | dihapus |
| 11 | GET | `/api/simulation-networks/pekerjaan/{pekerjaanId}` | SimulationNetworkController@byPekerjaan | auth:sanctum | dihapus |

### whatsapp (7)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/whatsapp/status` | WhatsAppController@status | auth:sanctum,role:admin | dihapus |
| 2 | GET | `/api/whatsapp/chats` | WhatsAppController@chats | auth:sanctum,role:admin | dihapus |
| 3 | GET | `/api/whatsapp/chats/{jid}/messages` | WhatsAppController@chatMessages | auth:sanctum,role:admin | dihapus |
| 4 | POST | `/api/whatsapp/start` | WhatsAppController@start | auth:sanctum,role:admin | dihapus |
| 5 | POST | `/api/whatsapp/stop` | WhatsAppController@stop | auth:sanctum,role:admin | dihapus |
| 6 | POST | `/api/whatsapp/send` | WhatsAppController@send | auth:sanctum,role:admin | dihapus |
| 7 | POST | `/api/whatsapp/send-bulk` | WhatsAppController@sendBulk | auth:sanctum,role:admin | dihapus |

### tool-pdfs (6)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/tool-pdfs/{toolPdf}/download` | ToolPdfController@download | auth:sanctum | belum |
| 2 | POST | `/api/tool-pdfs/bulk-download` | ToolPdfController@bulkDownload | auth:sanctum | belum |
| 3 | POST | `/api/tool-pdfs/sign` | ToolPdfController@sign | auth:sanctum | belum |
| 4 | GET | `/api/tool-pdfs` | ToolPdfController@index | auth:sanctum | belum |
| 5 | POST | `/api/tool-pdfs` | ToolPdfController@store | auth:sanctum | belum |
| 6 | DELETE | `/api/tool-pdfs/{id}` | ToolPdfController@destroy | auth:sanctum | belum |

### signature-libraries (3)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/signature-libraries` | SignatureLibraryController@index | auth:sanctum | rust |
| 2 | POST | `/api/signature-libraries` | SignatureLibraryController@store | auth:sanctum | rust |
| 3 | DELETE | `/api/signature-libraries/{id}` | SignatureLibraryController@destroy | auth:sanctum | rust |

### live-chat (5)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/live-chat/thread` | LiveChatController@myThread | auth:sanctum | dihapus |
| 2 | GET | `/api/live-chat/inbox` | LiveChatController@inbox | auth:sanctum | dihapus |
| 3 | GET | `/api/live-chat/threads/{thread}/messages` | LiveChatController@messages | auth:sanctum | dihapus |
| 4 | POST | `/api/live-chat/threads/{thread}/messages` | LiveChatController@sendMessage | auth:sanctum | dihapus |
| 5 | PATCH | `/api/live-chat/threads/{thread}/close` | LiveChatController@closeThread | auth:sanctum | dihapus |

### arumanis-insight (1)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | GET | `/api/arumanis-insight` | ArumanisInsightController@index | auth:sanctum | dihapus |

### chat (10)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | POST | `/api/chat` | ChatController@chat | auth:sanctum | dihapus |
| 2 | POST | `/api/chat/stream` | ChatController@chatStream | auth:sanctum | dihapus |
| 3 | GET | `/api/chat/models` | ChatController@listModels | auth:sanctum | dihapus |
| 4 | GET | `/api/chat/sessions` | ChatController@sessions | auth:sanctum | dihapus |
| 5 | POST | `/api/chat/sessions` | ChatController@createSession | auth:sanctum | dihapus |
| 6 | DELETE | `/api/chat/sessions/{id}` | ChatController@deleteSession | auth:sanctum | dihapus |
| 7 | PATCH | `/api/chat/sessions/{id}` | ChatController@renameSession | auth:sanctum | dihapus |
| 8 | GET | `/api/chat/sessions/{id}/messages` | ChatController@sessionMessages | auth:sanctum | dihapus |
| 9 | POST | `/api/chat/messages/{id}/vote` | ChatController@voteMessage | auth:sanctum | dihapus |
| 10 | GET | `/api/chat/reports/download` | ChatController@downloadReport | auth:sanctum | dihapus |

### paperless (7)

| No | Method | Path | Controller@action | Middleware | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | POST | `/api/paperless/sync-all` | PaperlessController@syncAll | auth:sanctum | dihapus |
| 2 | POST | `/api/paperless/synced-ids` | PaperlessController@syncedIds | auth:sanctum | dihapus |
| 3 | POST | `/api/paperless/media/{media}/sync` | PaperlessController@sync | auth:sanctum | dihapus |
| 4 | GET | `/api/paperless/media/{media}` | PaperlessController@show | auth:sanctum | dihapus |
| 5 | GET | `/api/paperless/media/{media}/download` | PaperlessController@download | auth:sanctum | dihapus |
| 6 | GET | `/api/paperless/documents` | PaperlessController@search | auth:sanctum | dihapus |
| 7 | GET | `/api/paperless/reconcile` | PaperlessController@reconcile | auth:sanctum | dihapus |

## Lain-lain

### `routes/web.php`

| Method | Path | Handler | Middleware | Catatan |
| --- | --- | --- | --- | --- |
| GET | `/` | closure: JSON `service`, `status`, `health`, `api`, dan `docs` (non-production) | - | Landing JSON. Tidak ada di `/api` |

### `routes/channels.php` (Broadcast / Reverb)

| Channel | Otorisasi |
| --- | --- |
| `App.Models.User.{id}` | closure (cek id user) |
| `pekerjaan.{pekerjaanId}` | closure |
| `live-chat.thread.{threadId}` | closure |
| `live-chat.inbox` | closure |

### `routes/console.php` (scheduler dan command)

| Jadwal | Perintah |
| --- | --- |
| daily | `blog-assets:cleanup-orphans --hours=24` |
| weekly | `chat:index-knowledge` |
| (command) | `inspire` (contoh bawaan Laravel, bisa dihapus) |

