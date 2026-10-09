//! `POST /api/spam-units/import`, setara `SpamUnitController@import` dan command `spam:import-data`
//! (`ImportSpamData`). Rute ada di grup `auth:sanctum`; pembatasan admin mengikuti modul spam-units
//! (middleware permission `/spam-units`).
//!
//! Alur yang diikuti:
//! 1. Validasi field `file`: wajib, berkas dengan ekstensi csv/txt, maksimal 10240 KB. Gagal → 422.
//! 2. Berkas disimpan sebagai `import_spam_data.csv`, lalu command dijalankan dengan path itu.
//! 3. Respon selalu 200 `{message: "Data successfully imported", output}`, termasuk saat command gagal
//!    (kode keluar Laravel diabaikan). Output berisi pesan command, sama seperti `Artisan::output()`.
//!    Kegagalan menyimpan berkas atau memulai transaksi memberi 500 `{message: "Failed to import data", error}`.
//! 4. Di dalam satu transaksi: `DELETE FROM tbl_spam_budgets` (SEMUA anggaran, termasuk input manual),
//!    lalu tiap baris CSV dicocokkan ke kecamatan, desa, dan unit SPAM. Desa, unit, dan pengelola
//!    dibuat bila belum ada. Jika ada error, transaksi di-rollback dan output berisi
//!    `Failed to import budgets: ...`.
//!
//! Lokasi berkas: env `SPAM_IMPORT_DIR`, default `storage/app/temp` di root repo (sama dengan
//! `storage_path('app/temp')`).
//!
//! Aturan CSV: delimiter `;`, baris pertama dilewati sebagai header, baris dengan kurang dari 5 kolom
//! dilewati, dan baris dengan desa atau kecamatan kosong (termasuk `"0"`, sesuai `empty()` PHP) dilewati
//! tanpa dihitung. Kecamatan `kaupandak` diganti `Kadupandak`. Pencocokan kecamatan dan desa memakai
//! `LOWER(...) = ?`, lalu fallback `LIKE '%nama%'` tanpa escape. Nilai kontrak meniru `(double)` PHP.
//!
//! Efek samping per baris yang dibuat (`Auditable` dan `NotifiesAdminsOnChanges`): audit `created`
//! dan notifikasi `Data {Model} dibuat` ke semua admin kecuali pengimpor. Laravel menjalankan ini karena
//! `runningInConsole()` bernilai false saat command dipanggil dari request HTTP.
//!
//! Berbeda dari Laravel:
//! - Laravel menyimpan dengan `storeAs` ke disk default (`FILESYSTEM_DISK`, root `storage/app/public`
//!   atau `storage/app/private`), tetapi command membaca `storage/app/temp`. Kedua lokasi berbeda, jadi
//!   dengan konfigurasi standar Laravel selalu menghasilkan "CSV file not found" dengan status 200. Rust
//!   menulis dan membaca lokasi yang sama. Ini mengubah perilaku endpoint: impor benar-benar berjalan dan
//!   menghapus semua anggaran. Konfirmasi dulu sebelum rute ini diaktifkan di Apache.
//! - Cache `dashboard_stats_version` (`Cache::increment`) tidak dipindah karena Rust tidak punya versi cache
//!   dashboard.
//! - Mime divalidasi dari ekstensi nama berkas, bukan dari isi berkas seperti `mimes:` Laravel.
//! - Isi CSV dibaca sebagai UTF-8 dengan penggantian (`U+FFFD`). Laravel mengirim byte mentah ke MySQL.
//! - `fgetcsv` Laravel dibatasi 1000 byte per baris. Rust membaca baris penuh. Parser berhenti di baris
//!   yang tidak bisa dibaca, sedangkan PHP melanjutkan.
//! - Escape `\` dipakai untuk struktur CSV, tetapi nilainya tidak mempertahankan backslash seperti PHP.
//! - Pesan error database berisi pesan sqlx, bukan pesan `QueryException` Laravel yang memuat SQL.
//! - Error saat mengirim notifikasi ditelan, sama dengan `try/catch` di `notifyAdmins`.
//! - Output memuat path absolut berkas di server, sama dengan Laravel. Hanya admin yang bisa melihatnya.
//! - Body di atas `BODY_LIMIT` (11 MiB) atau berkas di atas 10240 KB dijawab 422 `max`.
//! - `GET /api/spam-units/import` diperkirakan dijawab 405 oleh router Axum, sedangkan Laravel menjawab 404 dari
//!   `show` (belum diuji). Apache sebaiknya hanya meneruskan POST.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use axum::{
    body::Bytes,
    extract::{FromRequest, Multipart, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Transaction};

