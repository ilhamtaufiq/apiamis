//! `GET /api/pekerjaan/document-register` (`PekerjaanController@documentRegister`).
//!
//! Laravel tidak memakai Resource di sini. Setiap paket dikirim sebagai model `Pekerjaan` mentah dengan
//! relasi dari `documentRegisterEagerLoads()`, sehingga bentuk JSON disusun dari:
//! - kolom tabel (urutan kolom `SELECT *`), dengan cast dari `$casts` model,
//! - `withCount(['foto', 'penerima'])` sebagai `foto_count` dan `penerima_count`,
//! - relasi dengan nama metode sebagai kuncinya: `kontrak`, `kegiatan`, `beritaAcara`, `output`, `berkas`.
//!
//! Kontrak memuat `pivot` (kolom `kontrak_pekerjaan`), `penyedia`, dan `registers` (masing-masing dengan `type`).
//! Tidak ada `addendum` di respons ini karena Laravel tidak memuatnya.
//!
//! Catatan paritas (daftar lengkap ada di laporan):
//! - Paket konsolidasi tambahan (berbagi kontrak dengan paket di halaman, di luar halaman) tetap memakai
//!   scope `byUserRole()` (berbeda dari Laravel, agar non-admin tidak melihat paket di luar scope), dan
//!   tidak memakai filter request, seperti Laravel.
//! - Laravel tidak memakai `ORDER BY`. Di sini paket diurutkan `p.id`, dan relasi berurutan `id`.
//! - Datetime memakai bentuk Carbon mentah (`.000000Z`), belum diverifikasi terhadap respons Laravel.

use std::collections::{BTreeSet, HashMap};

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySqlPool, Row};

use crate::{
    access,
    format::number_like_php,
    kegiatan_write,
    lookup::{carbon_json, document_type_json, DocumentTypeRow},
    require_auth, AppState,
};

const PER_PAGE_DEFAULT: i64 = 20;
const PHP_TRIM: &[char] = &[' ', '\t', '\n', '\r', '\0', '\x0B'];

/// `Pekerjaan::has('kontrak')`: relasi belongsToMany lewat `kontrak_pekerjaan` (bukan `id_pekerjaan`).
const HAS_KONTRAK: &str = "EXISTS (SELECT 1 FROM kontrak_pekerjaan hkp INNER JOIN tbl_kontrak hk \
     ON hk.id = hkp.kontrak_id WHERE hkp.pekerjaan_id = p.id)";

/// Tipe kolom untuk pembacaan dan cast Eloquent.
#[derive(Clone, Copy)]
pub(crate) enum K {
    /// Integer (cast `integer` atau kolom tanpa cast).
    Int,
    /// `boolean` dari tinyint.
    Bool,
    /// String apa adanya.
    Txt,
    /// Angka desimal dari cast `float` atau `decimal` (dibaca sebagai DOUBLE).
    Flt,
    /// String dari kolom `decimal` tanpa cast numerik (`decimal:2` menjadi "12.00").
    Chr,
    /// Cast `date` (Carbon, tengah malam UTC).
    Date,
    /// Cast `datetime` atau timestamp Eloquent.
    Ts,
    /// Cast `array` dari kolom JSON atau longtext berisi JSON.
    Json,
}

pub(crate) const PEKERJAAN_COLS: &[(&str, K)] = &[
    ("id", K::Int),
    ("kode_rekening", K::Txt),
    ("nama_paket", K::Txt),
    ("kecamatan_id", K::Int),
    ("desa_id", K::Int),
    ("kegiatan_id", K::Int),
    ("pagu", K::Flt),
    ("is_konsultan", K::Bool),
    ("status", K::Txt),
    ("catatan", K::Txt),
    ("created_at", K::Ts),
    ("updated_at", K::Ts),
    ("pengawas_id", K::Int),
    ("pendamping_id", K::Int),
];

