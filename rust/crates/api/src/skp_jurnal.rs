//! Jurnal kegiatan SKP: tabel `skp_jurnal_kegiatan` (migrasi `0001_skp_jurnal_kegiatan`).
//!
//! Modul baru, tidak ada padanannya di Laravel. Rute:
//! - `GET /api/skp/jurnal?tahun=YYYY&bulan=M`: kegiatan satu bulan milik pengguna, terbaru dulu, plus `ringkasan`.
//! - `POST /api/skp/jurnal`: tambah kegiatan, 201.
//! - `PUT /api/skp/jurnal/{id}`: ubah kegiatan, 200. Id milik pengguna lain dianggap tidak ada (404).
//! - `DELETE /api/skp/jurnal/{id}`: hapus, 204.
//!
//! - `POST /api/skp/jurnal/{id}/foto`: unggah foto bukti (multipart `foto` atau `foto[]`), JPG/PNG.
//! - `DELETE /api/skp/jurnal/{id}/foto/{media_id}`: hapus satu foto.
//!
//! Setiap pengguna yang login mengelola jurnalnya sendiri. Tidak ada role khusus.
//! Foto disimpan lewat tabel `media` (model `App\Models\SkpJurnalKegiatan`, koleksi `foto`).

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{FromRequest, Multipart, Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySqlPool, Row};

use crate::{
    format::iso8601_utc,
    media::{self, internal},
    require_auth, AppState,
};

const TABLE: &str = "skp_jurnal_kegiatan";
/// `CAST` pada `rhk` karena kolom SMALLINT UNSIGNED.
const COLS: &str = "id, tanggal, CAST(rhk AS SIGNED) AS rhk, kegiatan, output, satuan, keterangan, created_at, updated_at";
const MAX_KEGIATAN: usize = 1000;
const MAX_SATUAN: usize = 50;
const MAX_KETERANGAN: usize = 500;
const MAX_RHK: i64 = 99;
const VALIDATION_MESSAGE: &str = "The given data was invalid.";
const MODEL: &str = "App\\Models\\SkpJurnalKegiatan";
const COLLECTION: &str = "foto";
/// Foto per kegiatan.
const MAX_FOTO: usize = 10;
const MAX_FOTO_BYTES: usize = 10 * 1024 * 1024;
const FOTO_EXT: &[&str] = &["jpg", "jpeg", "png"];
/// Batas body satu request unggah (beberapa foto sekaligus), sama dengan modul foto lain.
pub const BODY_LIMIT: usize = media::MAX_FILE_BYTES + 1024 * 1024;

#[derive(Debug, Clone)]
struct Entry {
    id: u64,
    tanggal: NaiveDate,
    rhk: Option<i64>,
    kegiatan: String,
    output: Option<f64>,
    satuan: Option<String>,
    keterangan: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    fotos: Vec<Foto>,
}

#[derive(Debug, Clone, PartialEq)]
struct Foto {
    id: u64,
    url: String,
    thumb: String,
}

/// Input yang sudah lolos validasi.
#[derive(Debug, Clone, PartialEq)]
struct Input {
    tanggal: NaiveDate,
    rhk: Option<i64>,
    kegiatan: String,
    output: Option<f64>,
    satuan: Option<String>,
    keterangan: Option<String>,
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn parse_id(raw: &str) -> Result<u64, ApiError> {
    raw.parse::<u64>().map_err(|_| ApiError::not_found())
}

/// `TrimStrings` dan `ConvertEmptyStringsToNull`: string kosong dianggap tidak diisi.
fn normalize(v: Value) -> Value {
    match v {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Value::Null
            } else {
                Value::String(t.to_string())
            }
        }
        other => other,
    }
}

fn gagal(errors: &mut BTreeMap<String, Vec<String>>, field: &str, message: &str) {
    errors
        .entry(field.to_string())
        .or_default()
        .push(message.to_string());
}

/// Bilangan bulat dari JSON, termasuk `2.0`. Pecahan ditolak.
fn as_int(v: &Value) -> Option<i64> {
    if let Some(i) = v.as_i64() {
        return Some(i);
    }
    v.as_f64()
        .filter(|f| f.fract() == 0.0 && f.abs() < 1e15)
        .map(|f| f as i64)
}