use crate::{
    audit,
    foto::base_url,
    notify,
    require_auth,
    spam_integration::{Ctx, MODEL_BUDGET, MODEL_UNIT},
    AppState,
};

const MODEL_DESA: &str = "App\\Models\\Desa";
const MODEL_PENGELOLA: &str = "App\\Models\\Pengelola";

const FILE_NAME: &str = "import_spam_data.csv";
const MAX_KB: usize = 10240;
const MAX_BYTES: usize = MAX_KB * 1024;
/// Batas body untuk layer `DefaultBodyLimit`: berkas maksimal ditambah overhead multipart.
pub const BODY_LIMIT: usize = MAX_BYTES + 1024 * 1024;

const MSG_REQUIRED: &str = "The file field is required.";
const MSG_MIMES: &str = "The file field must be a file of type: csv, txt.";
const MSG_MAX: &str = "The file field must not be greater than 10240 kilobytes.";

/// Direktori penyimpanan berkas. Dapat diganti dengan `SPAM_IMPORT_DIR`.
pub fn import_dir() -> PathBuf {
    std::env::var_os("SPAM_IMPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../storage/app/temp"
            ))
        })
}

// ---------------------------------------------------------------------------
// Upload dan validasi
// ---------------------------------------------------------------------------

enum Upload {
    Missing,
    File { name: String, bytes: Bytes },
    /// Body atau berkas melewati batas. Nama dibaca dari header bagian sebelum body dipotong.
    TooLarge { name: String },
}

fn is_csv_or_txt(name: &str) -> bool {
    matches!(
        Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase()),
        Some(ref e) if e == "csv" || e == "txt"
    )
}

async fn read_upload(multipart: &mut Multipart) -> Upload {
    let mut upload = Upload::Missing;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    return Upload::TooLarge { name: String::new() };
                }
                break;
            }
        };
        if field.name() != Some("file") {
            continue;
        }
        // Tanpa nama berkas, PHP menganggap tidak ada berkas (UPLOAD_ERR_NO_FILE).
        let name = field.file_name().unwrap_or_default().to_string();
        if name.is_empty() {
            continue;
        }
        match field.bytes().await {
            Ok(bytes) => upload = Upload::File { name, bytes },
            Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                return Upload::TooLarge { name };
            }
            Err(_) => break,
        }
    }
    upload
}

/// Aturan `required|file|mimes:csv,txt|max:10240` dengan pesan Laravel. Mengembalikan isi berkas bila lolos.
fn check_upload(upload: Upload) -> Result<Bytes, ApiError> {
    let mut msgs: Vec<&str> = Vec::new();
    let bytes = match upload {
        Upload::Missing => {
            msgs.push(MSG_REQUIRED);
            None
        }
        Upload::File { name, bytes } => {
            // Berkas kosong gagal `mimes` karena MIME-nya `application/x-empty`.
            if !is_csv_or_txt(&name) || bytes.is_empty() {
                msgs.push(MSG_MIMES);
            }
            if bytes.len() > MAX_BYTES {
                msgs.push(MSG_MAX);
            }
            Some(bytes)
        }
        Upload::TooLarge { name } => {
            if !name.is_empty() && !is_csv_or_txt(&name) {
                msgs.push(MSG_MIMES);
            }
            msgs.push(MSG_MAX);
            None
        }
    };
    if msgs.is_empty() {
        if let Some(b) = bytes {
            return Ok(b);
        }
    }
    let mut errs = BTreeMap::new();
    errs.insert(
        "file".to_string(),
        msgs.into_iter().map(str::to_string).collect::<Vec<_>>(),
    );
    Err(ApiError::validation("The given data was invalid.", errs))
}

// ---------------------------------------------------------------------------
// Helper PHP
// ---------------------------------------------------------------------------

/// `trim()` PHP: membuang ` \t\n\r\0\x0B` di kedua ujung.
fn php_trim(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\0' | '\x0B'))
}