pub(crate) const KONTRAK_COLS: &[(&str, K)] = &[
    ("id", K::Int),
    ("id_kegiatan", K::Int),
    ("id_pekerjaan", K::Int),
    ("id_penyedia", K::Int),
    ("kode_rup", K::Txt),
    ("kode_paket", K::Txt),
    ("nomor_penawaran", K::Txt),
    ("tanggal_penawaran", K::Date),
    ("nilai_kontrak", K::Flt),
    ("tgl_sppbj", K::Date),
    ("tgl_spk", K::Date),
    ("tgl_spmk", K::Date),
    ("tgl_selesai", K::Date),
    ("sppbj", K::Txt),
    ("spk", K::Txt),
    ("spmk", K::Txt),
    ("spse_sppbj_id", K::Txt),
    ("spse_spk_id", K::Txt),
    ("spse_rekanan_id", K::Txt),
    ("spse_pushed_at", K::Ts),
    ("spse_push_log", K::Json),
    ("created_at", K::Ts),
    ("updated_at", K::Ts),
];

pub(crate) const PENYEDIA_COLS: &[(&str, K)] = &[
    ("id", K::Int),
    ("nama", K::Txt),
    ("direktur", K::Txt),
    ("no_akta", K::Txt),
    ("notaris", K::Txt),
    ("tanggal_akta", K::Date),
    ("alamat", K::Txt),
    ("npwp", K::Txt),
    ("bank", K::Txt),
    ("norek", K::Txt),
    ("created_at", K::Ts),
    ("updated_at", K::Ts),
];

const REGISTER_COLS: &[(&str, K)] = &[
    ("id", K::Int),
    ("kontrak_id", K::Int),
    ("type_id", K::Int),
    ("addendum_id", K::Int),
    ("attachment_type", K::Txt),
    ("nomor", K::Txt),
    ("tanggal", K::Date),
    ("sequence_number", K::Int),
    ("year", K::Int),
    ("description", K::Txt),
    ("nilai", K::Flt),
    ("created_at", K::Ts),
    ("updated_at", K::Ts),
];

const BERITA_COLS: &[(&str, K)] = &[
    ("id", K::Int),
    ("pekerjaan_id", K::Int),
    ("data", K::Json),
    ("created_at", K::Ts),
    ("updated_at", K::Ts),
];

/// Relasi `output` memakai `select` terbatas di Laravel, sehingga urutan kolomnya mengikuti daftar itu.
const OUTPUT_COLS: &[(&str, K)] = &[
    ("id", K::Int),
    ("pekerjaan_id", K::Int),
    ("komponen", K::Txt),
    ("volume", K::Chr),
    ("satuan", K::Txt),
    ("penerima_is_optional", K::Bool),
];

const BERKAS_COLS: &[(&str, K)] = &[
    ("id", K::Int),
    ("pekerjaan_id", K::Int),
    ("jenis_dokumen", K::Txt),
];

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// `filled()` Laravel untuk string: tidak kosong setelah `trim()` PHP.
fn filled(v: &str) -> bool {
    !v.trim_matches(PHP_TRIM).is_empty()
}

/// `(int)` PHP untuk string: awalan angka desimal, selain itu 0.
fn php_int(s: &str) -> i64 {
    let t = s.trim_start();
    let (sign, digits) = match t.as_bytes().first() {
        Some(b'-') => (-1, &t[1..]),
        Some(b'+') => (1, &t[1..]),
        _ => (1, t),
    };
    let num: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse::<i64>().map(|v| sign * v).unwrap_or(0)
}

