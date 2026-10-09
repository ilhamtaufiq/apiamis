//! `/api/usulan-kegiatan`: port `UsulanKegiatanController` (index, store, show, update, destroy, exportExcel),
//! `UsulanKegiatanResource`, dan `UsulanKegiatanExport` (11 kolom, baris pertama tebal).
//!
//! Akses: admin melihat dan mengubah semua usulan. Role lain hanya usulan miliknya (index, show, update,
//! destroy). Export Excel tidak memakai scope, sama dengan Laravel: user yang login mendapat semua baris.
//! Gate `route_permission::check` tidak diubah. `/usulan-kegiatan` tidak ada di `MUTATION_RESOURCE_PREFIXES`,
//! jadi POST/PUT/DELETE dari non-admin ditolak kecuali ada rule di DB, seperti di Laravel.
//!
//! Deviasi (dicatat, bukan diputuskan ulang):
//! - Notifikasi `store` dikirim ke semua admin termasuk pelaku (`Notification::send` tanpa pengecualian).
//!   Maka memakai `notify::admin_ids` + `notify::to_users`, bukan `notify::admins` yang melewati pelaku.
//! - `store` dan `update` memakai satu transaksi. Laravel tidak memakai transaksi: bila berkas gagal disimpan,
//!   Laravel meninggalkan baris tanpa dokumen. Di sini seluruh perubahan dibatalkan.
//! - `update` membaca multipart dan JSON. PHP tidak mengisi `$_POST`/`$_FILES` untuk PUT, jadi di Laravel
//!   field dan berkas multipart pada PUT tidak terbaca. Di sini dibaca supaya penggantian dokumen bisa jalan.
//! - `update` tidak mengubah `ringkasan`, sama dengan `only([...])` di Laravel. Validasinya tetap jalan.
//! - `dokumen`: `mimes` memeriksa ekstensi saja. Laravel juga memeriksa tipe MIME dari isi berkas.
//! - `kecamatan_id` dan `desa_id` pada store/update harus bilangan bulat murni. Laravel menerima "12abc"
//!   lewat coercion MySQL. Filter index tetap mengikat nilai mentah seperti `where()` Laravel.
//! - `per_page`: default 20 bila tidak dikirim. Kosong, nol, atau non-angka memakai 15 (`paginate()`).
//!   Nilai negatif belum diverifikasi.
//! - Waktu `created_at` dan `updated_at` memakai bentuk Carbon UTC (`carbon_json`). Zona aplikasi UTC.
//! - Audit `updated` hanya memuat kolom yang berubah, seperti `berkas.rs`. Laravel 10+ memanggil
//!   `syncChanges()` sebelum event `updated`; perilaku pasti belum diverifikasi tanpa `vendor/`.

use std::{collections::BTreeMap, collections::HashMap, path::PathBuf};

