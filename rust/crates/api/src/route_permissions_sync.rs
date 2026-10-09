//! Sinkron rule permission dari rute: `POST /api/route-permissions/sync` (khusus admin).
//!
//! Setara `RoutePermissionController::sync` dan `RoutePermissionSyncService::sync` di Laravel.
//! Laravel memindai `Route::getRoutes()`. Router Rust tidak bisa dipindai saat berjalan, jadi
//! `ROUTES` disalin dari keluaran `Route::getRoutes()` dengan urutan yang sama. Rute non-api dan
//! method HEAD ikut disalin, lalu disaring di kode seperti di Laravel.
//!
//! Perilaku yang direplikasi:
//! - Rute dipilih bila uri berawalan `prefix` (default `api`). Prefix kosong atau `"0"` tidak
//!   menyaring, seperti `if ($prefix && ...)` di PHP.
//! - `path`: awalan `api/` dibuang, `{x}` dan `{x?}` menjadi `:x`, `?` dibuang, slash ganda
//!   digabung, slash akhir dibuang, dan hasil kosong menjadi `/`.
//! - Setiap method selain HEAD masuk `scanned`. Bila pasangan path + method belum ada, dibuat rule
//!   dengan `description` "Auto generated for {nama rute atau path}", `allowed_roles` berisi
//!   `default_role`, dan `is_active` true. Rule yang sudah ada tidak diubah.
//! - `clean`: rule yang tidak ada di hasil pindaian dihapus, termasuk rule milik prefix lain.
//! - Setiap create dan delete menulis `tbl_audit_logs`, seperti trait `Auditable`.
//! - Semua langkah berjalan dalam satu transaksi. Laravel tidak memakai transaksi, jadi bila
//!   gagal di tengah jalan Laravel meninggalkan sebagian rule, sedangkan Rust tidak menyisakan apa pun.
//!
//! Penjagaan drift: `ROUTES_SOURCE_SHA256` adalah SHA-256 berkas yang menentukan rute (lihat
//! `source_digest`). Tes `route_table_matches_sources` gagal bila berkas itu berubah. Untuk membuat
//! ulang `ROUTES` (perlu PHP dan `vendor/` lengkap), jalankan dari root repo:
//! `php -r 'require "vendor/autoload.php"; $app = require "bootstrap/app.php";
//! $app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap();
//! foreach (\Illuminate\Support\Facades\Route::getRoutes() as $r) echo json_encode([$r->uri(),
//! array_values($r->methods()), $r->getName()]), "\n";'`
//! Lalu ubah tiap baris menjadi `RouteEntry` dan setel `ROUTES_SOURCE_SHA256` ke digest yang
//! dilaporkan tes.

use std::{collections::HashSet, sync::OnceLock};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySql, MySqlPool, Row, Transaction};

use crate::{
    audit, desa::internal, lookup::carbon_json, notifications::require_admin, require_auth,
    validation::Errors, AppState,
};

const SYNC_PATH: &str = "/api/route-permissions/sync";
const MODEL: &str = "App\\Models\\RoutePermission";
const MESSAGE: &str = "Route permission berhasil disinkronkan.";
const DEFAULT_PREFIX: &str = "api";
const DEFAULT_ROLE: &str = "admin";
const ROW_COLUMNS: &str = "SELECT id, route_path, route_method, description, \
    CAST(allowed_roles AS CHAR) AS allowed_roles, CAST(is_active AS SIGNED) AS is_active, \
    created_at, updated_at FROM route_permissions";

/// Satu entri `Route::getRoutes()`: uri tanpa slash di depan, method termasuk HEAD, dan nama rute.
struct RouteEntry {
    uri: &'static str,
    methods: &'static [&'static str],
    name: Option<&'static str>,
}

/// SHA-256 gabungan berkas sumber rute. Lihat `source_digest` dan modul ini.
#[cfg(test)]
const ROUTES_SOURCE_SHA256: &str = "9e66f1f6047c6659f48dcedb20c05d54afda88db8bbd7e01f7c199ce282c8e35";