/// Angka dari JSON. Terima juga string dengan koma desimal (`"2,5"`) seperti input di formulir.
fn as_number(v: &Value) -> Option<f64> {
    let n = match v {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.replace(',', ".").parse::<f64>().ok()?,
        _ => return None,
    };
    n.is_finite().then_some(n)
}

/// Validasi body store dan update. Semua error dikumpulkan dalam satu respons 422.
fn validate(body: &Value) -> Result<Input, ApiError> {
    let get = |k: &str| {
        body.as_object()
            .and_then(|o| o.get(k))
            .cloned()
            .map(normalize)
            .unwrap_or(Value::Null)
    };
    let mut errors = BTreeMap::new();

    let tanggal = match get("tanggal") {
        Value::Null => {
            gagal(&mut errors, "tanggal", "Tanggal wajib diisi.");
            None
        }
        Value::String(s) => match NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
            Ok(d) => Some(d),
            Err(_) => {
                gagal(
                    &mut errors,
                    "tanggal",
                    "Tanggal harus berformat YYYY-MM-DD.",
                );
                None
            }
        },
        _ => {
            gagal(
                &mut errors,
                "tanggal",
                "Tanggal harus berformat YYYY-MM-DD.",
            );
            None
        }
    };

    let rhk = match get("rhk") {
        Value::Null => None,
        v => match as_int(&v).filter(|n| (1..=MAX_RHK).contains(n)) {
            Some(n) => Some(n),
            None => {
                gagal(&mut errors, "rhk", "RHK harus angka 1 sampai 99.");
                None
            }
        },
    };

    let kegiatan = match get("kegiatan") {
        Value::String(s) if s.chars().count() <= MAX_KEGIATAN => Some(s),
        Value::String(_) => {
            gagal(
                &mut errors,
                "kegiatan",
                "Uraian kegiatan maksimal 1000 karakter.",
            );
            None
        }
        _ => {
            gagal(&mut errors, "kegiatan", "Uraian kegiatan wajib diisi.");
            None
        }
    };

    let output = match get("output") {
        Value::Null => None,
        v => match as_number(&v).filter(|n| *n >= 0.0) {
            Some(n) => Some(n),
            None => {
                gagal(
                    &mut errors,
                    "output",
                    "Output harus berupa angka tidak negatif.",
                );
                None
            }
        },
    };

    let satuan = optional_text(
        &get("satuan"),
        MAX_SATUAN,
        "satuan",
        "Satuan maksimal 50 karakter.",
        &mut errors,
    );
    let keterangan = optional_text(
        &get("keterangan"),
        MAX_KETERANGAN,
        "keterangan",
        "Keterangan maksimal 500 karakter.",
        &mut errors,
    );

    if !errors.is_empty() {
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }
    Ok(Input {
        tanggal: tanggal.expect("tanggal ada bila tidak ada error"),
        rhk,
        kegiatan: kegiatan.expect("kegiatan ada bila tidak ada error"),
        output,
        satuan,
        keterangan,
    })
}

fn optional_text(
    v: &Value,
    max: usize,
    field: &str,
    message: &str,
    errors: &mut BTreeMap<String, Vec<String>>,
) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) if s.chars().count() <= max => Some(s.clone()),
        _ => {
            gagal(errors, field, message);
            None
        }
    }
}

/// Periode dari query `tahun` dan `bulan`. Keduanya wajib.
fn periode(query: &HashMap<String, String>) -> Result<(i32, u32), ApiError> {
    let mut errors = BTreeMap::new();
    let tahun = query
        .get("tahun")
        .and_then(|s| s.parse::<i32>().ok())
        .filter(|t| (2000..=2100).contains(t));
    if tahun.is_none() {
        gagal(&mut errors, "tahun", "Tahun harus antara 2000 dan 2100.");
    }
    let bulan = query
        .get("bulan")
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|b| (1..=12).contains(b));
    if bulan.is_none() {
        gagal(&mut errors, "bulan", "Bulan harus antara 1 dan 12.");
    }
    match (tahun, bulan) {
        (Some(t), Some(b)) if errors.is_empty() => Ok((t, b)),
        _ => Err(ApiError::validation(VALIDATION_MESSAGE, errors)),
    }
}