use axum::{
    extract::{FromRequest, Multipart, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Form, Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use rust_xlsxwriter::{Format, Workbook, Worksheet};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row};

use crate::{
    changes, desa, kecamatan,
    kontrak_xlsx::xlsx_response,
    lookup::carbon_json,
    media::{self, internal, Upload},
    notify,
    pagination::{paginate_laravel, PageParams},
    require_auth, users, AppState,
};

const MODEL: &str = "App\\Models\\UsulanKegiatan";
const COLLECTION: &str = "dokumen";
const VALIDATION_MESSAGE: &str = "Validation error";
const SUB_BIDANG: [&str; 2] = ["air minum", "sanitasi"];
const DOKUMEN_EXTENSIONS: [&str; 8] = ["pdf", "doc", "docx", "xls", "xlsx", "png", "jpg", "jpeg"];
/// `max:10240` (kilobyte) pada `dokumen`.
const DOKUMEN_KB: usize = 10_240;
const MAX_DOKUMEN_BYTES: usize = DOKUMEN_KB * 1024;
/// Batas body untuk rute tulis: berkas maksimum plus ruang untuk field multipart.
pub const BODY_LIMIT: usize = MAX_DOKUMEN_BYTES + 1024 * 1024;
/// Kolom yang bisa diubah lewat update, urut tetap untuk audit. `ringkasan` tidak ada (seperti Laravel).
const UPDATABLE: [&str; 8] = [
    "sub_bidang",
    "nama_pengusul",
    "kecamatan_id",
    "desa_id",
    "perihal",
    "tanggal_surat_masuk",
    "nomor_surat_masuk",
    "tanggal_surat",
];
const EXPORT_HEADINGS: [&str; 11] = [
    "No",
    "Sub Bidang",
    "Nama Pengusul",
    "Kecamatan",
    "Desa",
    "Perihal",
    "Tanggal Surat Masuk",
    "Nomor Surat Masuk",
    "Tanggal Surat",
    "Tanggal Pengajuan",
    "Dokumen",
];

/// `SELECT` dengan join kecamatan dan desa (untuk kolom Excel). Alias tabel `u`.
const SELECT_USULAN: &str = "SELECT CAST(u.id AS SIGNED) AS id, CAST(u.user_id AS SIGNED) AS user_id, \
     CAST(u.sub_bidang AS CHAR) AS sub_bidang, u.nama_pengusul, \
     CAST(u.kecamatan_id AS SIGNED) AS kecamatan_id, CAST(u.desa_id AS SIGNED) AS desa_id, u.perihal, \
     u.ringkasan, u.tanggal_surat_masuk, u.nomor_surat_masuk, u.tanggal_surat, u.created_at, u.updated_at, \
     k.n_kec AS kecamatan_nama, d.n_desa AS desa_nama \
     FROM tbl_usulan_kegiatan u \
     LEFT JOIN tbl_kecamatan k ON k.id = u.kecamatan_id \
     LEFT JOIN tbl_desa d ON d.id = u.desa_id";

/// Baris `tbl_usulan_kegiatan` beserta nama kecamatan dan desa.
#[derive(Debug, Clone, PartialEq)]
pub struct UsulanRow {
    pub id: i64,
    pub user_id: i64,
    pub sub_bidang: String,
    pub nama_pengusul: String,
    pub kecamatan_id: i64,
    pub desa_id: i64,
    pub perihal: String,
    pub ringkasan: String,
    pub tanggal_surat_masuk: NaiveDate,
    pub nomor_surat_masuk: String,
    pub tanggal_surat: NaiveDate,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub kecamatan_nama: Option<String>,
    pub desa_nama: Option<String>,
}

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<UsulanRow, sqlx::Error> {
    Ok(UsulanRow {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        sub_bidang: r.try_get("sub_bidang")?,
        nama_pengusul: r.try_get("nama_pengusul")?,
        kecamatan_id: r.try_get("kecamatan_id")?,
        desa_id: r.try_get("desa_id")?,
        perihal: r.try_get("perihal")?,
        ringkasan: r.try_get::<Option<String>, _>("ringkasan")?.unwrap_or_default(),
        tanggal_surat_masuk: r.try_get("tanggal_surat_masuk")?,
        nomor_surat_masuk: r.try_get("nomor_surat_masuk")?,
        tanggal_surat: r.try_get("tanggal_surat")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
        kecamatan_nama: r.try_get("kecamatan_nama")?,
        desa_nama: r.try_get("desa_nama")?,
    })
}

async fn find_row<'e, E>(exec: E, id: i64) -> Result<Option<UsulanRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_USULAN} WHERE u.id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_row).transpose()
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn ymd_dmy(d: NaiveDate) -> String {
    d.format("%d/%m/%Y").to_string()
}

/// Nilai kolom untuk perbandingan dan audit (`getAttributes()`).
fn col_json(row: &UsulanRow, col: &str) -> Value {
    match col {
        "sub_bidang" => json!(row.sub_bidang),
        "nama_pengusul" => json!(row.nama_pengusul),
        "kecamatan_id" => json!(row.kecamatan_id),
        "desa_id" => json!(row.desa_id),
        "perihal" => json!(row.perihal),
        "tanggal_surat_masuk" => json!(ymd(row.tanggal_surat_masuk)),
        "nomor_surat_masuk" => json!(row.nomor_surat_masuk),
        "tanggal_surat" => json!(ymd(row.tanggal_surat)),
        _ => Value::Null,
    }
}

/// `getAttributes()` untuk audit `created` dan `deleted`.
fn attributes(row: &UsulanRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    m.insert("user_id".into(), json!(row.user_id));
    for col in UPDATABLE {
        m.insert(col.into(), col_json(row, col));
    }
    m.insert("ringkasan".into(), json!(row.ringkasan));
    m.insert("created_at".into(), carbon_json(row.created_at));
    m.insert("updated_at".into(), carbon_json(row.updated_at));
    m
}

/// `UsulanKegiatanResource` dengan relasi `user`, `kecamatan`, dan `desa` (selalu dimuat di Laravel).
async fn resource(pool: &MySqlPool, app_url: &str, row: &UsulanRow) -> Result<Value, ApiError> {
    // `UserResource` tanpa `roles` dan `permissions`: relasi itu tidak dimuat di Laravel, jadi key-nya hilang.
    let user = match users::resource(pool, app_url, row.user_id as u64)
        .await
        .map_err(internal)?
    {
        Some(mut v) => {
            if let Some(obj) = v.as_object_mut() {
                obj.remove("roles");
                obj.remove("permissions");
            }
            v
        }
        None => Value::Null,
    };
    let kecamatan_json = match kecamatan::find(pool, row.kecamatan_id as u64)
        .await
        .map_err(internal)?
    {
        Some(k) => kecamatan::to_resource(&k),
        None => Value::Null,
    };
    // `DesaResource` tanpa `kecamatan`: relasi itu tidak dimuat, jadi key-nya tidak muncul.
    let desa_json = match desa::find(pool, row.desa_id as u64).await.map_err(internal)? {
        Some(d) => desa::base_resource(&d),
        None => Value::Null,
    };
    let (dokumen_url, _) =
        media::first_urls(pool, app_url, MODEL, row.id as u64, COLLECTION)
            .await
            .map_err(internal)?;
    Ok(json!({
        "id": row.id,
        "user_id": row.user_id,
        "user": user,
        "sub_bidang": row.sub_bidang,
        "nama_pengusul": row.nama_pengusul,
        "kecamatan_id": row.kecamatan_id,
        "kecamatan": kecamatan_json,
        "desa_id": row.desa_id,
        "desa": desa_json,
        "perihal": row.perihal,
        "ringkasan": row.ringkasan,
        "tanggal_surat_masuk": ymd(row.tanggal_surat_masuk),
        "nomor_surat_masuk": row.nomor_surat_masuk,
        "tanggal_surat": ymd(row.tanggal_surat),
        "dokumen_url": dokumen_url,
        "created_at": carbon_json(row.created_at),
        "updated_at": carbon_json(row.updated_at),
    }))
}

// ---------------------------------------------------------------------------
// Input dan validasi
// ---------------------------------------------------------------------------

/// Nilai satu field. `Null` = kosong atau null (ConvertEmptyStringsToNull). `Other` = non-string dari JSON.
#[derive(Debug, Clone, PartialEq)]
enum Field {
    Null,
    Text(String),
    Other,
}

#[derive(Default)]
struct Input {
    fields: BTreeMap<String, Field>,
    /// Berkas `dokumen` (field multipart dengan nama berkas dan isi tidak kosong).
    dokumen: Option<Upload>,
}

/// Tampilan nilai untuk aturan validasi.
#[derive(Clone, Copy)]
enum Val<'a> {
    Absent,
    Null,
    Text(&'a str),
    Other,
}

fn val<'a>(input: &'a Input, key: &str) -> Val<'a> {
    match input.fields.get(key) {
        None => Val::Absent,
        Some(Field::Null) => Val::Null,
        Some(Field::Text(s)) => Val::Text(s.as_str()),
        Some(Field::Other) => Val::Other,
    }
}