/// Daftar rute dari `Route::getRoutes()` Laravel, dalam urutan router.
const ROUTES: &[RouteEntry] = &[
    RouteEntry {
        uri: "api/auth/login",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/auth/handoff",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/auth/handoff/exchange",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/auth/google",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/auth/google/callback",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/google-drive/callback",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/maintenance",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/storage-stats",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/test-mail-connection",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/mail-templates",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/mail-templates",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/mail-templates/{key}/test",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/kontrak-templates",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/kontrak-templates/{key}/download",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog/comments",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog/{blog}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog/{blog}/comments",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog/{blog}/comments/thread/{comment}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog/{blog}/comments/count",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/public/spam-units/stats",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/public/spam-units/map-stats",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/public/spam-units/map-stats/series",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/public/spam-kelembagaan/form/{token}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/public/spam-kelembagaan/form/{token}",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/public/spm-sanitasi/stats",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/public/spm-sanitasi/map-stats",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/public/spm-sanitasi/map-stats/series",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/public/contact",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/onlyoffice/callback",
        methods: &["POST"],
        name: Some("onlyoffice.callback"),
    },
    RouteEntry {
        uri: "api/onlyoffice/health",
        methods: &["GET", "HEAD"],
        name: Some("onlyoffice.health"),
    },
    RouteEntry {
        uri: "api/onlyoffice/media/{media}/download",
        methods: &["GET", "HEAD"],
        name: Some("onlyoffice.media.download"),
    },
    RouteEntry {
        uri: "api/onlyoffice/temp/{file}",
        methods: &["GET", "HEAD"],
        name: Some("onlyoffice.temp.download"),
    },
    RouteEntry {
        uri: "api/broadcasting/auth",
        methods: &["GET", "POST", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/auth/logout",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/auth/me",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/auth/profile",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/auth/avatar",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/auth/avatar",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/auth/impersonate/{user}",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/client-error-reports",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/berita-acara/sequence",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/berita-acara/sequence",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/document-register",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/kecamatan/{kecamatanId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/desa/{desaId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/kegiatan/{kegiatanId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/kecamatan/{kecamatanId}/desa/{desaId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/stats/pagu-kecamatan/{kecamatanId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/stats/pagu-kegiatan/{kegiatanId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/import",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/import/template",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan",
        methods: &["GET", "HEAD"],
        name: Some("pekerjaan.index"),
    },
    RouteEntry {
        uri: "api/pekerjaan",
        methods: &["POST"],
        name: Some("pekerjaan.store"),
    },
    RouteEntry {
        uri: "api/pekerjaan/{pekerjaan}",
        methods: &["GET", "HEAD"],
        name: Some("pekerjaan.show"),
    },
    RouteEntry {
        uri: "api/pekerjaan/{pekerjaan}",
        methods: &["PUT", "PATCH"],
        name: Some("pekerjaan.update"),
    },
    RouteEntry {
        uri: "api/pekerjaan/{pekerjaan}",
        methods: &["DELETE"],
        name: Some("pekerjaan.destroy"),
    },
    RouteEntry {
        uri: "api/pekerjaan/{pekerjaan}/media",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/{pekerjaan}/download-all-berkas",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/menu-permissions/user/menus",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/sipd-pekerjaan-links",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/sipd-pekerjaan-links",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/sipd-pekerjaan-links",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/kegiatan-role",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kegiatan-role",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kegiatan-role/{kegiatanRoleId}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-pekerjaan",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-pekerjaan",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-pekerjaan/{id}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-pekerjaan/user/{userId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-pekerjaan/pekerjaan/{pekerjaanId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-pekerjaan/available-users",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-pekerjaan/completeness-gaps",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-pekerjaan/broadcast-reminders",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/data-quality/stats",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/data-quality/items",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/data-quality/action-inbox",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/audit-logs",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/audit-logs/{auditLog}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/error-logs",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/error-logs/bulk/resolve",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/error-logs/bulk/reopen",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/error-logs/bulk/delete",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/error-logs/empty",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/error-logs/bulk",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/error-logs/empty",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/error-logs/{errorLog}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/error-logs/{errorLog}/resolve",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/error-logs/{errorLog}/reopen",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/sk",
        methods: &["GET", "HEAD"],
        name: Some("sk.index"),
    },
    RouteEntry {
        uri: "api/sk",
        methods: &["POST"],
        name: Some("sk.store"),
    },
    RouteEntry {
        uri: "api/sk/{sk}",
        methods: &["GET", "HEAD"],
        name: Some("sk.show"),
    },
    RouteEntry {
        uri: "api/sk/{sk}",
        methods: &["PUT", "PATCH"],
        name: Some("sk.update"),
    },
    RouteEntry {
        uri: "api/sk/{sk}",
        methods: &["DELETE"],
        name: Some("sk.destroy"),
    },
    RouteEntry {
        uri: "api/user",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/dashboard/stats",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/dashboard/analytics",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/dashboard/executive-progress",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/search",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/stats",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/stats/series",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/integration/output-options",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/integration",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/integration/desa/{desaId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/air-minum-pekerjaan",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/{unitSpam}/pekerjaan",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/{unitSpam}/pekerjaan/{pekerjaanId}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/{unitSpam}/sync-pekerjaan",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/{unitSpam}/achievements",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/{unitSpam}/budgets",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/{unitSpam}/budgets/{budgetId}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units/import",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-units",
        methods: &["GET", "HEAD"],
        name: Some("spam-units.index"),
    },
    RouteEntry {
        uri: "api/spam-units",
        methods: &["POST"],
        name: Some("spam-units.store"),
    },
    RouteEntry {
        uri: "api/spam-units/{spam_unit}",
        methods: &["GET", "HEAD"],
        name: Some("spam-units.show"),
    },
    RouteEntry {
        uri: "api/spam-units/{spam_unit}",
        methods: &["PUT", "PATCH"],
        name: Some("spam-units.update"),
    },
    RouteEntry {
        uri: "api/spam-units/{spam_unit}",
        methods: &["DELETE"],
        name: Some("spam-units.destroy"),
    },
    RouteEntry {
        uri: "api/spam-kelembagaan/share-links",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-kelembagaan/share-links",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-kelembagaan/share-links/{shareLink}",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-kelembagaan/share-links/{shareLink}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-kelembagaan/submissions",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-kelembagaan/submissions/{submission}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-kelembagaan/submissions/{submission}/approve",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/spam-kelembagaan/submissions/{submission}/reject",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi/stats",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi/stats/series",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi/capaian",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi/integration",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi/integration/desa/{desaId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi/mck-pekerjaan",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi/{spmSanitasi}/pekerjaan",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi/{spmSanitasi}/pekerjaan/{pekerjaanId}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi/export",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi/import/template",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi/import",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/spm-sanitasi",
        methods: &["GET", "HEAD"],
        name: Some("spm-sanitasi.index"),
    },
    RouteEntry {
        uri: "api/spm-sanitasi",
        methods: &["POST"],
        name: Some("spm-sanitasi.store"),
    },
    RouteEntry {
        uri: "api/spm-sanitasi/{spm_sanitasi}",
        methods: &["GET", "HEAD"],
        name: Some("spm-sanitasi.show"),
    },
    RouteEntry {
        uri: "api/spm-sanitasi/{spm_sanitasi}",
        methods: &["PUT", "PATCH"],
        name: Some("spm-sanitasi.update"),
    },
    RouteEntry {
        uri: "api/spm-sanitasi/{spm_sanitasi}",
        methods: &["DELETE"],
        name: Some("spm-sanitasi.destroy"),
    },
    RouteEntry {
        uri: "api/survey-lokasi/stats",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/survey-lokasi",
        methods: &["GET", "HEAD"],
        name: Some("survey-lokasi.index"),
    },
    RouteEntry {
        uri: "api/survey-lokasi",
        methods: &["POST"],
        name: Some("survey-lokasi.store"),
    },
    RouteEntry {
        uri: "api/survey-lokasi/{survey_lokasi}",
        methods: &["GET", "HEAD"],
        name: Some("survey-lokasi.show"),
    },
    RouteEntry {
        uri: "api/survey-lokasi/{survey_lokasi}",
        methods: &["PUT", "PATCH"],
        name: Some("survey-lokasi.update"),
    },
    RouteEntry {
        uri: "api/survey-lokasi/{survey_lokasi}",
        methods: &["DELETE"],
        name: Some("survey-lokasi.destroy"),
    },
    RouteEntry {
        uri: "api/survey-lokasi/{survey_lokasi}/verifikasi",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/survey-lokasi/{survey_lokasi}/foto",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/survey-lokasi/{survey_lokasi}/foto/{mediaId}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/survey-tugas",
        methods: &["GET", "HEAD"],
        name: Some("survey-tugas.index"),
    },
    RouteEntry {
        uri: "api/survey-tugas/{survey_tugas}",
        methods: &["GET", "HEAD"],
        name: Some("survey-tugas.show"),
    },
    RouteEntry {
        uri: "api/survey-tugas",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/survey-tugas/{survey_tugas}",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/survey-tugas/{survey_tugas}",
        methods: &["PATCH"],
        name: None,
    },
    RouteEntry {
        uri: "api/survey-tugas/{survey_tugas}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/kecamatan",
        methods: &["GET", "HEAD"],
        name: Some("kecamatan.index"),
    },
    RouteEntry {
        uri: "api/kecamatan",
        methods: &["POST"],
        name: Some("kecamatan.store"),
    },
    RouteEntry {
        uri: "api/kecamatan/{kecamatan}",
        methods: &["GET", "HEAD"],
        name: Some("kecamatan.show"),
    },
    RouteEntry {
        uri: "api/kecamatan/{kecamatan}",
        methods: &["PUT", "PATCH"],
        name: Some("kecamatan.update"),
    },
    RouteEntry {
        uri: "api/kecamatan/{kecamatan}",
        methods: &["DELETE"],
        name: Some("kecamatan.destroy"),
    },
    RouteEntry {
        uri: "api/desa/{desa}/profile",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/desa",
        methods: &["GET", "HEAD"],
        name: Some("desa.index"),
    },
    RouteEntry {
        uri: "api/desa",
        methods: &["POST"],
        name: Some("desa.store"),
    },
    RouteEntry {
        uri: "api/desa/{desa}",
        methods: &["GET", "HEAD"],
        name: Some("desa.show"),
    },
    RouteEntry {
        uri: "api/desa/{desa}",
        methods: &["PUT", "PATCH"],
        name: Some("desa.update"),
    },
    RouteEntry {
        uri: "api/desa/{desa}",
        methods: &["DELETE"],
        name: Some("desa.destroy"),
    },
    RouteEntry {
        uri: "api/penyedia",
        methods: &["GET", "HEAD"],
        name: Some("penyedia.index"),
    },
    RouteEntry {
        uri: "api/penyedia",
        methods: &["POST"],
        name: Some("penyedia.store"),
    },
    RouteEntry {
        uri: "api/penyedia/{penyedia}",
        methods: &["GET", "HEAD"],
        name: Some("penyedia.show"),
    },
    RouteEntry {
        uri: "api/penyedia/{penyedia}",
        methods: &["PUT", "PATCH"],
        name: Some("penyedia.update"),
    },
    RouteEntry {
        uri: "api/penyedia/{penyedia}",
        methods: &["DELETE"],
        name: Some("penyedia.destroy"),
    },
    RouteEntry {
        uri: "api/kegiatan",
        methods: &["GET", "HEAD"],
        name: Some("kegiatan.index"),
    },
    RouteEntry {
        uri: "api/kegiatan",
        methods: &["POST"],
        name: Some("kegiatan.store"),
    },
    RouteEntry {
        uri: "api/kegiatan/{kegiatan}",
        methods: &["GET", "HEAD"],
        name: Some("kegiatan.show"),
    },
    RouteEntry {
        uri: "api/kegiatan/{kegiatan}",
        methods: &["PUT", "PATCH"],
        name: Some("kegiatan.update"),
    },
    RouteEntry {
        uri: "api/kegiatan/{kegiatan}",
        methods: &["DELETE"],
        name: Some("kegiatan.destroy"),
    },
    RouteEntry {
        uri: "api/kontrak/export/excel",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/export-all-covers",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/pekerjaan/{pekerjaanId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/kegiatan/{kegiatanId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/penyedia/{penyediaId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/{id}/export",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums/register-gaps",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}/addendums",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}/addendum-register-gaps",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}/addendums",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}/addendum-numbers",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums/{kontrakAddendum}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums/{kontrakAddendum}",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums/{kontrakAddendum}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums/{kontrakAddendum}/submit",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums/{kontrakAddendum}/process",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums/{kontrakAddendum}/approve",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums/{kontrakAddendum}/override-kelengkapan",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums/{kontrakAddendum}/reject",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums/{kontrakAddendum}/upload",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak-addendums/{kontrakAddendum}/attachment-numbers",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/import",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/import/template",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/status",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/session",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/session",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/sync",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/sync/runs",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/staging",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/staging/{id}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/staging/apply",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/staging/map",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/staging/promote-draft",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/packages/{kode_paket}/documents",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/packages/import-documents",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/packages/download-zip",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/procurement/spse/kontrak/push",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak",
        methods: &["GET", "HEAD"],
        name: Some("kontrak.index"),
    },
    RouteEntry {
        uri: "api/kontrak",
        methods: &["POST"],
        name: Some("kontrak.store"),
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}",
        methods: &["GET", "HEAD"],
        name: Some("kontrak.show"),
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}",
        methods: &["PUT", "PATCH"],
        name: Some("kontrak.update"),
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}",
        methods: &["DELETE"],
        name: Some("kontrak.destroy"),
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}/export",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}/export-ringkasan",
        methods: &["GET", "POST", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}/preview-ringkasan",
        methods: &["GET", "POST", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}/export-cover",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}/bap-context",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kontrak/{kontrak}/export-bap",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/penerima/summary",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/penerima/rekap",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/penerima",
        methods: &["GET", "HEAD"],
        name: Some("penerima.index"),
    },
    RouteEntry {
        uri: "api/penerima",
        methods: &["POST"],
        name: Some("penerima.store"),
    },
    RouteEntry {
        uri: "api/penerima/{penerima}",
        methods: &["GET", "HEAD"],
        name: Some("penerima.show"),
    },
    RouteEntry {
        uri: "api/penerima/{penerima}",
        methods: &["PUT", "PATCH"],
        name: Some("penerima.update"),
    },
    RouteEntry {
        uri: "api/penerima/{penerima}",
        methods: &["DELETE"],
        name: Some("penerima.destroy"),
    },
    RouteEntry {
        uri: "api/berkas/jenis-dokumen",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/berkas/upload-from-url",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/berkas/{berkas}/export-pdf",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/berkas/bulk",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/berkas",
        methods: &["GET", "HEAD"],
        name: Some("berkas.index"),
    },
    RouteEntry {
        uri: "api/berkas",
        methods: &["POST"],
        name: Some("berkas.store"),
    },
    RouteEntry {
        uri: "api/berkas/{berkas}",
        methods: &["GET", "HEAD"],
        name: Some("berkas.show"),
    },
    RouteEntry {
        uri: "api/berkas/{berkas}",
        methods: &["PUT", "PATCH"],
        name: Some("berkas.update"),
    },
    RouteEntry {
        uri: "api/berkas/{berkas}",
        methods: &["DELETE"],
        name: Some("berkas.destroy"),
    },
    RouteEntry {
        uri: "api/peripaan",
        methods: &["GET", "HEAD"],
        name: Some("peripaan.index"),
    },
    RouteEntry {
        uri: "api/peripaan",
        methods: &["POST"],
        name: Some("peripaan.store"),
    },
    RouteEntry {
        uri: "api/peripaan/{peripaan}",
        methods: &["DELETE"],
        name: Some("peripaan.destroy"),
    },
    RouteEntry {
        uri: "api/koordinat/validate",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/foto/bulk",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/foto",
        methods: &["GET", "HEAD"],
        name: Some("foto.index"),
    },
    RouteEntry {
        uri: "api/foto",
        methods: &["POST"],
        name: Some("foto.store"),
    },
    RouteEntry {
        uri: "api/foto/{foto}",
        methods: &["GET", "HEAD"],
        name: Some("foto.show"),
    },
    RouteEntry {
        uri: "api/foto/{foto}",
        methods: &["PUT", "PATCH"],
        name: Some("foto.update"),
    },
    RouteEntry {
        uri: "api/foto/{foto}",
        methods: &["DELETE"],
        name: Some("foto.destroy"),
    },
    RouteEntry {
        uri: "api/user-drive",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-drive/folders",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-drive/files",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-drive/bulk",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-drive/{userDriveItem}/share",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-drive/{userDriveItem}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-drive/{userDriveItem}",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/user-drive/{userDriveItem}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/users",
        methods: &["GET", "HEAD"],
        name: Some("users.index"),
    },
    RouteEntry {
        uri: "api/users",
        methods: &["POST"],
        name: Some("users.store"),
    },
    RouteEntry {
        uri: "api/users/{user}",
        methods: &["GET", "HEAD"],
        name: Some("users.show"),
    },
    RouteEntry {
        uri: "api/users/{user}",
        methods: &["PUT", "PATCH"],
        name: Some("users.update"),
    },
    RouteEntry {
        uri: "api/users/{user}",
        methods: &["DELETE"],
        name: Some("users.destroy"),
    },
    RouteEntry {
        uri: "api/roles",
        methods: &["GET", "HEAD"],
        name: Some("roles.index"),
    },
    RouteEntry {
        uri: "api/roles",
        methods: &["POST"],
        name: Some("roles.store"),
    },
    RouteEntry {
        uri: "api/roles/{role}",
        methods: &["GET", "HEAD"],
        name: Some("roles.show"),
    },
    RouteEntry {
        uri: "api/roles/{role}",
        methods: &["PUT", "PATCH"],
        name: Some("roles.update"),
    },
    RouteEntry {
        uri: "api/roles/{role}",
        methods: &["DELETE"],
        name: Some("roles.destroy"),
    },
    RouteEntry {
        uri: "api/permissions",
        methods: &["GET", "HEAD"],
        name: Some("permissions.index"),
    },
    RouteEntry {
        uri: "api/permissions",
        methods: &["POST"],
        name: Some("permissions.store"),
    },
    RouteEntry {
        uri: "api/permissions/{permission}",
        methods: &["GET", "HEAD"],
        name: Some("permissions.show"),
    },
    RouteEntry {
        uri: "api/permissions/{permission}",
        methods: &["PUT", "PATCH"],
        name: Some("permissions.update"),
    },
    RouteEntry {
        uri: "api/permissions/{permission}",
        methods: &["DELETE"],
        name: Some("permissions.destroy"),
    },
    RouteEntry {
        uri: "api/route-permissions/check-access",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/route-permissions/rules",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/route-permissions/user/accessible",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/route-permissions/sync",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/route-permissions",
        methods: &["GET", "HEAD"],
        name: Some("route-permissions.index"),
    },
    RouteEntry {
        uri: "api/route-permissions",
        methods: &["POST"],
        name: Some("route-permissions.store"),
    },
    RouteEntry {
        uri: "api/route-permissions/{route_permission}",
        methods: &["GET", "HEAD"],
        name: Some("route-permissions.show"),
    },
    RouteEntry {
        uri: "api/route-permissions/{route_permission}",
        methods: &["PUT", "PATCH"],
        name: Some("route-permissions.update"),
    },
    RouteEntry {
        uri: "api/route-permissions/{route_permission}",
        methods: &["DELETE"],
        name: Some("route-permissions.destroy"),
    },
    RouteEntry {
        uri: "api/menu-permissions",
        methods: &["GET", "HEAD"],
        name: Some("menu-permissions.index"),
    },
    RouteEntry {
        uri: "api/menu-permissions",
        methods: &["POST"],
        name: Some("menu-permissions.store"),
    },
    RouteEntry {
        uri: "api/menu-permissions/{menu_permission}",
        methods: &["GET", "HEAD"],
        name: Some("menu-permissions.show"),
    },
    RouteEntry {
        uri: "api/menu-permissions/{menu_permission}",
        methods: &["PUT", "PATCH"],
        name: Some("menu-permissions.update"),
    },
    RouteEntry {
        uri: "api/menu-permissions/{menu_permission}",
        methods: &["DELETE"],
        name: Some("menu-permissions.destroy"),
    },
    RouteEntry {
        uri: "api/blog/{blog}/comments",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog/comments/{comment}",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog/comments/{comment}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog/upload-video",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog/{blog}/feature",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog/{blog}/feature",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/blog",
        methods: &["POST"],
        name: Some("blog.store"),
    },
    RouteEntry {
        uri: "api/blog/{blog}",
        methods: &["PUT", "PATCH"],
        name: Some("blog.update"),
    },
    RouteEntry {
        uri: "api/blog/{blog}",
        methods: &["DELETE"],
        name: Some("blog.destroy"),
    },
    RouteEntry {
        uri: "api/tags",
        methods: &["GET", "HEAD"],
        name: Some("tags.index"),
    },
    RouteEntry {
        uri: "api/tags",
        methods: &["POST"],
        name: Some("tags.store"),
    },
    RouteEntry {
        uri: "api/tags/{tag}",
        methods: &["GET", "HEAD"],
        name: Some("tags.show"),
    },
    RouteEntry {
        uri: "api/tags/{tag}",
        methods: &["PUT", "PATCH"],
        name: Some("tags.update"),
    },
    RouteEntry {
        uri: "api/tags/{tag}",
        methods: &["DELETE"],
        name: Some("tags.destroy"),
    },
    RouteEntry {
        uri: "api/pengawas/statistics",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pengawas",
        methods: &["GET", "HEAD"],
        name: Some("pengawas.index"),
    },
    RouteEntry {
        uri: "api/pengawas",
        methods: &["POST"],
        name: Some("pengawas.store"),
    },
    RouteEntry {
        uri: "api/pengawas/{pengawa}",
        methods: &["GET", "HEAD"],
        name: Some("pengawas.show"),
    },
    RouteEntry {
        uri: "api/pengawas/{pengawa}",
        methods: &["PUT", "PATCH"],
        name: Some("pengawas.update"),
    },
    RouteEntry {
        uri: "api/pengawas/{pengawa}",
        methods: &["DELETE"],
        name: Some("pengawas.destroy"),
    },
    RouteEntry {
        uri: "api/draft-pekerjaan/export/excel",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/draft-pekerjaan",
        methods: &["GET", "HEAD"],
        name: Some("draft-pekerjaan.index"),
    },
    RouteEntry {
        uri: "api/draft-pekerjaan",
        methods: &["POST"],
        name: Some("draft-pekerjaan.store"),
    },
    RouteEntry {
        uri: "api/draft-pekerjaan/{draft_pekerjaan}",
        methods: &["GET", "HEAD"],
        name: Some("draft-pekerjaan.show"),
    },
    RouteEntry {
        uri: "api/draft-pekerjaan/{draft_pekerjaan}",
        methods: &["PUT", "PATCH"],
        name: Some("draft-pekerjaan.update"),
    },
    RouteEntry {
        uri: "api/draft-pekerjaan/{draft_pekerjaan}",
        methods: &["DELETE"],
        name: Some("draft-pekerjaan.destroy"),
    },
    RouteEntry {
        uri: "api/checklist-items",
        methods: &["GET", "HEAD"],
        name: Some("checklist-items.index"),
    },
    RouteEntry {
        uri: "api/checklist-items",
        methods: &["POST"],
        name: Some("checklist-items.store"),
    },
    RouteEntry {
        uri: "api/checklist-items/{checklist_item}",
        methods: &["GET", "HEAD"],
        name: Some("checklist-items.show"),
    },
    RouteEntry {
        uri: "api/checklist-items/{checklist_item}",
        methods: &["PUT", "PATCH"],
        name: Some("checklist-items.update"),
    },
    RouteEntry {
        uri: "api/checklist-items/{checklist_item}",
        methods: &["DELETE"],
        name: Some("checklist-items.destroy"),
    },
    RouteEntry {
        uri: "api/checklist-items/reorder",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan-checklist",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan-checklist/toggle",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan-checklist/history",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan-checklist/export/excel",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan-checklist/export/pdf",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/post-pekerjaan-checklist",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/desa/kecamatan/{kecamatanId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kegiatan/tahun/{tahun}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/output/summary",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/output",
        methods: &["GET", "HEAD"],
        name: Some("output.index"),
    },
    RouteEntry {
        uri: "api/output",
        methods: &["POST"],
        name: Some("output.store"),
    },
    RouteEntry {
        uri: "api/output/{output}",
        methods: &["GET", "HEAD"],
        name: Some("output.show"),
    },
    RouteEntry {
        uri: "api/output/{output}",
        methods: &["PUT", "PATCH"],
        name: Some("output.update"),
    },
    RouteEntry {
        uri: "api/output/{output}",
        methods: &["DELETE"],
        name: Some("output.destroy"),
    },
    RouteEntry {
        uri: "api/kanban/board",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/kanban/cards",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kanban/cards/from-tiket",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/kanban/cards/{card}",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/kanban/cards/{card}/move",
        methods: &["PATCH"],
        name: None,
    },
    RouteEntry {
        uri: "api/kanban/cards/{card}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/tiket/bulk-update",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/tiket",
        methods: &["GET", "HEAD"],
        name: Some("tiket.index"),
    },
    RouteEntry {
        uri: "api/tiket",
        methods: &["POST"],
        name: Some("tiket.store"),
    },
    RouteEntry {
        uri: "api/tiket/{tiket}",
        methods: &["GET", "HEAD"],
        name: Some("tiket.show"),
    },
    RouteEntry {
        uri: "api/tiket/{tiket}",
        methods: &["PUT", "PATCH"],
        name: Some("tiket.update"),
    },
    RouteEntry {
        uri: "api/tiket/{tiket}",
        methods: &["DELETE"],
        name: Some("tiket.destroy"),
    },
    RouteEntry {
        uri: "api/usulan-kegiatan/export-excel",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/usulan-kegiatan",
        methods: &["GET", "HEAD"],
        name: Some("usulan-kegiatan.index"),
    },
    RouteEntry {
        uri: "api/usulan-kegiatan",
        methods: &["POST"],
        name: Some("usulan-kegiatan.store"),
    },
    RouteEntry {
        uri: "api/usulan-kegiatan/{usulan_kegiatan}",
        methods: &["GET", "HEAD"],
        name: Some("usulan-kegiatan.show"),
    },
    RouteEntry {
        uri: "api/usulan-kegiatan/{usulan_kegiatan}",
        methods: &["PUT", "PATCH"],
        name: Some("usulan-kegiatan.update"),
    },
    RouteEntry {
        uri: "api/usulan-kegiatan/{usulan_kegiatan}",
        methods: &["DELETE"],
        name: Some("usulan-kegiatan.destroy"),
    },
    RouteEntry {
        uri: "api/tiket/{tiket}/comments",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/penerima/pekerjaan/{pekerjaanId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/penerima/pekerjaan/{pekerjaanId}/stats/komunal",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/progress/pekerjaan/{pekerjaanId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/progress/pekerjaan/{pekerjaanId}",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/{pekerjaanId}/progress-estimasi",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/pekerjaan/{pekerjaanId}/progress-estimasi",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/master-fase-pekerjaan",
        methods: &["GET", "HEAD"],
        name: Some("master-fase-pekerjaan.index"),
    },
    RouteEntry {
        uri: "api/master-fase-pekerjaan",
        methods: &["POST"],
        name: Some("master-fase-pekerjaan.store"),
    },
    RouteEntry {
        uri: "api/master-fase-pekerjaan/{master_fase_pekerjaan}",
        methods: &["GET", "HEAD"],
        name: Some("master-fase-pekerjaan.show"),
    },
    RouteEntry {
        uri: "api/master-fase-pekerjaan/{master_fase_pekerjaan}",
        methods: &["PUT", "PATCH"],
        name: Some("master-fase-pekerjaan.update"),
    },
    RouteEntry {
        uri: "api/master-fase-pekerjaan/{master_fase_pekerjaan}",
        methods: &["DELETE"],
        name: Some("master-fase-pekerjaan.destroy"),
    },
    RouteEntry {
        uri: "api/document-types",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/document-types",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/document-types/{id}",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/document-types/{id}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/document-registers",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/document-registers",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/document-registers/{id}",
        methods: &["PUT"],
        name: None,
    },
    RouteEntry {
        uri: "api/document-registers/{id}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/notifications",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/notifications/{id}/read",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/notifications/mark-all-read",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/notifications/broadcast",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/notifications/broadcast-history",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/notifications/broadcast/{id}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/events/{event}/upload",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/events",
        methods: &["GET", "HEAD"],
        name: Some("events.index"),
    },
    RouteEntry {
        uri: "api/events",
        methods: &["POST"],
        name: Some("events.store"),
    },
    RouteEntry {
        uri: "api/events/{event}",
        methods: &["GET", "HEAD"],
        name: Some("events.show"),
    },
    RouteEntry {
        uri: "api/events/{event}",
        methods: &["PUT", "PATCH"],
        name: Some("events.update"),
    },
    RouteEntry {
        uri: "api/events/{event}",
        methods: &["DELETE"],
        name: Some("events.destroy"),
    },
    RouteEntry {
        uri: "api/app-settings/backups",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/jobs/{jobId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/jobs/{jobId}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/restore",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/google-drive/status",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/google-drive/connect",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/google-drive",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/google-drive/jobs/{jobId}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/google-drive/jobs/{jobId}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/{filename}/google-drive",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/s3/test",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/{filename}",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/app-settings/backups/{filename}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/tool-pdfs/{toolPdf}/download",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/tool-pdfs/bulk-download",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/tool-pdfs/sign",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/tool-pdfs",
        methods: &["GET", "HEAD"],
        name: Some("tool-pdfs.index"),
    },
    RouteEntry {
        uri: "api/tool-pdfs",
        methods: &["POST"],
        name: Some("tool-pdfs.store"),
    },
    RouteEntry {
        uri: "api/tool-pdfs/{tool_pdf}",
        methods: &["DELETE"],
        name: Some("tool-pdfs.destroy"),
    },
    RouteEntry {
        uri: "api/signature-libraries",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "api/signature-libraries",
        methods: &["POST"],
        name: None,
    },
    RouteEntry {
        uri: "api/signature-libraries/{id}",
        methods: &["DELETE"],
        name: None,
    },
    RouteEntry {
        uri: "api/onlyoffice/media/{media}/config",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "up",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "/",
        methods: &["GET", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "broadcasting/auth",
        methods: &["GET", "POST", "HEAD"],
        name: None,
    },
    RouteEntry {
        uri: "storage/{path}",
        methods: &["GET", "HEAD"],
        name: Some("storage.local"),
    },
    RouteEntry {
        uri: "storage/{path}",
        methods: &["PUT"],
        name: Some("storage.local.upload"),
    },
];

/// Opsi yang sudah tervalidasi.
struct Options {
    prefix: String,
    default_role: String,
    clean: bool,
}

#[derive(Default)]
struct Summary {
    scanned: u64,
    created: u64,
    removed: u64,
}

/// `POST /api/route-permissions/sync`.
pub async fn sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let input = parse_body(&body);
    let opts = validate(&state.pool, &input).await?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let sum = run(&mut tx, user.user_id, &opts, &headers).await?;
    tx.commit().await.map_err(internal)?;

    let body = json!({
        "message": MESSAGE,
        "data": {
            "scanned": sum.scanned,
            "created": sum.created,
            "removed": sum.removed,
            "prefix": opts.prefix,
            "default_role": opts.default_role,
        },
    });
    Ok((StatusCode::OK, Json(body)).into_response())
}

/// Aturan Laravel: `prefix` string maks 50, `default_role` string dan ada di `roles.name`,
/// `clean` boolean. Pesan pertama per field ikut menjadi `message`.
async fn validate(pool: &MySqlPool, input: &Map<String, Value>) -> Result<Options, ApiError> {
    let mut e = Errors::default();

    let prefix = match input.get("prefix") {
        None => DEFAULT_PREFIX.to_string(),
        Some(Value::String(s)) => {
            if s.chars().count() > 50 {
                e.add(
                    "prefix",
                    "The prefix field must not be greater than 50 characters.",
                );
            }
            s.clone()
        }
        Some(_) => {
            e.add("prefix", "The prefix field must be a string.");
            String::new()
        }
    };

    let default_role = match input.get("default_role") {
        None => DEFAULT_ROLE.to_string(),
        Some(Value::String(s)) => {
            let n: i64 =
                sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM roles WHERE name = ?")
                    .bind(s)
                    .fetch_one(pool)
                    .await
                    .map_err(internal)?;
            if n == 0 {
                e.add("default_role", "The selected default role is invalid.");
            }
            s.clone()
        }
        Some(_) => {
            e.add("default_role", "The default role field must be a string.");
            String::new()
        }
    };

    let clean = match input.get("clean") {
        None => false,
        Some(v) => match bool_value(v) {
            Some(b) => b,
            None => {
                e.add("clean", "The clean field must be true or false.");
                false
            }
        },
    };

    e.finish()?;
    Ok(Options {
        prefix,
        default_role,
        clean,
    })
}

async fn run(
    tx: &mut Transaction<'_, MySql>,
    actor: u64,
    o: &Options,
    headers: &HeaderMap,
) -> Result<Summary, ApiError> {
    let mut sum = Summary::default();
    let mut processed: HashSet<(String, String)> = HashSet::new();

    for route in ROUTES {
        if php_truthy(&o.prefix) && !route.uri.starts_with(o.prefix.as_str()) {
            continue;
        }
        let path = route_path(route.uri);
        let name = route
            .name
            .map(str::to_string)
            .unwrap_or_else(|| path.clone());

        for method in route.methods.iter().copied().filter(|m| *m != "HEAD") {
            processed.insert((path.clone(), method.to_string()));

            let existing: Option<u64> = sqlx::query_scalar(
                "SELECT id FROM route_permissions WHERE route_path = ? AND route_method = ? LIMIT 1",
            )
            .bind(&path)
            .bind(method)
            .fetch_optional(&mut **tx)
            .await
            .map_err(internal)?;

            if existing.is_none() {
                let roles = serde_json::to_string(&vec![o.default_role.clone()])
                    .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                let id = sqlx::query(
                    "INSERT INTO route_permissions (route_path, route_method, description, allowed_roles, is_active, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, 1, NOW(), NOW())",
                )
                .bind(&path)
                .bind(method)
                .bind(format!("Auto generated for {name}"))
                .bind(roles)
                .execute(&mut **tx)
                .await
                .map_err(internal)?
                .last_insert_id();

                let row = load_map(tx, id).await?;
                write_audit(tx, actor, "created", id, None, Some(row), headers).await?;
                sum.created += 1;
            }
            sum.scanned += 1;
        }
    }

    if o.clean {
        let rows = sqlx::query(ROW_COLUMNS)
            .fetch_all(&mut **tx)
            .await
            .map_err(internal)?;
        for r in &rows {
            let (id, path, method, map) = row_map(r).map_err(internal)?;
            if processed.contains(&(path.clone(), method.clone())) {
                continue;
            }
            write_audit(tx, actor, "deleted", id, Some(map), None, headers).await?;
            sqlx::query("DELETE FROM route_permissions WHERE id = ?")
                .bind(id)
                .execute(&mut **tx)
                .await
                .map_err(internal)?;
            sum.removed += 1;
        }
    }

    Ok(sum)
}

/// Mengubah uri `Route` menjadi `route_path` seperti `RoutePermissionSyncService`.
fn route_path(uri: &str) -> String {
    static PARAM: OnceLock<Regex> = OnceLock::new();
    let param = PARAM.get_or_init(|| Regex::new(r"\{([a-zA-Z0-9_?]+)\}").expect("regex valid"));

    let clean = uri.strip_prefix("api/").unwrap_or(uri);
    let raw = format!("/{}", param.replace_all(clean, ":$1"));
    let no_q: String = raw.chars().filter(|c| *c != '?').collect();

    let mut collapsed = String::with_capacity(no_q.len());
    let mut prev_slash = false;
    for c in no_q.chars() {
        if c == '/' {
            if prev_slash {
                continue;
            }
            prev_slash = true;
        } else {
            prev_slash = false;
        }
        collapsed.push(c);
    }

    let trimmed = collapsed.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Nilai falsy PHP untuk string: `""` dan `"0"`.
fn php_truthy(s: &str) -> bool {
    !(s.is_empty() || s == "0")
}

/// Aturan `boolean` Laravel: true, false, 1, 0, "1", "0".
fn bool_value(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) if n.as_i64() == Some(1) => Some(true),
        Value::Number(n) if n.as_i64() == Some(0) => Some(false),
        Value::String(s) if s == "1" => Some(true),
        Value::String(s) if s == "0" => Some(false),
        _ => None,
    }
}

/// `TrimStrings` lalu `ConvertEmptyStringsToNull` untuk nilai string di tingkat atas.
fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m
            .into_iter()
            .map(|(k, v)| {
                let v = match v {
                    Value::String(s) => {
                        let t = s.trim();
                        if t.is_empty() {
                            Value::Null
                        } else {
                            Value::String(t.to_string())
                        }
                    }
                    other => other,
                };
                (k, v)
            })
            .collect(),
        _ => Map::new(),
    }
}

/// Bentuk model `RoutePermission` dari baris, sama dengan `route_permissions::row_json`.
fn row_map(r: &MySqlRow) -> Result<(u64, String, String, Map<String, Value>), sqlx::Error> {
    let id: u64 = r.try_get("id")?;
    let route_path: String = r.try_get("route_path")?;
    let route_method: String = r.try_get("route_method")?;
    let description: Option<String> = r.try_get("description")?;
    let raw: Option<String> = r.try_get("allowed_roles")?;
    let roles: Vec<String> = raw
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    let is_active = r.try_get::<i64, _>("is_active")? != 0;
    let created: Option<DateTime<Utc>> = r.try_get("created_at")?;
    let updated: Option<DateTime<Utc>> = r.try_get("updated_at")?;

    let mut m = Map::new();
    m.insert("id".into(), json!(id));
    m.insert("route_path".into(), json!(route_path));
    m.insert("route_method".into(), json!(route_method));
    m.insert("description".into(), json!(description));
    m.insert("allowed_roles".into(), json!(roles));
    m.insert("is_active".into(), json!(is_active));
    m.insert("created_at".into(), carbon_json(created));
    m.insert("updated_at".into(), carbon_json(updated));
    Ok((id, route_path, route_method, m))
}

async fn load_map(
    tx: &mut Transaction<'_, MySql>,
    id: u64,
) -> Result<Map<String, Value>, ApiError> {
    let r = sqlx::query(&format!("{ROW_COLUMNS} WHERE id = ?"))
        .bind(id)
        .fetch_one(&mut **tx)
        .await
        .map_err(internal)?;
    Ok(row_map(&r).map_err(internal)?.3)
}

async fn write_audit(
    tx: &mut Transaction<'_, MySql>,
    actor: u64,
    event: &str,
    id: u64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    audit::write(
        tx,
        audit::Entry {
            actor,
            event,
            auditable_type: MODEL,
            auditable_id: id,
            old,
            new,
            url: SYNC_PATH,
        },
        headers,
    )
    .await
    .map_err(internal)
}

/// SHA-256 gabungan berkas yang memengaruhi daftar rute: `routes/*.php`, `config/*.php`,
/// `app/Providers/*.php`, `bootstrap/*.php`, dan `composer.lock`. Tiap berkas masuk sebagai
/// `path NUL isi NUL`, diurutkan menurut path.
#[cfg(test)]
fn source_digest() -> String {
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};

    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.."));
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in ["routes", "config", "app/Providers", "bootstrap"] {
        let entries = std::fs::read_dir(root.join(dir)).expect("direktori sumber rute");
        for e in entries {
            let p = e.expect("entri direktori").path();
            if p.is_file() && p.extension().is_some_and(|x| x == "php") {
                files.push(p);
            }
        }
    }
    files.push(root.join("composer.lock"));
    files.sort();

    let mut h = Sha256::new();
    for f in &files {
        let rel = f
            .strip_prefix(root)
            .expect("di bawah root")
            .to_string_lossy();
        h.update(rel.as_bytes());
        h.update([0u8]);
        h.update(std::fs::read(f).expect("baca berkas sumber"));
        h.update([0u8]);
    }
    format!("{:x}", h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_table_matches_sources() {
        let actual = source_digest();
        assert_eq!(
            actual, ROUTES_SOURCE_SHA256,
            "berkas sumber rute berubah. Buat ulang ROUTES (lihat dokumentasi modul), lalu setel ROUTES_SOURCE_SHA256 = \"{actual}\""
        );
    }

    #[test]
    fn route_path_matches_laravel_normalisation() {
        assert_eq!(
            route_path("api/pekerjaan/{pekerjaan}/media"),
            "/pekerjaan/:pekerjaan/media"
        );
        assert_eq!(route_path("api/desa/{id?}"), "/desa/:id");
        assert_eq!(route_path("api"), "/api");
        assert_eq!(route_path("api/"), "/");
        assert_eq!(route_path("/"), "/");
        assert_eq!(route_path("storage/{path}"), "/storage/:path");
        assert_eq!(route_path("broadcasting/auth"), "/broadcasting/auth");
        assert_eq!(route_path("api//a///b/"), "/a/b");
    }

    #[test]
    fn php_truthy_matches_php_strings() {
        assert!(!php_truthy(""));
        assert!(!php_truthy("0"));
        assert!(php_truthy("api"));
        assert!(php_truthy("00"));
    }

    #[test]
    fn boolean_rule_accepts_laravel_values_only() {
        assert_eq!(bool_value(&json!(true)), Some(true));
        assert_eq!(bool_value(&json!(0)), Some(false));
        assert_eq!(bool_value(&json!("1")), Some(true));
        assert_eq!(bool_value(&json!("true")), None);
        assert_eq!(bool_value(&json!(2)), None);
        assert_eq!(bool_value(&Value::Null), None);
    }

    #[test]
    fn table_contains_sync_route_as_post() {
        let e = ROUTES
            .iter()
            .find(|r| r.uri == "api/route-permissions/sync")
            .expect("rute sync ada");
        assert_eq!(e.methods, &["POST"]);
        assert_eq!(e.name, None);
    }
}