/// `empty()` PHP untuk string: `""` dan `"0"` dianggap kosong.
fn php_empty(s: &str) -> bool {
    s.is_empty() || s == "0"
}

/// Cast `(double)` PHP untuk string: awalan numerik setelah spasi, selain itu 0.
fn php_double(s: &str) -> f64 {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
        i += 1;
    }
    let start = i;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i < b.len() && b[i] == b'.' {
        let mut k = i + 1;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        frac_digits = k - (i + 1);
        if int_digits + frac_digits > 0 {
            i = k;
        }
    }
    if int_digits + frac_digits == 0 {
        return 0.0;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut k = i + 1;
        if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
            k += 1;
        }
        if k < b.len() && b[k].is_ascii_digit() {
            while k < b.len() && b[k].is_ascii_digit() {
                k += 1;
            }
            i = k;
        }
    }
    s[start..i].parse::<f64>().unwrap_or(0.0)
}

/// Baris CSV dengan delimiter `;`, quote `"`, dan escape `\`, seperti `fgetcsv` PHP.
pub fn parse_records(bytes: &[u8]) -> Vec<csv::ByteRecord> {
    csv::ReaderBuilder::new()
        .delimiter(b';')
        .quote(b'"')
        .escape(Some(b'\\'))
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes)
        .byte_records()
        .map_while(Result::ok)
        .collect()
}

fn field(rec: &csv::ByteRecord, i: usize) -> String {
    String::from_utf8_lossy(&rec[i]).into_owned()
}

// ---------------------------------------------------------------------------
// Audit dan notifikasi untuk model yang dibuat
// ---------------------------------------------------------------------------