/// `TrimStrings` lalu `ConvertEmptyStringsToNull`.
fn text_field(raw: &str) -> Field {
    let t = raw.trim();
    if t.is_empty() {
        Field::Null
    } else {
        Field::Text(t.to_string())
    }
}

fn bad_request(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, format!("Permintaan tidak valid: {e}"))
}

/// Membaca body sebagai multipart, form, atau JSON, sesuai `Content-Type`. Tipe lain: input kosong.
async fn read_input(state: &AppState, request: Request) -> Result<Input, ApiError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut input = Input::default();
    if content_type.starts_with("multipart/form-data") {
        let mut multipart = Multipart::from_request(request, state)
            .await
            .map_err(bad_request)?;
        while let Some(field) = multipart.next_field().await.map_err(bad_request)? {
            let name = field.name().unwrap_or_default().to_string();
            let file_name = field.file_name().map(str::to_string);
            match file_name {
                Some(original_name) if name == "dokumen" => {
                    let bytes = field.bytes().await.map_err(bad_request)?;
                    if !bytes.is_empty() {
                        input.dokumen = Some(Upload {
                            original_name,
                            bytes: bytes.to_vec(),
                        });
                    }
                }
                Some(_) => {
                    // Berkas pada field lain: diabaikan (Laravel tidak memakainya).
                    field.bytes().await.map_err(bad_request)?;
                }
                None => {
                    let text = field.text().await.map_err(bad_request)?;
                    input.fields.insert(name, text_field(&text));
                }
            }
        }
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        let Form(map) = Form::<HashMap<String, String>>::from_request(request, state)
            .await
            .map_err(bad_request)?;
        for (k, v) in map {
            input.fields.insert(k, text_field(&v));
        }
    } else if content_type.starts_with("application/json") {
        let bytes = axum::body::to_bytes(request.into_body(), BODY_LIMIT)
            .await
            .map_err(bad_request)?;
        // JSON yang tidak valid diperlakukan seperti input kosong.
        if let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(&bytes) {
            for (k, v) in map {
                let field = match v {
                    Value::Null => Field::Null,
                    Value::String(s) => text_field(&s),
                    _ => Field::Other,
                };
                input.fields.insert(k, field);
            }
        }
    }
    Ok(input)
}

/// Kumpulan error validasi per field, urut seperti `Validator::errors()`.
#[derive(Default)]
struct Errs(BTreeMap<String, Vec<String>>);