/// Rentang `[awal, akhir)` untuk satu bulan.
fn batas_bulan(tahun: i32, bulan: u32) -> Result<(NaiveDate, NaiveDate), ApiError> {
    let (ny, nm) = if bulan == 12 {
        (tahun + 1, 1)
    } else {
        (tahun, bulan + 1)
    };
    match (
        NaiveDate::from_ymd_opt(tahun, bulan, 1),
        NaiveDate::from_ymd_opt(ny, nm, 1),
    ) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(internal("periode tidak valid")),
    }
}

fn from_row(r: &MySqlRow) -> Result<Entry, sqlx::Error> {
    Ok(Entry {
        id: r.try_get("id")?,
        tanggal: r.try_get("tanggal")?,
        rhk: r.try_get("rhk")?,
        kegiatan: r.try_get("kegiatan")?,
        output: r.try_get("output")?,
        satuan: r.try_get("satuan")?,
        keterangan: r.try_get("keterangan")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
        fotos: Vec::new(),
    })
}

fn to_json(e: &Entry) -> Value {
    json!({
        "id": e.id,
        "tanggal": e.tanggal.format("%Y-%m-%d").to_string(),
        "rhk": e.rhk,
        "kegiatan": e.kegiatan,
        "output": e.output,
        "satuan": e.satuan,
        "keterangan": e.keterangan,
        "created_at": iso8601_utc(e.created_at),
        "updated_at": iso8601_utc(e.updated_at),
        "foto": e
            .fotos
            .iter()
            .map(|f| json!({ "id": f.id, "url": f.url, "thumb": f.thumb }))
            .collect::<Vec<_>>(),
    })
}

/// Ringkasan bulan: jumlah kegiatan, jumlah hari yang tercatat, dan total per RHK.
/// Kegiatan tanpa RHK dihitung di `jumlah` tapi tidak masuk `per_rhk`.
fn ringkasan(tahun: i32, bulan: u32, rows: &[Entry]) -> Value {
    let mut hari: Vec<NaiveDate> = rows.iter().map(|e| e.tanggal).collect();
    hari.sort();
    hari.dedup();

    let mut per: BTreeMap<i64, (u64, f64)> = BTreeMap::new();
    for e in rows {
        if let Some(rhk) = e.rhk {
            let slot = per.entry(rhk).or_insert((0, 0.0));
            slot.0 += 1;
            slot.1 += e.output.unwrap_or(0.0);
        }
    }

    json!({
        "tahun": tahun,
        "bulan": bulan,
        "jumlah": rows.len(),
        "hari": hari.len(),
        "per_rhk": per
            .into_iter()
            .map(|(rhk, (jumlah, output))| json!({ "rhk": rhk, "jumlah": jumlah, "output": output }))
            .collect::<Vec<_>>(),
    })
}