/// Audit `created` lalu notifikasi admin, setara `Auditable` dan `NotifiesAdminsOnChanges` saat `create()`.
/// Error audit dikembalikan (membatalkan impor). Error notifikasi ditelan, seperti `try/catch` Laravel.
async fn audit_created(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    actor: u64,
    model: &str,
    id: i64,
    new: Map<String, Value>,
) -> Result<(), sqlx::Error> {
    audit::write(
        tx,
        audit::Entry {
            actor,
            event: "created",
            auditable_type: model,
            auditable_id: id as u64,
            old: None,
            new: Some(new),
            url: ctx.url,
        },
        ctx.headers,
    )
    .await?;
    let short = model.rsplit('\\').next().unwrap_or(model);
    let name = notify::actor_name(tx, actor).await.unwrap_or_default();
    let message = notify::change_message(short, id as u64, "dibuat", &name, false);
    let title = format!("Data {short} dibuat");
    let _ = notify::admins(tx, actor, &title, &message, None).await;
    Ok(())
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Inti impor
// ---------------------------------------------------------------------------

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Summary {
    pub rows: u64,
    pub matched: u64,
    pub created_desa: u64,
    pub created_unit: u64,
}

async fn find_kec(
    tx: &mut Transaction<'_, MySql>,
    name: &str,
) -> Result<Option<(i64, String)>, sqlx::Error> {
    let exact: Option<(i64, String)> = sqlx::query_as(
        "SELECT CAST(id AS SIGNED), n_kec FROM tbl_kecamatan WHERE LOWER(n_kec) = ? LIMIT 1",
    )
    .bind(name.to_ascii_lowercase())
    .fetch_optional(&mut **tx)
    .await?;
    if exact.is_some() {
        return Ok(exact);
    }
    // Fallback typo atau fuzzy: `like "%nama%"` tanpa escape, sama dengan Laravel.
    sqlx::query_as(
        "SELECT CAST(id AS SIGNED), n_kec FROM tbl_kecamatan WHERE n_kec LIKE ? LIMIT 1",
    )
    .bind(format!("%{name}%"))
    .fetch_optional(&mut **tx)
    .await
}

async fn find_desa(
    tx: &mut Transaction<'_, MySql>,
    kec_id: i64,
    name: &str,
) -> Result<Option<(i64, Option<String>)>, sqlx::Error> {
    let exact: Option<(i64, Option<String>)> = sqlx::query_as(
        "SELECT CAST(id AS SIGNED), n_desa FROM tbl_desa WHERE kecamatan_id = ? AND (LOWER(n_desa) = ?) LIMIT 1",
    )
    .bind(kec_id)
    .bind(name.to_ascii_lowercase())
    .fetch_optional(&mut **tx)
    .await?;
    if exact.is_some() {
        return Ok(exact);
    }
    sqlx::query_as(
        "SELECT CAST(id AS SIGNED), n_desa FROM tbl_desa WHERE kecamatan_id = ? AND (n_desa LIKE ?) LIMIT 1",
    )
    .bind(kec_id)
    .bind(format!("%{name}%"))
    .fetch_optional(&mut **tx)
    .await
}

/// Isi transaksi: semua baris CSV (tanpa header). Dipanggil dengan transaksi milik pemanggil.
pub async fn import_rows(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    actor: u64,
    rows: &[csv::ByteRecord],
    out: &mut String,
) -> Result<Summary, sqlx::Error> {
    // `SpamBudget::query()->delete()`: hapus semua anggaran lewat query builder (tanpa event per baris).
    sqlx::query("DELETE FROM tbl_spam_budgets")
        .execute(&mut **tx)
        .await?;

    let mut s = Summary::default();
    for rec in rows {
        if rec.len() < 5 {
            continue;
        }
        let nilai_str = php_trim(&field(rec, 0)).to_string();
        let tahun = php_trim(&field(rec, 1)).to_string();
        let nama_paket = php_trim(&field(rec, 2)).to_string();
        let desa_name = php_trim(&field(rec, 3)).to_string();
        let mut kec_name = php_trim(&field(rec, 4)).to_string();

        if php_empty(&desa_name) || php_empty(&kec_name) {
            continue;
        }
        if kec_name.to_ascii_lowercase() == "kaupandak" {
            kec_name = "Kadupandak".to_string();
        }
        s.rows += 1;

        let Some((kec_id, kec_db_name)) = find_kec(tx, &kec_name).await? else {
            out.push_str(&format!(
                "Row {}: Kecamatan '{kec_name}' not found. Skipping.\n",
                s.rows
            ));
            continue;
        };

        let (desa_id, desa_db_name) = match find_desa(tx, kec_id, &desa_name).await? {
            Some(d) => d,
            None => {
                let id = sqlx::query(
                    "INSERT INTO tbl_desa (kecamatan_id, n_desa, bjp_master, created_at, updated_at) \
                     VALUES (?, ?, 0, NOW(), NOW())",
                )
                .bind(kec_id)
                .bind(&desa_name)
                .execute(&mut **tx)
                .await?
                .last_insert_id() as i64;
                s.created_desa += 1;
                audit_created(
                    tx,
                    ctx,
                    actor,
                    MODEL_DESA,
                    id,
                    obj(json!({ "kecamatan_id": kec_id, "n_desa": desa_name, "bjp_master": 0, "id": id })),
                )
                .await?;
                (id, Some(desa_name.clone()))
            }
        };
        let desa_upper = desa_db_name.unwrap_or_default().to_ascii_uppercase();
        let kec_upper = kec_db_name.to_ascii_uppercase();

        let unit_id: Option<i64> = sqlx::query_scalar(
            "SELECT CAST(id AS SIGNED) FROM tbl_unit_spam WHERE desa_id = ? LIMIT 1",
        )
        .bind(desa_id)
        .fetch_optional(&mut **tx)
        .await?;
        let unit_id = match unit_id {
            Some(id) => id,
            None => {
                let unit_name = format!("SPAM {desa_upper} {kec_upper}");
                let id = sqlx::query(
                    "INSERT INTO tbl_unit_spam (desa_id, name, is_simspam, sistem_layanan, sumber_mata_air_kap, \
                     sumber_air_tanah_kap, created_at, updated_at) VALUES (?, ?, 0, 'Perpipaan', '0', '0', NOW(), NOW())",
                )
                .bind(desa_id)
                .bind(&unit_name)
                .execute(&mut **tx)
                .await?
                .last_insert_id() as i64;
                audit_created(
                    tx,
                    ctx,
                    actor,
                    MODEL_UNIT,
                    id,
                    obj(json!({
                        "desa_id": desa_id, "name": unit_name, "is_simspam": false,
                        "sistem_layanan": "Perpipaan", "sumber_mata_air_kap": "0",
                        "sumber_air_tanah_kap": "0", "id": id,
                    })),
                )
                .await?;

                let pokmas = format!("KPSPAM {desa_upper} {kec_upper}");
                let pid = sqlx::query(
                    "INSERT INTO tbl_pengelola (unit_spam_id, pokmas, kepala, bendahara, sekretaris, created_at, updated_at) \
                     VALUES (?, ?, '-', '-', '-', NOW(), NOW())",
                )
                .bind(id)
                .bind(&pokmas)
                .execute(&mut **tx)
                .await?
                .last_insert_id() as i64;
                audit_created(
                    tx,
                    ctx,
                    actor,
                    MODEL_PENGELOLA,
                    pid,
                    obj(json!({
                        "unit_spam_id": id, "pokmas": pokmas, "kepala": "-",
                        "bendahara": "-", "sekretaris": "-", "id": pid,
                    })),
                )
                .await?;
                s.created_unit += 1;
                id
            }
        };

        // `str_replace(['Rp', '.'], '')`, lalu `explode(',')`, lalu `(double)` seperti di command.
        let clean = nilai_str.replace("Rp", "").replace('.', "");
        let mut parts = clean.split(',');
        let whole = parts.next().unwrap_or("");
        let nilai = match parts.next() {
            Some(frac) => php_double(&format!("{whole}.{frac}")),
            None => php_double(whole),
        };

        let budget_id = sqlx::query(
            "INSERT INTO tbl_spam_budgets (unit_spam_id, nilai_kontrak, tahun, nama_paket, sumber_dana, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 'APBD', NOW(), NOW())",
        )
        .bind(unit_id)
        .bind(nilai)
        .bind(&tahun)
        .bind(&nama_paket)
        .execute(&mut **tx)
        .await?
        .last_insert_id() as i64;
        audit_created(
            tx,
            ctx,
            actor,
            MODEL_BUDGET,
            budget_id,
            obj(json!({
                "unit_spam_id": unit_id, "nilai_kontrak": nilai, "tahun": tahun,
                "nama_paket": nama_paket, "sumber_dana": "APBD", "id": budget_id,
            })),
        )
        .await?;
        s.matched += 1;
    }
    Ok(s)
}

/// Setara `spam:import-data --file=...`. Mengembalikan teks output yang sama dengan `Artisan::output()`.
/// Error `Err` hanya untuk kegagalan memulai transaksi, yang di Laravel melempar keluar dari command.
pub async fn run_command(
    pool: &MySqlPool,
    ctx: &Ctx<'_>,
    actor: u64,
    path: &Path,
) -> Result<String, sqlx::Error> {
    let shown = path.display().to_string();
    let mut out = String::new();

    if !path.exists() {
        out.push_str(&format!("CSV file not found at: {shown}\n"));
        return Ok(out);
    }
    out.push_str(&format!("Opening and parsing SPSE CSV file from: {shown}...\n"));
    let bytes = match tokio::fs::read(path).await {
        Ok(b) => b,
        Err(_) => {
            out.push_str("Failed to open CSV file.\n");
            return Ok(out);
        }
    };
    let mut records = parse_records(&bytes);
    if records.is_empty() {
        out.push_str("CSV file is empty.\n");
        return Ok(out);
    }
    records.remove(0);

    out.push_str("Beginning transaction to import budget packages...\n");
    let mut tx = pool.begin().await?;
    match import_rows(&mut tx, ctx, actor, &records, &mut out).await {
        Ok(s) => match tx.commit().await {
            Ok(()) => {
                out.push_str("SPAM Budgets imported successfully!\n");
                out.push_str(&format!("- Total rows processed: {}\n", s.rows));
                out.push_str(&format!("- Successfully mapped budgets: {}\n", s.matched));
                out.push_str(&format!("- Dynamically created villages (Desa): {}\n", s.created_desa));
                out.push_str(&format!("- Dynamically created SPAM units: {}\n", s.created_unit));
            }
            Err(e) => out.push_str(&format!("Failed to import budgets: {e}\n")),
        },
        Err(e) => {
            // `tx` di-rollback saat dibuang (tanpa commit).
            drop(tx);
            out.push_str(&format!("Failed to import budgets: {e}\n"));
        }
    }
    Ok(out)
}

fn failed(detail: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "message": "Failed to import data", "error": detail })),
    )
        .into_response()
}