impl Errs {
    fn add(&mut self, key: &str, message: String) {
        self.0.entry(key.to_string()).or_default().push(message);
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn into_error(self) -> ApiError {
        ApiError::validation(VALIDATION_MESSAGE, self.0)
    }
}

/// Nama atribut di pesan Laravel: underscore menjadi spasi.
fn attribute(key: &str) -> String {
    key.replace('_', " ")
}

/// `required|string|max:N`. Dengan `sometimes`, field yang tidak dikirim dilewati.
fn required_string(errs: &mut Errs, key: &str, v: Val<'_>, max: usize, sometimes: bool) -> Option<String> {
    let attr = attribute(key);
    match v {
        Val::Absent if sometimes => None,
        Val::Absent | Val::Null => {
            errs.add(key, format!("The {attr} field is required."));
            None
        }
        Val::Other => {
            errs.add(key, format!("The {attr} field must be a string."));
            None
        }
        Val::Text(s) if s.chars().count() > max => {
            errs.add(
                key,
                format!("The {attr} field must not be greater than {max} characters."),
            );
            None
        }
        Val::Text(s) => Some(s.to_string()),
    }
}

/// `nullable|string`.
fn nullable_string(errs: &mut Errs, key: &str, v: Val<'_>) -> Option<String> {
    match v {
        Val::Text(s) => Some(s.to_string()),
        Val::Other => {
            errs.add(key, format!("The {} field must be a string.", attribute(key)));
            None
        }
        Val::Absent | Val::Null => None,
    }
}

/// `required|in:...`. Pesan `in` belum diverifikasi terhadap Laravel (lihat laporan).
fn required_in(errs: &mut Errs, key: &str, v: Val<'_>, allowed: &[&str], sometimes: bool) -> Option<String> {
    let attr = attribute(key);
    match v {
        Val::Absent if sometimes => None,
        Val::Absent | Val::Null => {
            errs.add(key, format!("The {attr} field is required."));
            None
        }
        Val::Text(s) if allowed.contains(&s) => Some(s.to_string()),
        _ => {
            errs.add(key, format!("The selected {attr} is invalid."));
            None
        }
    }
}

/// `required|exists:table,id`. Pesan `exists` belum diverifikasi terhadap Laravel (lihat laporan).
async fn required_exists(
    pool: &MySqlPool,
    errs: &mut Errs,
    key: &str,
    table: &str,
    v: Val<'_>,
    sometimes: bool,
) -> Result<Option<i64>, ApiError> {
    let attr = attribute(key);
    let text = match v {
        Val::Absent if sometimes => return Ok(None),
        Val::Absent | Val::Null => {
            errs.add(key, format!("The {attr} field is required."));
            return Ok(None);
        }
        Val::Other => {
            errs.add(key, format!("The selected {attr} is invalid."));
            return Ok(None);
        }
        Val::Text(s) => s,
    };
    let Ok(id) = text.parse::<i64>() else {
        errs.add(key, format!("The selected {attr} is invalid."));
        return Ok(None);
    };
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE id = ?");
    let n: i64 = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    if n == 0 {
        errs.add(key, format!("The selected {attr} is invalid."));
        return Ok(None);
    }
    Ok(Some(id))
}

/// `YYYY-MM-DD`, atau `YYYY-MM-DD` diikuti `T` atau spasi dan waktu. Format lain belum diterima (lihat laporan).
fn parse_date(s: &str) -> Option<NaiveDate> {
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d);
    }
    let head = s.get(..10)?;
    let rest = s.get(10..)?;
    if rest.starts_with('T') || rest.starts_with(' ') {
        NaiveDate::parse_from_str(head, "%Y-%m-%d").ok()
    } else {
        None
    }
}

/// `required|date`.
fn required_date(errs: &mut Errs, key: &str, v: Val<'_>, sometimes: bool) -> Option<NaiveDate> {
    let attr = attribute(key);
    match v {
        Val::Absent if sometimes => None,
        Val::Absent | Val::Null => {
            errs.add(key, format!("The {attr} field is required."));
            None
        }
        Val::Text(s) => match parse_date(s) {
            Some(d) => Some(d),
            None => {
                errs.add(key, format!("The {attr} field must be a valid date."));
                None
            }
        },
        Val::Other => {
            errs.add(key, format!("The {attr} field must be a valid date."));
            None
        }
    }
}