async fn find(pool: &MySqlPool, user_id: u64, id: u64) -> Result<Option<Entry>, ApiError> {
    let sql = format!("SELECT {COLS} FROM {TABLE} WHERE id = ? AND user_id = ?");
    let row = sqlx::query(&sql)
        .bind(id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    row.as_ref().map(from_row).transpose().map_err(internal)
}

/// Isi `fotos` semua entri dengan satu query ke `media`.
async fn isi_foto(pool: &MySqlPool, app_url: &str, entries: &mut [Entry]) -> Result<(), ApiError> {
    if entries.is_empty() {
        return Ok(());
    }
    let marks = vec!["?"; entries.len()].join(", ");
    let sql = format!(
        "SELECT CAST(id AS UNSIGNED) AS id, CAST(model_id AS UNSIGNED) AS model_id, file_name, \
         CAST(generated_conversions AS CHAR) AS generated_conversions FROM media \
         WHERE model_type = ? AND collection_name = ? AND model_id IN ({marks}) ORDER BY order_column, id"
    );
    let mut q = sqlx::query(&sql).bind(MODEL).bind(COLLECTION);
    for e in entries.iter() {
        q = q.bind(e.id);
    }
    let rows = q.fetch_all(pool).await.map_err(internal)?;
    for r in rows {
        let model_id: u64 = r.try_get("model_id").map_err(internal)?;
        let id: u64 = r.try_get("id").map_err(internal)?;
        let file_name: String = r.try_get("file_name").map_err(internal)?;
        let generated: String = r.try_get("generated_conversions").map_err(internal)?;
        let (url, thumb) = media::public_urls(app_url, id, &file_name, &generated);
        if let Some(e) = entries.iter_mut().find(|e| e.id == model_id) {
            e.fotos.push(Foto { id, url, thumb });
        }
    }
    Ok(())
}

/// Satu kegiatan milik pengguna, lengkap dengan foto. 404 bila bukan miliknya.
async fn muat(state: &AppState, user_id: u64, id: u64) -> Result<Entry, ApiError> {
    let mut entry = find(&state.pool, user_id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    isi_foto(
        &state.pool,
        &state.app_url,
        std::slice::from_mut(&mut entry),
    )
    .await?;
    Ok(entry)
}

fn bad_request(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, e.to_string())
}

/// Berkas `foto` atau `foto[]` dari multipart. Berkas kosong diabaikan.
async fn baca_foto(state: &AppState, req: Request) -> Result<Vec<media::Upload>, ApiError> {
    let mut files = Vec::new();
    let mut mp = Multipart::from_request(req, state)
        .await
        .map_err(bad_request)?;
    while let Some(field) = mp.next_field().await.map_err(bad_request)? {
        let name = field.name().unwrap_or_default().to_string();
        if name != "foto" && name != "foto[]" {
            continue;
        }
        let original_name = field.file_name().unwrap_or_default().to_string();
        let bytes = field.bytes().await.map_err(bad_request)?;
        if !bytes.is_empty() {
            files.push(media::Upload {
                original_name,
                bytes: bytes.to_vec(),
            });
        }
    }
    Ok(files)
}

/// Aturan foto: JPG/PNG, maksimal 10 MB per berkas, dan maksimal 10 foto per kegiatan.
fn validate_foto(files: &[media::Upload], sudah_ada: usize) -> Result<(), ApiError> {
    let mut errors = BTreeMap::new();
    if files.is_empty() {
        gagal(&mut errors, "foto", "Pilih minimal satu foto.");
    } else if sudah_ada + files.len() > MAX_FOTO {
        gagal(&mut errors, "foto", "Satu kegiatan maksimal 10 foto.");
    }
    for f in files {
        let ext = f.extension().to_ascii_lowercase();
        if !FOTO_EXT.contains(&ext.as_str()) || media::image_mime(&f.bytes).is_none() {
            gagal(
                &mut errors,
                "foto",
                "Foto harus berupa gambar JPG atau PNG.",
            );
        }
        if f.bytes.len() > MAX_FOTO_BYTES {
            gagal(&mut errors, "foto", "Ukuran foto maksimal 10 MB.");
        }
    }
    if !errors.is_empty() {
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let (tahun, bulan) = periode(&query)?;
    let (awal, akhir) = batas_bulan(tahun, bulan)?;

    let sql = format!(
        "SELECT {COLS} FROM {TABLE} WHERE user_id = ? AND tanggal >= ? AND tanggal < ? \
         ORDER BY tanggal DESC, id DESC"
    );
    let rows = sqlx::query(&sql)
        .bind(user.user_id)
        .bind(awal)
        .bind(akhir)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let mut entries = rows
        .iter()
        .map(from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;
    isi_foto(&state.pool, &state.app_url, &mut entries).await?;

    Ok(Json(json!({
        "data": entries.iter().map(to_json).collect::<Vec<_>>(),
        "ringkasan": ringkasan(tahun, bulan, &entries),
    })))
}

pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = validate(&body)?;

    let res = sqlx::query(
        "INSERT INTO skp_jurnal_kegiatan \
         (user_id, tanggal, rhk, kegiatan, output, satuan, keterangan, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(user.user_id)
    .bind(input.tanggal)
    .bind(input.rhk)
    .bind(input.kegiatan.as_str())
    .bind(input.output)
    .bind(input.satuan.as_deref())
    .bind(input.keterangan.as_deref())
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    let id = res.last_insert_id();
    let created = muat(&state, user.user_id, id).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "data": to_json(&created) })),
    )
        .into_response())
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let user = require_auth(&state, &headers).await?;
    find(&state.pool, user.user_id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let input = validate(&body)?;

    sqlx::query(
        "UPDATE skp_jurnal_kegiatan SET tanggal = ?, rhk = ?, kegiatan = ?, output = ?, satuan = ?, \
         keterangan = ?, updated_at = NOW() WHERE id = ? AND user_id = ?",
    )
    .bind(input.tanggal)
    .bind(input.rhk)
    .bind(input.kegiatan.as_str())
    .bind(input.output)
    .bind(input.satuan.as_deref())
    .bind(input.keterangan.as_deref())
    .bind(id)
    .bind(user.user_id)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    let updated = muat(&state, user.user_id, id).await?;
    Ok(Json(json!({ "data": to_json(&updated) })))
}

pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    let user = require_auth(&state, &headers).await?;
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query("DELETE FROM skp_jurnal_kegiatan WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user.user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    if res.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    let dirs = media::delete_collection(&mut tx, MODEL, id, COLLECTION, None).await?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&dirs).await;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /api/skp/jurnal/{id}/foto`. Respons: kegiatan lengkap dengan foto terbaru.
pub async fn upload_foto(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    req: Request,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let user = require_auth(&state, &headers).await?;
    let entry = muat(&state, user.user_id, id).await?;
    let files = baca_foto(&state, req).await?;
    validate_foto(&files, entry.fotos.len())?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let mut dirs = Vec::new();
    for file in &files {
        let mime = media::image_mime(&file.bytes).unwrap_or("image/jpeg");
        match media::attach(&mut tx, MODEL, id, COLLECTION, file, mime, true).await {
            Ok(stored) => dirs.push(stored.dir),
            Err(e) => {
                media::remove_dirs(&dirs).await;
                return Err(e);
            }
        }
    }
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&dirs).await;
        return Err(internal(e));
    }

    let updated = muat(&state, user.user_id, id).await?;
    Ok(Json(json!({ "data": to_json(&updated) })))
}

/// `DELETE /api/skp/jurnal/{id}/foto/{media_id}`. Respons: kegiatan lengkap dengan foto terbaru.
pub async fn delete_foto(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, media_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let id = parse_id(&id)?;
    let media_id = parse_id(&media_id)?;
    let user = require_auth(&state, &headers).await?;
    muat(&state, user.user_id, id).await?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let found: Option<u64> = sqlx::query_scalar(
        "SELECT CAST(id AS UNSIGNED) FROM media WHERE id = ? AND model_type = ? AND model_id = ? AND collection_name = ?",
    )
    .bind(media_id)
    .bind(MODEL)
    .bind(id)
    .bind(COLLECTION)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    if found.is_none() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Foto tidak ditemukan.",
        ));
    }
    sqlx::query("DELETE FROM media WHERE id = ?")
        .bind(media_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&[media::media_dir(media_id)]).await;

    let updated = muat(&state, user.user_id, id).await?;
    Ok(Json(json!({ "data": to_json(&updated) })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_body() -> Value {
        json!({
            "tanggal": "2026-09-03",
            "rhk": 2,
            "kegiatan": "  Input paket SPSE  ",
            "output": 2,
            "satuan": "Laporan",
            "keterangan": ""
        })
    }

    fn errors_of(err: ApiError) -> BTreeMap<String, Vec<String>> {
        err.errors.expect("harus ada errors")
    }

    #[test]
    fn validate_menerima_input_lengkap_dan_merapikan_teks() {
        let input = validate(&valid_body()).unwrap();
        assert_eq!(input.tanggal, NaiveDate::from_ymd_opt(2026, 9, 3).unwrap());
        assert_eq!(input.rhk, Some(2));
        assert_eq!(input.kegiatan, "Input paket SPSE");
        assert_eq!(input.output, Some(2.0));
        assert_eq!(input.satuan.as_deref(), Some("Laporan"));
        assert_eq!(input.keterangan, None, "string kosong menjadi NULL");
    }

    #[test]
    fn validate_kolom_opsional_boleh_null() {
        let body = json!({ "tanggal": "2026-09-03", "rhk": null, "kegiatan": "Rapat", "output": null,
                           "satuan": null, "keterangan": null });
        let input = validate(&body).unwrap();
        assert_eq!(input.rhk, None);
        assert_eq!(input.output, None);
    }

    #[test]
    fn validate_output_boleh_string_dengan_koma() {
        let mut body = valid_body();
        body["output"] = json!("2,5");
        assert_eq!(validate(&body).unwrap().output, Some(2.5));
    }

    #[test]
    fn validate_mengumpulkan_semua_error_sekaligus() {
        let body = json!({ "tanggal": "03/09/2026", "rhk": 0, "kegiatan": "", "output": -1 });
        let errs = errors_of(validate(&body).unwrap_err());
        for field in ["tanggal", "rhk", "kegiatan", "output"] {
            assert!(
                errs.contains_key(field),
                "error untuk {field} hilang: {errs:?}"
            );
        }
    }

    #[test]
    fn validate_batas_panjang() {
        let mut body = valid_body();
        body["kegiatan"] = json!("a".repeat(MAX_KEGIATAN + 1));
        body["satuan"] = json!("b".repeat(MAX_SATUAN + 1));
        body["keterangan"] = json!("c".repeat(MAX_KETERANGAN + 1));
        let errs = errors_of(validate(&body).unwrap_err());
        assert!(
            errs.contains_key("kegiatan")
                && errs.contains_key("satuan")
                && errs.contains_key("keterangan")
        );
    }

    #[test]
    fn validate_rhk_harus_bilangan_bulat_1_sampai_99() {
        for bad in [json!(100), json!(2.5), json!("abc")] {
            let mut body = valid_body();
            body["rhk"] = bad.clone();
            assert!(
                validate(&body)
                    .unwrap_err()
                    .errors
                    .unwrap()
                    .contains_key("rhk"),
                "{bad} harus ditolak"
            );
        }
    }

    #[test]
    fn periode_wajib_tahun_dan_bulan_valid() {
        let mut q = HashMap::new();
        q.insert("tahun".to_string(), "2026".to_string());
        assert!(periode(&q).is_err());
        q.insert("bulan".to_string(), "13".to_string());
        assert!(periode(&q).is_err());
        q.insert("bulan".to_string(), "9".to_string());
        assert_eq!(periode(&q).unwrap(), (2026, 9));
    }

    #[test]
    fn batas_bulan_desember_berpindah_tahun() {
        let (awal, akhir) = batas_bulan(2026, 12).unwrap();
        assert_eq!(awal, NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
        assert_eq!(akhir, NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
    }

    fn entry(tgl: (i32, u32, u32), rhk: Option<i64>, output: Option<f64>) -> Entry {
        Entry {
            id: 1,
            tanggal: NaiveDate::from_ymd_opt(tgl.0, tgl.1, tgl.2).unwrap(),
            rhk,
            kegiatan: "x".into(),
            output,
            satuan: None,
            keterangan: None,
            created_at: None,
            updated_at: None,
            fotos: Vec::new(),
        }
    }

    #[test]
    fn ringkasan_mengelompokkan_per_rhk_dan_menghitung_hari() {
        let rows = vec![
            entry((2026, 9, 3), Some(2), Some(2.0)),
            entry((2026, 9, 3), Some(2), Some(1.5)),
            entry((2026, 9, 5), Some(1), Some(1.0)),
            entry((2026, 9, 5), None, None),
        ];
        let r = ringkasan(2026, 9, &rows);
        assert_eq!(r["jumlah"], 4);
        assert_eq!(r["hari"], 2);
        assert_eq!(
            r["per_rhk"],
            json!([
                { "rhk": 1, "jumlah": 1, "output": 1.0 },
                { "rhk": 2, "jumlah": 2, "output": 3.5 }
            ])
        );
    }
}