/// `POST /api/spam-units/import` (multipart, field `file`).
pub async fn import(State(state): State<AppState>, request: Request) -> Result<Response, ApiError> {
    // Auth dijalankan lebih dulu (middleware `auth:sanctum` di Laravel), baru body multipart dibaca.
    let headers = request.headers().clone();
    let user = require_auth(&state, &headers).await?;
    let upload = match Multipart::from_request(request, &state).await {
        Ok(mut multipart) => read_upload(&mut multipart).await,
        // Bukan multipart: Laravel melihat tidak ada berkas, jadi `required`.
        Err(_) => Upload::Missing,
    };
    let bytes = check_upload(upload)?;

    let dir = import_dir();
    let path = dir.join(FILE_NAME);
    let stored = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(&path, &bytes).await
    };
    if let Err(e) = stored.await {
        return Ok(failed(e.to_string()));
    }

    let url = format!("{}/api/spam-units/import", base_url(&state));
    let ctx = Ctx {
        user: Some(user.user_id),
        roles: &[],
        url: &url,
        headers: &headers,
    };
    let output = match run_command(&state.pool, &ctx, user.user_id, &path).await {
        Ok(o) => o,
        Err(e) => return Ok(failed(e.to_string())),
    };

    Ok(Json(json!({ "message": "Data successfully imported", "output": output })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_double_matches_cast() {
        assert_eq!(php_double("1250000.50"), 1250000.5);
        assert_eq!(php_double(" 1000"), 1000.0);
        assert_eq!(php_double("1e3"), 1000.0);
        assert_eq!(php_double("1.2.3"), 1.2);
        assert_eq!(php_double("1."), 1.0);
        assert_eq!(php_double(".5"), 0.5);
        assert_eq!(php_double("-5"), -5.0);
        assert_eq!(php_double("abc"), 0.0);
        assert_eq!(php_double(""), 0.0);
        assert_eq!(php_double("."), 0.0);
    }

    #[test]
    fn php_empty_treats_zero_as_empty() {
        assert!(php_empty(""));
        assert!(php_empty("0"));
        assert!(!php_empty("00"));
        assert!(!php_empty(" "));
    }

    #[test]
    fn php_trim_strips_php_whitespace_only() {
        assert_eq!(php_trim(" \t\0a b\x0B\n"), "a b");
        assert_eq!(php_trim("\u{a0}x"), "\u{a0}x");
    }

    #[test]
    fn csv_rules_skip_header_and_blank_lines() {
        let recs = parse_records(b"h1;h2;h3;h4;h5\n\n1;2;3;4;5\n6;7\n");
        assert_eq!(recs.len(), 3);
        assert_eq!(field(&recs[1], 4), "5");
    }

    #[test]
    fn csv_quoted_field_with_semicolon() {
        let recs = parse_records(b"h\n\"a;b\";2;3;4;5\n");
        assert_eq!(field(&recs[1], 0), "a;b");
    }

    #[test]
    fn upload_rules_match_laravel_messages() {
        let err = check_upload(Upload::Missing).unwrap_err();
        assert_eq!(err.errors.unwrap()["file"], vec![MSG_REQUIRED.to_string()]);

        let err = check_upload(Upload::File {
            name: "a.xlsx".into(),
            bytes: Bytes::from_static(b"x"),
        })
        .unwrap_err();
        assert_eq!(err.errors.unwrap()["file"], vec![MSG_MIMES.to_string()]);

        let err = check_upload(Upload::File {
            name: "a.csv".into(),
            bytes: Bytes::new(),
        })
        .unwrap_err();
        assert_eq!(err.errors.unwrap()["file"], vec![MSG_MIMES.to_string()]);

        let ok = check_upload(Upload::File {
            name: "A.CSV".into(),
            bytes: Bytes::from_static(b"x"),
        });
        assert!(ok.is_ok());
        assert!(is_csv_or_txt("data.txt"));
        assert!(!is_csv_or_txt("csv"));
    }

    #[test]
    fn upload_too_large_reports_max() {
        let err = check_upload(Upload::TooLarge { name: "a.csv".into() }).unwrap_err();
        assert_eq!(err.errors.unwrap()["file"], vec![MSG_MAX.to_string()]);
    }
}