/// `nullable|file|mimes:...|max:10240`. Tanpa berkas, tidak ada error kecuali field dikirim sebagai teks.
fn check_dokumen(errs: &mut Errs, input: &Input) {
    let mimes = DOKUMEN_EXTENSIONS.join(", ");
    if let Some(upload) = &input.dokumen {
        let ext = upload.extension().to_ascii_lowercase();
        if !DOKUMEN_EXTENSIONS.contains(&ext.as_str()) {
            errs.add(
                "dokumen",
                format!("The dokumen field must be a file of type: {mimes}."),
            );
        }
        if upload.bytes.len() > MAX_DOKUMEN_BYTES {
            errs.add(
                "dokumen",
                format!("The dokumen field must not be greater than {DOKUMEN_KB} kilobytes."),
            );
        }
    } else if matches!(input.fields.get("dokumen"), Some(f) if *f != Field::Null) {
        errs.add("dokumen", "The dokumen field must be a file.".to_string());
        errs.add(
            "dokumen",
            format!("The dokumen field must be a file of type: {mimes}."),
        );
    }
}

struct StoreForm {
    sub_bidang: String,
    nama_pengusul: String,
    kecamatan_id: i64,
    desa_id: i64,
    perihal: String,
    ringkasan: String,
    tanggal_surat_masuk: NaiveDate,
    nomor_surat_masuk: String,
    tanggal_surat: NaiveDate,
}

/// Aturan `store` (semua `required`). Error dikumpulkan lalu dikembalikan sekaligus, seperti `Validator`.
async fn validate_store(pool: &MySqlPool, input: &Input) -> Result<StoreForm, ApiError> {
    let mut errs = Errs::default();
    let sub_bidang = required_in(&mut errs, "sub_bidang", val(input, "sub_bidang"), &SUB_BIDANG, false);
    let nama_pengusul = required_string(&mut errs, "nama_pengusul", val(input, "nama_pengusul"), 255, false);
    let kecamatan_id =
        required_exists(pool, &mut errs, "kecamatan_id", "tbl_kecamatan", val(input, "kecamatan_id"), false)
            .await?;
    let desa_id = required_exists(pool, &mut errs, "desa_id", "tbl_desa", val(input, "desa_id"), false).await?;
    let perihal = required_string(&mut errs, "perihal", val(input, "perihal"), 255, false);
    check_dokumen(&mut errs, input);
    let ringkasan = nullable_string(&mut errs, "ringkasan", val(input, "ringkasan"));
    let tanggal_surat_masuk =
        required_date(&mut errs, "tanggal_surat_masuk", val(input, "tanggal_surat_masuk"), false);
    let nomor_surat_masuk =
        required_string(&mut errs, "nomor_surat_masuk", val(input, "nomor_surat_masuk"), 255, false);
    let tanggal_surat = required_date(&mut errs, "tanggal_surat", val(input, "tanggal_surat"), false);
    if !errs.is_empty() {
        return Err(errs.into_error());
    }
    let (
        Some(sub_bidang),
        Some(nama_pengusul),
        Some(kecamatan_id),
        Some(desa_id),
        Some(perihal),
        Some(tanggal_surat_masuk),
        Some(nomor_surat_masuk),
        Some(tanggal_surat),
    ) = (
        sub_bidang,
        nama_pengusul,
        kecamatan_id,
        desa_id,
        perihal,
        tanggal_surat_masuk,
        nomor_surat_masuk,
        tanggal_surat,
    )
    else {
        return Err(errs.into_error());
    };
    Ok(StoreForm {
        sub_bidang,
        nama_pengusul,
        kecamatan_id,
        desa_id,
        perihal,
        ringkasan: ringkasan.unwrap_or_default(),
        tanggal_surat_masuk,
        nomor_surat_masuk,
        tanggal_surat,
    })
}

/// Field update yang dikirim. `None` = tidak diubah. Aturan `sometimes|required`.
#[derive(Default)]
struct UpdateForm {
    sub_bidang: Option<String>,
    nama_pengusul: Option<String>,
    kecamatan_id: Option<i64>,
    desa_id: Option<i64>,
    perihal: Option<String>,
    tanggal_surat_masuk: Option<NaiveDate>,
    nomor_surat_masuk: Option<String>,
    tanggal_surat: Option<NaiveDate>,
}

async fn validate_update(pool: &MySqlPool, input: &Input) -> Result<UpdateForm, ApiError> {
    let mut errs = Errs::default();
    let form = UpdateForm {
        sub_bidang: required_in(&mut errs, "sub_bidang", val(input, "sub_bidang"), &SUB_BIDANG, true),
        nama_pengusul: required_string(&mut errs, "nama_pengusul", val(input, "nama_pengusul"), 255, true),
        kecamatan_id: required_exists(pool, &mut errs, "kecamatan_id", "tbl_kecamatan", val(input, "kecamatan_id"), true)
            .await?,
        desa_id: required_exists(pool, &mut errs, "desa_id", "tbl_desa", val(input, "desa_id"), true).await?,
        perihal: required_string(&mut errs, "perihal", val(input, "perihal"), 255, true),
        tanggal_surat_masuk: required_date(&mut errs, "tanggal_surat_masuk", val(input, "tanggal_surat_masuk"), true),
        nomor_surat_masuk: required_string(&mut errs, "nomor_surat_masuk", val(input, "nomor_surat_masuk"), 255, true),
        tanggal_surat: required_date(&mut errs, "tanggal_surat", val(input, "tanggal_surat"), true),
    };
    // `ringkasan` tetap divalidasi, walaupun tidak disimpan oleh update (sama dengan Laravel).
    let _ = nullable_string(&mut errs, "ringkasan", val(input, "ringkasan"));
    check_dokumen(&mut errs, input);
    if !errs.is_empty() {
        return Err(errs.into_error());
    }
    Ok(form)
}