/// `empty()` PHP untuk nilai hasil `json_decode`.
fn php_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f == 0.0),
        Value::String(s) => s.is_empty() || s == "0",
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// Hasil `json_decode(..., true)`: objek kosong menjadi array kosong PHP (`[]`).
fn php_decoded(v: Value) -> Value {
    match v {
        Value::Object(m) if m.is_empty() => Value::Array(Vec::new()),
        Value::Object(m) => {
            Value::Object(m.into_iter().map(|(k, v)| (k, php_decoded(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(php_decoded).collect()),
        other => other,
    }
}

fn json_column(raw: Option<String>) -> Value {
    match raw {
        Some(s) => serde_json::from_str::<Value>(&s)
            .map(php_decoded)
            .unwrap_or(Value::Null),
        None => Value::Null,
    }
}

/// Cast `date` Laravel: Carbon tengah malam UTC.
fn date_json(d: Option<NaiveDate>) -> Value {
    match d {
        Some(d) => json!(format!("{}T00:00:00.000000Z", d.format("%Y-%m-%d"))),
        None => Value::Null,
    }
}

/// Daftar kolom `SELECT` dengan cast yang sesuai tipe. `alias` kosong berarti tanpa prefiks.
pub(crate) fn select_list(alias: &str, cols: &[(&str, K)]) -> String {
    let p = if alias.is_empty() {
        String::new()
    } else {
        format!("{alias}.")
    };
    cols.iter()
        .map(|(c, k)| match k {
            K::Int | K::Bool => format!("CAST({p}{c} AS SIGNED) AS {c}"),
            K::Flt => format!("CAST({p}{c} AS DOUBLE) AS {c}"),
            K::Chr | K::Json => format!("CAST({p}{c} AS CHAR) AS {c}"),
            K::Txt | K::Date | K::Ts => format!("{p}{c}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Membaca baris menjadi atribut JSON, dengan cast Eloquent per kolom.
pub(crate) fn read(r: &MySqlRow, cols: &[(&str, K)]) -> Result<Map<String, Value>, sqlx::Error> {
    let mut m = Map::new();
    for (c, k) in cols {
        let v = match k {
            K::Int => json!(r.try_get::<Option<i64>, _>(*c)?),
            K::Bool => r
                .try_get::<Option<i64>, _>(*c)?
                .map_or(Value::Null, |v| json!(v != 0)),
            K::Txt | K::Chr => json!(r.try_get::<Option<String>, _>(*c)?),
            K::Flt => r
                .try_get::<Option<f64>, _>(*c)?
                .map_or(Value::Null, number_like_php),
            K::Date => date_json(r.try_get::<Option<NaiveDate>, _>(*c)?),
            K::Ts => carbon_json(r.try_get::<Option<DateTime<Utc>>, _>(*c)?),
            K::Json => json_column(r.try_get::<Option<String>, _>(*c)?),
        };
        m.insert((*c).to_string(), v);
    }
    Ok(m)
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// Filter `search` dan `tahun` dari `documentRegister`, dengan urutan bind yang sama dengan SQL.
fn filters(query: &HashMap<String, String>) -> (String, Vec<String>) {
    let mut sql = String::new();
    let mut binds = Vec::new();
    if let Some(t) = query.get("tahun").filter(|v| filled(v)) {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM tbl_kegiatan fg WHERE fg.id = p.kegiatan_id AND fg.tahun_anggaran = ?)",
        );
        binds.push(t.clone());
    }
    if let Some(s) = query.get("search").filter(|v| filled(v)) {
        let like = format!("%{s}%");
        sql.push_str(
            " AND (p.nama_paket LIKE ? OR p.kode_rekening LIKE ? OR EXISTS (SELECT 1 FROM kontrak_pekerjaan skp \
             INNER JOIN tbl_kontrak sk ON sk.id = skp.kontrak_id WHERE skp.pekerjaan_id = p.id AND (\
             sk.sppbj LIKE ? OR sk.spk LIKE ? OR sk.spmk LIKE ? \
             OR EXISTS (SELECT 1 FROM tbl_penyedia spy WHERE spy.id = sk.id_penyedia AND spy.nama LIKE ?) \
             OR EXISTS (SELECT 1 FROM tbl_document_registers sr WHERE sr.kontrak_id = sk.id \
             AND (sr.nomor LIKE ? OR sr.description LIKE ?)))))",
        );
        binds.extend((0..8).map(|_| like.clone()));
    }
    (sql, binds)
}

/// Jumlah paket dan ringkasan `spk_missing`, `spmk_missing`, `pho_completed` atas seluruh hasil filter.
async fn summary(pool: &MySqlPool, where_sql: &str, binds: &[String]) -> Result<Value, ApiError> {
    let sql = format!(
        "SELECT CAST(p.id AS SIGNED) AS id, k.spk, k.spmk, CAST(ba.data AS CHAR) AS data \
         FROM tbl_pekerjaan p INNER JOIN kontrak_pekerjaan kp ON kp.pekerjaan_id = p.id \
         INNER JOIN tbl_kontrak k ON k.id = kp.kontrak_id \
         LEFT JOIN tbl_berita_acara ba ON ba.pekerjaan_id = p.id{where_sql}"
    );
    let mut q = sqlx::query(&sql);
    for b in binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await.map_err(internal)?;

    // (ada spk terisi, ada spmk terisi, ada serah terima pertama)
    let mut flags: HashMap<i64, (bool, bool, bool)> = HashMap::new();
    for r in &rows {
        let id: i64 = r.try_get("id").map_err(internal)?;
        let e = flags.entry(id).or_insert((false, false, false));
        let spk: Option<String> = r.try_get("spk").map_err(internal)?;
        let spmk: Option<String> = r.try_get("spmk").map_err(internal)?;
        e.0 |= spk.as_deref().is_some_and(filled);
        e.1 |= spmk.as_deref().is_some_and(filled);
        let data: Option<String> = r.try_get("data").map_err(internal)?;
        if let Some(Value::Object(m)) = data.and_then(|d| serde_json::from_str::<Value>(&d).ok()) {
            if m.get("serah_terima_pertama").is_some_and(|v| !php_empty(v)) {
                e.2 = true;
            }
        }
    }
    let total = flags.len() as i64;
    let spk_ok = flags.values().filter(|f| f.0).count() as i64;
    let spmk_ok = flags.values().filter(|f| f.1).count() as i64;
    let pho = flags.values().filter(|f| f.2).count() as i64;
    Ok(json!({
        "spk_missing": total - spk_ok,
        "spmk_missing": total - spmk_ok,
        "pho_completed": pho,
    }))
}

/// Atribut paket (dengan `foto_count` dan `penerima_count`) untuk setiap baris di `where_sql`.
async fn fetch_rows(
    pool: &MySqlPool,
    where_sql: &str,
    binds: &[String],
    page: Option<(u64, u64)>,
) -> Result<Vec<(i64, Map<String, Value>)>, ApiError> {
    let mut sql = format!(
        "SELECT {}, (SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_foto f WHERE f.pekerjaan_id = p.id) AS foto_count, \
         (SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_penerima x WHERE x.pekerjaan_id = p.id) AS penerima_count \
         FROM tbl_pekerjaan p{where_sql} ORDER BY p.id",
        select_list("p", PEKERJAAN_COLS)
    );
    if let Some((limit, offset)) = page {
        sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));
    }
    let mut q = sqlx::query(&sql);
    for b in binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await.map_err(internal)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let id: i64 = r.try_get("id").map_err(internal)?;
        let mut m = read(r, PEKERJAAN_COLS).map_err(internal)?;
        m.insert(
            "foto_count".into(),
            json!(r.try_get::<i64, _>("foto_count").map_err(internal)?),
        );
        m.insert(
            "penerima_count".into(),
            json!(r.try_get::<i64, _>("penerima_count").map_err(internal)?),
        );
        out.push((id, m));
    }
    Ok(out)
}

async fn count(pool: &MySqlPool, where_sql: &str, binds: &[String]) -> Result<u64, ApiError> {
    let sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_pekerjaan p{where_sql}");
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    for b in binds {
        q = q.bind(b);
    }
    Ok(q.fetch_one(pool).await.map_err(internal)? as u64)
}

async fn penyedia_json(pool: &MySqlPool, id: Option<i64>) -> Result<Value, ApiError> {
    let Some(id) = id else {
        return Ok(Value::Null);
    };
    let sql = format!(
        "SELECT {} FROM tbl_penyedia WHERE id = ?",
        select_list("", PENYEDIA_COLS)
    );
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    match row {
        Some(r) => Ok(Value::Object(read(&r, PENYEDIA_COLS).map_err(internal)?)),
        None => Ok(Value::Null),
    }
}

/// `DocumentType` untuk `register.type`; null bila baris tipe tidak ada.
async fn type_json(pool: &MySqlPool, id: Option<i64>) -> Result<Value, ApiError> {
    let Some(id) = id else {
        return Ok(Value::Null);
    };
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, name, code, format_template, created_at, updated_at \
         FROM tbl_document_types WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(r) = row else {
        return Ok(Value::Null);
    };
    let t = DocumentTypeRow {
        id: r.try_get::<i64, _>("id").map_err(internal)? as u64,
        name: r.try_get("name").map_err(internal)?,
        code: r.try_get("code").map_err(internal)?,
        format_template: r.try_get("format_template").map_err(internal)?,
        created_at: r.try_get("created_at").map_err(internal)?,
        updated_at: r.try_get("updated_at").map_err(internal)?,
    };
    Ok(document_type_json(&t))
}

async fn registers_json(pool: &MySqlPool, kontrak_id: i64) -> Result<Vec<Value>, ApiError> {
    let sql = format!(
        "SELECT {} FROM tbl_document_registers r WHERE r.kontrak_id = ? ORDER BY r.id",
        select_list("r", REGISTER_COLS)
    );
    let rows = sqlx::query(&sql)
        .bind(kontrak_id)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let mut m = read(r, REGISTER_COLS).map_err(internal)?;
        let type_id = m.get("type_id").and_then(Value::as_i64);
        let ty = type_json(pool, type_id).await?;
        m.insert("type".into(), ty);
        out.push(Value::Object(m));
    }
    Ok(out)
}

/// `pekerjaan.kontrak`: kontrak lewat pivot, dengan `pivot`, `penyedia`, dan `registers.type`.
async fn kontrak_list(pool: &MySqlPool, pekerjaan_id: i64) -> Result<Vec<Value>, ApiError> {
    let sql = format!(
        "SELECT {}, CAST(kp.pekerjaan_id AS SIGNED) AS pivot_pekerjaan_id, \
         CAST(kp.kontrak_id AS SIGNED) AS pivot_kontrak_id, kp.created_at AS pivot_created_at, \
         kp.updated_at AS pivot_updated_at \
         FROM kontrak_pekerjaan kp INNER JOIN tbl_kontrak k ON k.id = kp.kontrak_id \
         WHERE kp.pekerjaan_id = ? ORDER BY k.id",
        select_list("k", KONTRAK_COLS)
    );
    let rows = sqlx::query(&sql)
        .bind(pekerjaan_id)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let mut m = read(r, KONTRAK_COLS).map_err(internal)?;
        m.insert(
            "pivot".into(),
            json!({
                "pekerjaan_id": r.try_get::<i64, _>("pivot_pekerjaan_id").map_err(internal)?,
                "kontrak_id": r.try_get::<i64, _>("pivot_kontrak_id").map_err(internal)?,
                "created_at": carbon_json(r.try_get("pivot_created_at").map_err(internal)?),
                "updated_at": carbon_json(r.try_get("pivot_updated_at").map_err(internal)?),
            }),
        );
        let penyedia_id = m.get("id_penyedia").and_then(Value::as_i64);
        let kontrak_id = m.get("id").and_then(Value::as_i64).unwrap_or_default();
        m.insert("penyedia".into(), penyedia_json(pool, penyedia_id).await?);
        m.insert(
            "registers".into(),
            Value::Array(registers_json(pool, kontrak_id).await?),
        );
        out.push(Value::Object(m));
    }
    Ok(out)
}

async fn berita_acara_json(pool: &MySqlPool, pekerjaan_id: i64) -> Result<Value, ApiError> {
    let sql = format!(
        "SELECT {} FROM tbl_berita_acara WHERE pekerjaan_id = ? LIMIT 1",
        select_list("", BERITA_COLS)
    );
    let row = sqlx::query(&sql)
        .bind(pekerjaan_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    match row {
        Some(r) => Ok(Value::Object(read(&r, BERITA_COLS).map_err(internal)?)),
        None => Ok(Value::Null),
    }
}

async fn output_list(pool: &MySqlPool, pekerjaan_id: i64) -> Result<Vec<Value>, ApiError> {
    let sql = format!(
        "SELECT {} FROM tbl_output WHERE pekerjaan_id = ? ORDER BY id",
        select_list("", OUTPUT_COLS)
    );
    let rows = sqlx::query(&sql)
        .bind(pekerjaan_id)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    rows.iter()
        .map(|r| read(r, OUTPUT_COLS).map(Value::Object).map_err(internal))
        .collect()
}

async fn berkas_list(pool: &MySqlPool, pekerjaan_id: i64) -> Result<Vec<Value>, ApiError> {
    let sql = format!(
        "SELECT {} FROM tbl_berkas WHERE pekerjaan_id = ? ORDER BY id",
        select_list("", BERKAS_COLS)
    );
    let rows = sqlx::query(&sql)
        .bind(pekerjaan_id)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    rows.iter()
        .map(|r| read(r, BERKAS_COLS).map(Value::Object).map_err(internal))
        .collect()
}

/// Satu paket dengan relasi yang dimuat `documentRegisterEagerLoads()`, urutan kunci seperti Laravel.
async fn item(pool: &MySqlPool, attrs: Map<String, Value>) -> Result<Value, ApiError> {
    let id = attrs.get("id").and_then(Value::as_i64).unwrap_or_default();
    let kegiatan_id = attrs.get("kegiatan_id").and_then(Value::as_i64);
    let kegiatan = match kegiatan_id {
        Some(k) => kegiatan_write::attributes(pool, k as u64)
            .await?
            .map_or(Value::Null, Value::Object),
        None => Value::Null,
    };
    let mut m = attrs;
    m.insert(
        "kontrak".into(),
        Value::Array(kontrak_list(pool, id).await?),
    );
    m.insert("kegiatan".into(), kegiatan);
    m.insert("beritaAcara".into(), berita_acara_json(pool, id).await?);
    m.insert("output".into(), Value::Array(output_list(pool, id).await?));
    m.insert("berkas".into(), Value::Array(berkas_list(pool, id).await?));
    Ok(Value::Object(m))
}

/// `kontrak[].id` dari satu paket, untuk konsolidasi.
fn kontrak_ids(item: &Value) -> Vec<i64> {
    item.get("kontrak")
        .and_then(Value::as_array)
        .map(|ks| {
            ks.iter()
                .filter_map(|k| k.get("id").and_then(Value::as_i64))
                .collect()
        })
        .unwrap_or_default()
}

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let pool = &state.pool;
    let roles = auth::login::roles_of(pool, user.user_id)
        .await
        .map_err(internal)?;
    let scope = access::restriction(user.user_id, &roles, "p");

    // Urutan: has('kontrak'), filter request, lalu scopeByUserRole (sama dengan Laravel).
    let (filter_sql, mut binds) = filters(&query);
    let where_sql = format!(" WHERE {HAS_KONTRAK}{filter_sql}{}", scope.sql);
    binds.extend(scope.binds.iter().map(u64::to_string));

    let summary = summary(pool, &where_sql, &binds).await?;
    let per = php_int(query.get("per_page").map_or("20", String::as_str));

    if per == -1 {
        let rows = fetch_rows(pool, &where_sql, &binds, None).await?;
        let total = rows.len();
        let mut data = Vec::with_capacity(total);
        for (_, attrs) in rows {
            data.push(item(pool, attrs).await?);
        }
        return Ok(Json(json!({
            "data": data,
            "meta": { "total": total, "summary": summary },
        }))
        .into_response());
    }

    // `paginate(0)` di Laravel gagal (DivisionByZero di lastPage); di sini dipakai 20.
    let per_page: u64 = if per <= 0 {
        PER_PAGE_DEFAULT as u64
    } else {
        per as u64
    };
    let page: u64 = query
        .get("page")
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|p| *p >= 1)
        .map_or(1, |p| p as u64);
    let total = count(pool, &where_sql, &binds).await?;
    let offset = (page - 1).saturating_mul(per_page);

    let page_rows = fetch_rows(pool, &where_sql, &binds, Some((per_page, offset))).await?;
    let page_ids: Vec<i64> = page_rows.iter().map(|(id, _)| *id).collect();
    let mut items = Vec::with_capacity(page_rows.len());
    for (_, attrs) in page_rows {
        items.push(item(pool, attrs).await?);
    }
    let page_count = items.len();

    // Konsolidasi: paket lain yang berbagi kontrak dengan halaman ini ikut ditambahkan di akhir.
    // Berbeda dari Laravel: scope byUserRole() tetap dipakai, supaya pengguna tidak melihat paket
    // di luar scope-nya lewat kontrak bersama. Filter request tidak dipakai, seperti Laravel.
    let shared_kontrak: BTreeSet<i64> = items.iter().flat_map(kontrak_ids).collect();
    if !shared_kontrak.is_empty() {
        let mut extra_sql = format!(
            " WHERE {HAS_KONTRAK} AND p.id IN (SELECT kp.pekerjaan_id FROM kontrak_pekerjaan kp \
             WHERE kp.kontrak_id IN ({}))",
            placeholders(shared_kontrak.len())
        );
        let mut extra_binds: Vec<String> = shared_kontrak.iter().map(i64::to_string).collect();
        if !page_ids.is_empty() {
            extra_sql.push_str(&format!(
                " AND p.id NOT IN ({})",
                placeholders(page_ids.len())
            ));
            extra_binds.extend(page_ids.iter().map(i64::to_string));
        }
        extra_sql.push_str(&scope.sql);
        extra_binds.extend(scope.binds.iter().map(u64::to_string));
        for (_, attrs) in fetch_rows(pool, &extra_sql, &extra_binds, None).await? {
            items.push(item(pool, attrs).await?);
        }
    }

    let last_page = total.div_ceil(per_page).max(1);
    let (from, to) = if page_count > 0 {
        let first = (page - 1).saturating_mul(per_page) + 1;
        (json!(first), json!(first + page_count as u64 - 1))
    } else {
        (Value::Null, Value::Null)
    };

    Ok(Json(json!({
        "success": true,
        "data": items,
        "meta": {
            "current_page": page,
            "last_page": last_page,
            "per_page": per_page,
            "total": total,
            "from": from,
            "to": to,
            "summary": summary,
        },
    }))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_filled_treats_whitespace_as_blank() {
        assert!(!filled(""));
        assert!(!filled("   \t"));
        assert!(filled("0"));
        assert!(filled(" a "));
    }

    #[test]
    fn php_empty_matches_php_rules() {
        assert!(php_empty(&json!("0")));
        assert!(php_empty(&json!("")));
        assert!(php_empty(&json!(0)));
        assert!(php_empty(&json!(false)));
        assert!(php_empty(&json!(null)));
        assert!(!php_empty(&json!("2099-01-01")));
        assert!(!php_empty(&json!("00")));
        assert!(!php_empty(&json!(true)));
    }

    #[test]
    fn empty_json_object_decodes_to_php_empty_array() {
        assert_eq!(php_decoded(json!({})), json!([]));
        assert_eq!(php_decoded(json!({"a": {}})), json!({"a": []}));
    }

    #[test]
    fn php_int_reads_leading_digits() {
        assert_eq!(php_int("-1"), -1);
        assert_eq!(php_int(" 12abc"), 12);
        assert_eq!(php_int("abc"), 0);
    }

    #[test]
    fn date_cast_is_utc_midnight_carbon() {
        let d = NaiveDate::from_ymd_opt(2026, 4, 23);
        assert_eq!(date_json(d), json!("2026-04-23T00:00:00.000000Z"));
        assert_eq!(date_json(None), Value::Null);
    }

    #[test]
    fn filters_follow_laravel_order_and_bind_count() {
        let mut q = HashMap::new();
        q.insert("tahun".to_string(), "2099".to_string());
        q.insert("search".to_string(), "uji".to_string());
        let (sql, binds) = filters(&q);
        assert_eq!(binds.len(), 1 + 8);
        assert_eq!(sql.matches('?').count(), binds.len());
        assert_eq!(binds[0], "2099");
        assert_eq!(binds[1], "%uji%");
    }

    #[test]
    fn blank_filters_are_ignored() {
        let mut q = HashMap::new();
        q.insert("tahun".to_string(), "  ".to_string());
        q.insert("search".to_string(), String::new());
        let (sql, binds) = filters(&q);
        assert!(sql.is_empty());
        assert!(binds.is_empty());
    }
}