// ---------------------------------------------------------------------------
// Hak akses dan pembantu
// ---------------------------------------------------------------------------

async fn is_admin(pool: &MySqlPool, user_id: u64) -> Result<bool, ApiError> {
    let roles = auth::login::roles_of(pool, user_id).await.map_err(internal)?;
    Ok(roles.iter().any(|(_, name)| name == "admin"))
}

fn forbidden() -> ApiError {
    ApiError::new(StatusCode::FORBIDDEN, "Forbidden")
}

/// `!hasRole('admin') && user_id !== me` menghasilkan 403 `Forbidden`.
async fn ensure_access(pool: &MySqlPool, user_id: u64, row: &UsulanRow) -> Result<(), ApiError> {
    if row.user_id == user_id as i64 || is_admin(pool, user_id).await? {
        return Ok(());
    }
    Err(forbidden())
}

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse().map_err(|_| ApiError::not_found())
}

fn base_url(state: &AppState) -> String {
    state.app_url.trim_end_matches('/').to_string()
}

/// Parameter `search`, `sub_bidang`, dan lainnya: `filled()` setelah trim.
fn filled<'a>(query: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    query.get(key).map(|v| v.trim()).filter(|v| !v.is_empty())
}

/// `$request->get('per_page', 20)` lalu `paginate()`: nilai falsy (kosong, nol) memakai 15.
fn per_page_param(query: &HashMap<String, String>) -> u64 {
    match query.get("per_page") {
        None => 20,
        Some(raw) => match raw.trim().parse::<i64>() {
            Ok(n) if n > 0 => n as u64,
            _ => 15,
        },
    }
}

fn page_param(query: &HashMap<String, String>) -> u64 {
    query
        .get("page")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|p| *p >= 1)
        .unwrap_or(1)
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/usulan-kegiatan`: paginator Laravel dengan `data`, `links`, dan `meta`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let admin = is_admin(&state.pool, user.user_id).await?;

    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if !admin {
        clauses.push("u.user_id = ?".into());
        binds.push(user.user_id.to_string());
    }
    if let Some(search) = filled(&query, "search") {
        clauses.push("(u.nama_pengusul LIKE ? OR u.perihal LIKE ? OR u.ringkasan LIKE ?)".into());
        let pattern = format!("%{search}%");
        binds.extend([pattern.clone(), pattern.clone(), pattern]);
    }
    for (param, column) in [
        ("sub_bidang", "u.sub_bidang"),
        ("kecamatan_id", "u.kecamatan_id"),
        ("desa_id", "u.desa_id"),
    ] {
        if let Some(value) = filled(&query, param) {
            clauses.push(format!("{column} = ?"));
            binds.push(value.to_string());
        }
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    let params = PageParams {
        page: page_param(&query),
        per_page: per_page_param(&query),
    };

    let count_sql = format!("SELECT COUNT(*) FROM tbl_usulan_kegiatan u{where_sql}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        count_q = count_q.bind(b.clone());
    }
    let total = count_q.fetch_one(&state.pool).await.map_err(internal)?;

    let sql = format!("{SELECT_USULAN}{where_sql} ORDER BY u.created_at DESC, u.id DESC LIMIT ? OFFSET ?");
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b.clone());
    }
    let offset = (params.page - 1).saturating_mul(params.per_page);
    let raw_rows = q
        .bind(params.per_page as i64)
        .bind(offset as i64)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let rows = raw_rows
        .iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;

    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        data.push(resource(&state.pool, &state.app_url, row).await?);
    }
    let base = format!("{}/api/usulan-kegiatan", base_url(&state));
    let url_for = |page: u64| format!("{base}?page={page}");
    let body = paginate_laravel(data, total as u64, params, &base, &url_for);
    Ok(Json(body).into_response())
}

/// `POST /api/usulan-kegiatan`: 200 dengan `{data}` (bukan 201, seperti Laravel).
pub async fn store(State(state): State<AppState>, request: Request) -> Result<Response, ApiError> {
    let headers = request.headers().clone();
    let user = require_auth(&state, &headers).await?;
    let input = read_input(&state, request).await?;
    let form = validate_store(&state.pool, &input).await?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO tbl_usulan_kegiatan (user_id, sub_bidang, nama_pengusul, kecamatan_id, desa_id, perihal, \
         ringkasan, tanggal_surat_masuk, nomor_surat_masuk, tanggal_surat, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(user.user_id as i64)
    .bind(&form.sub_bidang)
    .bind(&form.nama_pengusul)
    .bind(form.kecamatan_id)
    .bind(form.desa_id)
    .bind(&form.perihal)
    .bind(&form.ringkasan)
    .bind(form.tanggal_surat_masuk)
    .bind(&form.nomor_surat_masuk)
    .bind(form.tanggal_surat)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;
    let row = find_row(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("usulan baru tidak terbaca"))?;

    let url = format!("{}/api/usulan-kegiatan", base_url(&state));
    changes::audit_only(
        &mut tx,
        &headers,
        user.user_id,
        MODEL,
        "created",
        id,
        None,
        Some(attributes(&row)),
        &url,
    )
    .await?;

    let mut created_dir: Option<PathBuf> = None;
    if let Some(upload) = &input.dokumen {
        let mime = media::mime_for_name(&upload.original_name);
        let stored = media::attach(&mut tx, MODEL, id as u64, COLLECTION, upload, mime, false).await?;
        created_dir = Some(stored.dir);
    }

    // `Notification::send($admins, ...)`: semua admin, termasuk pelaku.
    let admins = notify::admin_ids(&mut tx).await.map_err(internal)?;
    let message = format!(
        "Usulan baru \"{}\" telah diajukan oleh {}",
        row.perihal, row.nama_pengusul
    );
    let link = format!("/usulan-kegiatan?id={id}");
    notify::to_users(
        &mut tx,
        &admins,
        "Usulan Kegiatan Baru",
        &message,
        Some(link.as_str()),
        "info",
    )
    .await
    .map_err(internal)?;

    if let Err(e) = tx.commit().await {
        if let Some(dir) = created_dir {
            media::remove_dirs(&[dir]).await;
        }
        return Err(internal(e));
    }
    let data = resource(&state.pool, &state.app_url, &row).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `GET /api/usulan-kegiatan/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_access(&state.pool, user.user_id, &row).await?;
    let data = resource(&state.pool, &state.app_url, &row).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `PUT/PATCH /api/usulan-kegiatan/{id}`. Lihat catatan deviasi di doc modul (multipart pada PUT).
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let headers = request.headers().clone();
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let current = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_access(&state.pool, user.user_id, &current).await?;

    let input = read_input(&state, request).await?;
    let form = validate_update(&state.pool, &input).await?;

    let mut next = current.clone();
    if let Some(v) = form.sub_bidang {
        next.sub_bidang = v;
    }
    if let Some(v) = form.nama_pengusul {
        next.nama_pengusul = v;
    }
    if let Some(v) = form.kecamatan_id {
        next.kecamatan_id = v;
    }
    if let Some(v) = form.desa_id {
        next.desa_id = v;
    }
    if let Some(v) = form.perihal {
        next.perihal = v;
    }
    if let Some(v) = form.tanggal_surat_masuk {
        next.tanggal_surat_masuk = v;
    }
    if let Some(v) = form.nomor_surat_masuk {
        next.nomor_surat_masuk = v;
    }
    if let Some(v) = form.tanggal_surat {
        next.tanggal_surat = v;
    }
    let changed: Vec<&str> = UPDATABLE
        .iter()
        .copied()
        .filter(|c| col_json(&current, c) != col_json(&next, c))
        .collect();

    let url = format!("{}/api/usulan-kegiatan/{id}", base_url(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    if !changed.is_empty() {
        sqlx::query(
            "UPDATE tbl_usulan_kegiatan SET sub_bidang = ?, nama_pengusul = ?, kecamatan_id = ?, desa_id = ?, \
             perihal = ?, tanggal_surat_masuk = ?, nomor_surat_masuk = ?, tanggal_surat = ?, updated_at = NOW() \
             WHERE id = ?",
        )
        .bind(&next.sub_bidang)
        .bind(&next.nama_pengusul)
        .bind(next.kecamatan_id)
        .bind(next.desa_id)
        .bind(&next.perihal)
        .bind(next.tanggal_surat_masuk)
        .bind(&next.nomor_surat_masuk)
        .bind(next.tanggal_surat)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        let after = find_row(&mut *tx, id)
            .await
            .map_err(internal)?
            .ok_or_else(|| internal("usulan hilang saat update"))?;
        let mut old = Map::new();
        let mut new = Map::new();
        for col in &changed {
            old.insert((*col).into(), col_json(&current, col));
            new.insert((*col).into(), col_json(&after, col));
        }
        old.insert("updated_at".into(), carbon_json(current.updated_at));
        new.insert("updated_at".into(), carbon_json(after.updated_at));
        changes::audit_only(
            &mut tx,
            &headers,
            user.user_id,
            MODEL,
            "updated",
            id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
    }

    // `clearMediaCollection('dokumen')` lalu `addMediaFromRequest`.
    let (obsolete, created_dir) = if let Some(upload) = &input.dokumen {
        let obsolete = media::delete_collection(&mut tx, MODEL, id as u64, COLLECTION, None).await?;
        let mime = media::mime_for_name(&upload.original_name);
        let dir = media::attach(&mut tx, MODEL, id as u64, COLLECTION, upload, mime, false)
            .await?
            .dir;
        (obsolete, Some(dir))
    } else {
        (Vec::new(), None)
    };

    if let Err(e) = tx.commit().await {
        if let Some(dir) = created_dir {
            media::remove_dirs(&[dir]).await;
        }
        return Err(internal(e));
    }
    media::remove_dirs(&obsolete).await;

    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let data = resource(&state.pool, &state.app_url, &row).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `DELETE /api/usulan-kegiatan/{id}`: hapus baris (tanpa soft delete) dan berkasnya.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let current = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_access(&state.pool, user.user_id, &current).await?;

    let url = format!("{}/api/usulan-kegiatan/{id}", base_url(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let dirs = media::delete_collection(&mut tx, MODEL, id as u64, COLLECTION, None).await?;
    sqlx::query("DELETE FROM tbl_usulan_kegiatan WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    changes::audit_only(
        &mut tx,
        &headers,
        user.user_id,
        MODEL,
        "deleted",
        id,
        Some(attributes(&current)),
        None,
        &url,
    )
    .await?;
    if let Err(e) = tx.commit().await {
        return Err(internal(e));
    }
    media::remove_dirs(&dirs).await;
    Ok(Json(json!({ "message": "Usulan kegiatan berhasil dihapus." })).into_response())
}

/// `GET /api/usulan-kegiatan/export-excel`: semua usulan (tanpa scope), urut `created_at` menurun.
pub async fn export_excel(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let sql = format!("{SELECT_USULAN} ORDER BY u.created_at DESC, u.id DESC");
    let raw_rows = sqlx::query(&sql)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let rows = raw_rows
        .iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;

    // URL dokumen diambil sebelum workbook dibuat, supaya tidak ada `await` di antara penulisan sel.
    let mut dokumen = Vec::with_capacity(rows.len());
    for row in &rows {
        let (url, _) = media::first_urls(&state.pool, &state.app_url, MODEL, row.id as u64, COLLECTION)
            .await
            .map_err(internal)?;
        dokumen.push(url);
    }
    let bytes = build_export(&rows, &dokumen)?;
    Ok(xlsx_response(bytes, "rekap_usulan_kegiatan.xlsx"))
}

fn excel_error(e: impl std::fmt::Display) -> ApiError {
    internal(format!("Gagal membuat berkas Excel: {e}"))
}

fn write_text(ws: &mut Worksheet, row: u32, col: u16, value: Option<&str>) -> Result<(), ApiError> {
    if let Some(v) = value {
        ws.write_string(row, col, v).map_err(excel_error)?;
    }
    Ok(())
}

/// Workbook `UsulanKegiatanExport`: baris pertama tebal, kolom diatur otomatis.
fn build_export(rows: &[UsulanRow], dokumen: &[String]) -> Result<Vec<u8>, ApiError> {
    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();
    let ws = wb.add_worksheet();
    for (col, heading) in EXPORT_HEADINGS.iter().enumerate() {
        ws.write_string_with_format(0, col as u16, *heading, &bold)
            .map_err(excel_error)?;
    }
    for (i, (row, url)) in rows.iter().zip(dokumen.iter()).enumerate() {
        let r = (i + 1) as u32;
        ws.write_number(r, 0, (i + 1) as f64).map_err(excel_error)?;
        write_text(ws, r, 1, Some(row.sub_bidang.as_str()))?;
        write_text(ws, r, 2, Some(row.nama_pengusul.as_str()))?;
        write_text(ws, r, 3, row.kecamatan_nama.as_deref())?;
        write_text(ws, r, 4, row.desa_nama.as_deref())?;
        write_text(ws, r, 5, Some(row.perihal.as_str()))?;
        let masuk = ymd_dmy(row.tanggal_surat_masuk);
        write_text(ws, r, 6, Some(masuk.as_str()))?;
        write_text(ws, r, 7, Some(row.nomor_surat_masuk.as_str()))?;
        let surat = ymd_dmy(row.tanggal_surat);
        write_text(ws, r, 8, Some(surat.as_str()))?;
        let pengajuan = row.created_at.map(|t| t.format("%d/%m/%Y %H:%M").to_string());
        write_text(ws, r, 9, pengajuan.as_deref())?;
        let link = if url.is_empty() { "-" } else { url.as_str() };
        write_text(ws, r, 10, Some(link))?;
    }
    ws.autofit();
    wb.save_to_buffer().map_err(excel_error)
}
