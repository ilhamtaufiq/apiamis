//! SPM sanitasi (`SpmSanitasiController`, model `SpmSanitasi`, tabel `tbl_spm_sanitasi`): daftar,
//! statistik, dan rute publik. Kapasitas ada di `spm_sanitasi_capaian`, tulis di `spm_sanitasi_write`.
//!
//! Bentuk model mengikuti `toArray()` Eloquent: semua kolom tabel (kolom decimal sebagai float, flag
//! `*_dari_integrasi` sebagai bool, timestamp Carbon), dan relasi `desa` dengan `kecamatan` bila dimuat.
//!
//! Belum dipindahkan: `show` (memuat pekerjaan beserta kegiatan, output, dan desanya), `integration*`,
//! `mck-pekerjaan`, attach/detach pekerjaan, export/template/import Excel, `stats/series`, dan
//! `public/spm-sanitasi/map-stats/series`.
//!
//! Perbedaan kecil: `per_page` dan `page` di bawah 1 dibatasi ke 1 (Laravel bisa 500 atau memotong dari akhir).

use std::{collections::HashMap, cmp::Ordering};

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{
    mysql::{MySqlArguments, MySqlRow},
    query::Query as SqlQuery,
    MySql, MySqlPool, Row,
};

use crate::{
    format::iso8601_utc, lookup::carbon_json, require_auth, spm_sanitasi_capaian, AppState,
};

/// Jenis infrastruktur yang dikenal (`in:` di validasi, dan urutan `by_jenis`).
pub const JENIS: &[&str] = &["spaldt", "spalds", "iplt", "mck_individu", "mck_komunal"];

/// Tipe kolom untuk pembacaan (`CAST`) dan pengikatan nilai.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Int,
    Num,
    Text,
    Time,
    Bool,
}

/// Semua kolom `tbl_spm_sanitasi` dalam urutan migrasi, dengan tipe baca.
pub const COLUMNS: &[(&str, Kind)] = &[
    ("id", Kind::Int),
    ("jenis", Kind::Text),
    ("desa_id", Kind::Int),
    ("skala_pelayanan", Kind::Text),
    ("nama_infrastruktur", Kind::Text),
    ("latitude", Kind::Num),
    ("longitude", Kind::Num),
    ("alamat_lengkap", Kind::Text),
    ("jumlah_pemanfaat_kk", Kind::Int),
    ("jumlah_pemanfaat_jiwa", Kind::Int),
    ("tahun_konstruksi", Kind::Int),
    ("pembiayaan_apbn", Kind::Num),
    ("pembiayaan_apbd", Kind::Num),
    ("pembiayaan_dak", Kind::Num),
    ("pembiayaan_hibah", Kind::Num),
    ("pembiayaan_csr", Kind::Num),
    ("pembiayaan_lain", Kind::Num),
    ("pembiayaan_total", Kind::Num),
    ("status_keberfungsian", Kind::Text),
    ("kualitas_keberfungsian", Kind::Text),
    ("pengelola", Kind::Text),
    ("kapasitas_desain", Kind::Num),
    ("kapasitas_terpakai", Kind::Num),
    ("kapasitas_tidak_terpakai", Kind::Num),
    ("jenis_pengolahan", Kind::Text),
    ("peta_cakupan", Kind::Text),
    ("status_lahan", Kind::Text),
    ("luas_lahan_ha", Kind::Text),
    ("opsi_teknologi", Kind::Text),
    ("jumlah_stasiun_pompa", Kind::Text),
    ("biaya_operasional", Kind::Num),
    ("jenis_pengelola", Kind::Text),
    ("sistem_pengolahan", Kind::Text),
    ("truk_tinja_unit", Kind::Int),
    ("kapasitas_truk_m3", Kind::Num),
    ("jumlah_ritasi", Kind::Int),
    ("jarak_maksimal_pelayanan_km", Kind::Num),
    ("alokasi_biaya_operasional", Kind::Num),
    ("created_at", Kind::Time),
    ("updated_at", Kind::Time),
    ("pemanfaat_dari_integrasi", Kind::Bool),
    ("pembiayaan_dari_integrasi", Kind::Bool),
];

const DESA_COLUMNS: &[(&str, Kind)] = &[
    ("id", Kind::Int),
    ("n_desa", Kind::Text),
    ("luas", Kind::Num),
    ("jumlah_penduduk", Kind::Int),
    ("jumlah_kk", Kind::Int),
    ("target", Kind::Int),
    ("bjp_master", Kind::Int),
    ("kecamatan_id", Kind::Int),
    ("created_at", Kind::Time),
    ("updated_at", Kind::Time),
];

const KECAMATAN_COLUMNS: &[(&str, Kind)] = &[
    ("id", Kind::Int),
    ("n_kec", Kind::Text),
    ("created_at", Kind::Time),
    ("updated_at", Kind::Time),
];

/// Fragmen `Desa::scopeRealWilayah` untuk alias `d`: nama desa wajar dan kecamatannya juga wilayah resmi.
pub const REAL_DESA: &str = "d.n_desa IS NOT NULL AND d.n_desa <> '' \
     AND LOWER(TRIM(d.n_desa)) NOT IN ('null', 'nulls') \
     AND EXISTS (SELECT 1 FROM tbl_kecamatan kr WHERE kr.id = d.kecamatan_id \
     AND kr.n_kec IS NOT NULL AND kr.n_kec <> '' AND LOWER(TRIM(kr.n_kec)) NOT IN ('null', 'nulls'))";

/// `Kecamatan::scopeRealWilayah` untuk tabel `tbl_kecamatan` tanpa alias.
pub const REAL_KECAMATAN: &str = "n_kec IS NOT NULL AND n_kec <> '' AND LOWER(TRIM(n_kec)) NOT IN ('null', 'nulls')";

/// Filter bersama untuk statistik dan kapasitas. `tahun` berupa teks yang sudah lolos truthiness PHP.
#[derive(Clone, Default)]
pub struct Scope {
    pub kecamatan: Option<i64>,
    pub jenis: Option<String>,
    pub tahun: Option<String>,
}

/// Nilai untuk `?` pada SQL dinamis.
#[derive(Clone)]
pub enum Arg {
    I(i64),
    S(String),
}

pub fn bind<'q>(
    mut q: SqlQuery<'q, MySql, MySqlArguments>,
    args: &[Arg],
) -> SqlQuery<'q, MySql, MySqlArguments> {
    for a in args {
        q = match a {
            Arg::I(v) => q.bind(*v),
            Arg::S(s) => q.bind(s.clone()),
        };
    }
    q
}

pub fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// `(int)` PHP untuk teks: angka di depan, selain itu 0.
pub fn php_int(raw: &str) -> i64 {
    let s = raw.trim_start();
    let bytes = s.as_bytes();
    let mut end = 0;
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
    }
    let digits = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits {
        return 0;
    }
    s[..end].parse::<i64>().unwrap_or(0)
}

/// `$request->filled(key)` dengan teks yang sudah dipangkas: kosong dianggap tidak ada.
pub fn input(q: &HashMap<String, String>, key: &str) -> Option<String> {
    q.get(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Truthiness PHP untuk `when($x)` dan `if ($x)`: string kosong dan `"0"` dianggap tidak ada.
pub fn truthy(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.is_empty() && s != "0")
}

/// `$request->integer(key) ?: null`.
pub fn int_or_null(q: &HashMap<String, String>, key: &str) -> Option<i64> {
    input(q, key).map(|v| php_int(&v)).filter(|v| *v != 0)
}

/// `$request->integer(key, default)`.
pub fn int_or(q: &HashMap<String, String>, key: &str, default: i64) -> i64 {
    match q.get(key) {
        Some(v) => php_int(v.trim()),
        None => default,
    }
}

/// Pilihan select untuk kolom: angka dan bool di-`CAST` agar tipe MySQL tidak mengganggu decode.
pub fn select_list(cols: &[(&str, Kind)]) -> String {
    cols.iter()
        .map(|(name, kind)| match kind {
            Kind::Int | Kind::Bool => format!("CAST({name} AS SIGNED) AS {name}"),
            Kind::Num => format!("CAST({name} AS DOUBLE) AS {name}"),
            Kind::Text | Kind::Time => (*name).to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Baca satu baris menjadi atribut model (urutan kolom dipertahankan).
pub fn read_map(row: &MySqlRow, cols: &[(&str, Kind)]) -> Result<Map<String, Value>, sqlx::Error> {
    let mut m = Map::new();
    for (name, kind) in cols {
        let v = match kind {
            Kind::Int => json!(row.try_get::<Option<i64>, _>(*name)?),
            Kind::Num => json!(row.try_get::<Option<f64>, _>(*name)?),
            Kind::Bool => json!(row.try_get::<Option<i64>, _>(*name)?.map(|x| x != 0)),
            Kind::Text => json!(row.try_get::<Option<String>, _>(*name)?),
            Kind::Time => carbon_json(row.try_get::<Option<DateTime<Utc>>, _>(*name)?),
        };
        m.insert((*name).to_string(), v);
    }
    Ok(m)
}

/// Satu baris `tbl_spm_sanitasi` sebagai atribut, atau `None`.
pub async fn find<'e, E>(exec: E, id: i64) -> Result<Option<Map<String, Value>>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!(
        "SELECT {} FROM tbl_spm_sanitasi WHERE id = ?",
        select_list(COLUMNS)
    );
    match sqlx::query(&sql).bind(id).fetch_optional(exec).await? {
        Some(row) => Ok(Some(read_map(&row, COLUMNS)?)),
        None => Ok(None),
    }
}

/// Desa beserta kecamatannya seperti `toArray()` (`null` bila desa tidak ada).
pub async fn desa_with_kecamatan(pool: &MySqlPool, desa_id: i64) -> Result<Value, ApiError> {
    let sql = format!("SELECT {} FROM tbl_desa WHERE id = ?", select_list(DESA_COLUMNS));
    let Some(row) = sqlx::query(&sql)
        .bind(desa_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
    else {
        return Ok(Value::Null);
    };
    let mut desa = read_map(&row, DESA_COLUMNS).map_err(internal)?;
    let kec_id = desa.get("kecamatan_id").and_then(Value::as_i64);
    let kecamatan = match kec_id {
        Some(k) => {
            let sql = format!(
                "SELECT {} FROM tbl_kecamatan WHERE id = ?",
                select_list(KECAMATAN_COLUMNS)
            );
            sqlx::query(&sql)
                .bind(k)
                .fetch_optional(pool)
                .await
                .map_err(internal)?
                .map(|r| read_map(&r, KECAMATAN_COLUMNS))
                .transpose()
                .map_err(internal)?
                .map_or(Value::Null, Value::Object)
        }
        None => Value::Null,
    };
    desa.insert("kecamatan".into(), kecamatan);
    Ok(Value::Object(desa))
}

/// Tambahkan relasi `desa` (dengan `kecamatan`) ke atribut, memakai cache per desa.
pub async fn with_desa(
    pool: &MySqlPool,
    mut attrs: Map<String, Value>,
    cache: &mut HashMap<i64, Value>,
) -> Result<Map<String, Value>, ApiError> {
    let desa = match attrs.get("desa_id").and_then(Value::as_i64) {
        Some(id) => match cache.get(&id) {
            Some(v) => v.clone(),
            None => {
                let v = desa_with_kecamatan(pool, id).await?;
                cache.insert(id, v.clone());
                v
            }
        },
        None => Value::Null,
    };
    attrs.insert("desa".into(), desa);
    Ok(attrs)
}

/// Satu baris dengan relasi `desa`, untuk respon tunggal.
pub async fn resource_of(pool: &MySqlPool, attrs: Map<String, Value>) -> Result<Value, ApiError> {
    let mut cache = HashMap::new();
    Ok(Value::Object(with_desa(pool, attrs, &mut cache).await?))
}

pub async fn fetch_row(pool: &MySqlPool, sql: &str, args: &[Arg]) -> Result<MySqlRow, ApiError> {
    bind(sqlx::query(sql), args)
        .fetch_one(pool)
        .await
        .map_err(internal)
}

/// Filter `tbl_spm_sanitasi s`. `desa_real`: desa pemilik harus wilayah resmi (`whereHas('desa', realWilayah)`).
/// `desa_required`: hanya baris yang punya desa (`whereNotNull('desa_id')`).
pub fn spm_filter(scope: &Scope, desa_real: bool, desa_required: bool) -> (String, Vec<Arg>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut args: Vec<Arg> = Vec::new();
    if desa_required {
        clauses.push("s.desa_id IS NOT NULL".into());
    }
    if desa_real || scope.kecamatan.is_some() {
        let mut inner = String::from("d.id = s.desa_id");
        if desa_real {
            inner.push_str(&format!(" AND {REAL_DESA}"));
        }
        if let Some(k) = scope.kecamatan {
            inner.push_str(" AND d.kecamatan_id = ?");
            args.push(Arg::I(k));
        }
        clauses.push(format!("EXISTS (SELECT 1 FROM tbl_desa d WHERE {inner})"));
    }
    if let Some(j) = &scope.jenis {
        clauses.push("s.jenis = ?".into());
        args.push(Arg::S(j.clone()));
    }
    if let Some(t) = &scope.tahun {
        clauses.push("s.tahun_konstruksi = ?".into());
        args.push(Arg::I(php_int(t)));
    }
    if clauses.is_empty() {
        ("1 = 1".into(), args)
    } else {
        (clauses.join(" AND "), args)
    }
}

/// `buildStats`: jumlah per jenis, total, berfungsi, pemanfaat, investasi, lalu ringkasan capaian.
async fn build_stats(pool: &MySqlPool, scope: &Scope) -> Result<Value, ApiError> {
    let (w, args) = spm_filter(scope, false, false);
    let counts_sql = format!(
        "SELECT s.jenis, CAST(COUNT(*) AS SIGNED) FROM tbl_spm_sanitasi s WHERE {w} GROUP BY s.jenis"
    );
    let mut counts: HashMap<String, i64> = HashMap::new();
    for r in bind(sqlx::query(&counts_sql), &args)
        .fetch_all(pool)
        .await
        .map_err(internal)?
    {
        counts.insert(r.try_get(0).map_err(internal)?, r.try_get(1).map_err(internal)?);
    }

    let totals_sql = format!(
        "SELECT CAST(COUNT(*) AS SIGNED), CAST(COALESCE(SUM(s.jumlah_pemanfaat_kk), 0) AS SIGNED), \
         CAST(COALESCE(SUM(s.pembiayaan_total), 0) AS DOUBLE), \
         CAST(COALESCE(SUM(CASE WHEN s.status_keberfungsian = 'Berfungsi' THEN 1 ELSE 0 END), 0) AS SIGNED) \
         FROM tbl_spm_sanitasi s WHERE {w}"
    );
    let t = fetch_row(pool, &totals_sql, &args).await?;
    let total_count: i64 = t.try_get(0).map_err(internal)?;
    let total_pemanfaat: i64 = t.try_get(1).map_err(internal)?;
    let total_investasi: f64 = t.try_get(2).map_err(internal)?;
    let berfungsi: i64 = t.try_get(3).map_err(internal)?;

    let capaian = spm_sanitasi_capaian::summary(pool, scope).await?;
    let mut out = Map::new();
    out.insert("spaldt_count".into(), json!(counts.get("spaldt").copied().unwrap_or(0)));
    out.insert("spalds_count".into(), json!(counts.get("spalds").copied().unwrap_or(0)));
    out.insert("iplt_count".into(), json!(counts.get("iplt").copied().unwrap_or(0)));
    out.insert("mck_individu_count".into(), json!(counts.get("mck_individu").copied().unwrap_or(0)));
    out.insert("mck_komunal_count".into(), json!(counts.get("mck_komunal").copied().unwrap_or(0)));
    out.insert("total_count".into(), json!(total_count));
    out.insert("berfungsi_count".into(), json!(berfungsi));
    out.insert("total_pemanfaat_kk".into(), json!(total_pemanfaat));
    out.insert("total_investasi".into(), json!(total_investasi));
    out.extend(capaian);
    Ok(Value::Object(out))
}

/// `GET /api/spm-sanitasi/stats`.
pub async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let scope = Scope {
        kecamatan: int_or_null(&q, "kecamatan_id"),
        jenis: truthy(input(&q, "jenis")),
        tahun: truthy(input(&q, "tahun")),
    };
    let data = build_stats(&state.pool, &scope).await?;
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// `GET /api/spm-sanitasi/stats/series?years=2020,2021,...&kecamatan_id=` (auth).
/// Sama dengan `statsSeries` Laravel: satu `build_stats` per tahun, tanpa filter jenis.
pub async fn stats_series(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let kecamatan = int_or_null(&q, "kecamatan_id");
    let mut data = Map::new();
    for y in years_param(q.get("years")) {
        let scope = Scope {
            kecamatan,
            jenis: None,
            tahun: Some(y.clone()),
        };
        let v = build_stats(&state.pool, &scope).await?;
        data.insert(y, v);
    }
    let data = if data.is_empty() {
        json!([])
    } else {
        Value::Object(data)
    };
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// Tahun dari `years=2020,2021,...`: 4 digit, unik, maksimal 20 (seperti `statsSeries`).
fn years_param(raw: Option<&String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in raw.map(String::as_str).unwrap_or("").split(',') {
        let y = part.trim();
        if y.len() == 4 && y.bytes().all(|b| b.is_ascii_digit()) && !out.iter().any(|x| x == y) {
            out.push(y.to_string());
        }
        if out.len() >= 20 {
            break;
        }
    }
    out
}

/// `GET /api/spm-sanitasi`: paginator Laravel (`data` dan `meta`), terbaru dulu.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let pool = &state.pool;

    let mut clauses: Vec<String> = Vec::new();
    let mut args: Vec<Arg> = Vec::new();
    if let Some(j) = input(&q, "jenis") {
        clauses.push("s.jenis = ?".into());
        args.push(Arg::S(j));
    }
    if let Some(k) = input(&q, "kecamatan_id") {
        clauses.push("EXISTS (SELECT 1 FROM tbl_desa d WHERE d.id = s.desa_id AND d.kecamatan_id = ?)".into());
        args.push(Arg::I(php_int(&k)));
    }
    if let Some(d) = input(&q, "desa_id") {
        clauses.push("s.desa_id = ?".into());
        args.push(Arg::I(php_int(&d)));
    }
    if let Some(s) = input(&q, "search") {
        let like = format!("%{s}%");
        clauses.push(
            "(s.nama_infrastruktur LIKE ? OR s.alamat_lengkap LIKE ? OR s.pengelola LIKE ? \
             OR EXISTS (SELECT 1 FROM tbl_desa d WHERE d.id = s.desa_id AND d.n_desa LIKE ?))"
                .into(),
        );
        for _ in 0..4 {
            args.push(Arg::S(like.clone()));
        }
    }
    if let Some(t) = input(&q, "tahun") {
        clauses.push("s.tahun_konstruksi = ?".into());
        args.push(Arg::I(php_int(&t)));
    }
    let where_sql = if clauses.is_empty() { "1 = 1".to_string() } else { clauses.join(" AND ") };

    let per_page = int_or(&q, "per_page", 15).max(1) as u64;
    let page = int_or(&q, "page", 1).max(1) as u64;

    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_spm_sanitasi s WHERE {where_sql}");
    let total: i64 = fetch_row(pool, &count_sql, &args)
        .await?
        .try_get(0)
        .map_err(internal)?;
    let total = total as u64;

    let sql = format!(
        "SELECT {} FROM tbl_spm_sanitasi s WHERE {where_sql} ORDER BY s.id DESC LIMIT {per_page} OFFSET {}",
        select_list(COLUMNS),
        (page - 1) * per_page
    );
    let rows = bind(sqlx::query(&sql), &args)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    let mut cache = HashMap::new();
    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        let attrs = read_map(row, COLUMNS).map_err(internal)?;
        data.push(Value::Object(with_desa(pool, attrs, &mut cache).await?));
    }

    let last_page = total.div_ceil(per_page).max(1);
    Ok(Json(json!({
        "success": true,
        "data": data,
        "meta": {
            "current_page": page,
            "last_page": last_page,
            "per_page": per_page,
            "total": total,
        },
    }))
    .into_response())
}

/// Statistik publik: ringkasan capaian tanpa login, dengan total dan jumlah per jenis.
pub async fn public_stats(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let jenis = truthy(input(&q, "jenis"));
    let tahun = truthy(input(&q, "tahun"));
    let scope = Scope {
        kecamatan: None,
        jenis: jenis.clone(),
        tahun: tahun.clone(),
    };
    let pool = &state.pool;
    let mut out = spm_sanitasi_capaian::summary(pool, &scope).await?;

    let (w, args) = spm_filter(&scope, false, true);
    let counts_sql = format!(
        "SELECT s.jenis, CAST(COUNT(*) AS SIGNED) FROM tbl_spm_sanitasi s WHERE {w} GROUP BY s.jenis"
    );
    let mut counts: HashMap<String, i64> = HashMap::new();
    for r in bind(sqlx::query(&counts_sql), &args)
        .fetch_all(pool)
        .await
        .map_err(internal)?
    {
        counts.insert(r.try_get(0).map_err(internal)?, r.try_get(1).map_err(internal)?);
    }

    let totals_sql = format!(
        "SELECT CAST(COUNT(*) AS SIGNED), \
         CAST(COALESCE(SUM(CASE WHEN s.status_keberfungsian = 'Berfungsi' THEN 1 ELSE 0 END), 0) AS SIGNED), \
         CAST(COALESCE(SUM(s.pembiayaan_total), 0) AS DOUBLE) \
         FROM tbl_spm_sanitasi s WHERE {w}"
    );
    let t = fetch_row(pool, &totals_sql, &args).await?;
    let total_count: i64 = t.try_get(0).map_err(internal)?;
    let berfungsi: i64 = t.try_get(1).map_err(internal)?;
    let total_investasi: f64 = t.try_get(2).map_err(internal)?;

    let wilayah: i64 = sqlx::query_scalar(&format!(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_kecamatan WHERE {REAL_KECAMATAN}"
    ))
    .fetch_one(pool)
    .await
    .map_err(internal)?;

    let scope_label = match &tahun {
        Some(t) => format!("Infrastruktur tahun konstruksi {t}"),
        None => "Semua infrastruktur terdata".to_string(),
    };
    out.extend([
        ("scope_label".to_string(), json!(scope_label)),
        ("stats_generated_at".to_string(), iso8601_utc(Some(Utc::now()))),
        ("total_count".to_string(), json!(total_count)),
        ("berfungsi_count".to_string(), json!(berfungsi)),
        ("total_investasi".to_string(), json!(total_investasi)),
        ("wilayah_total_kecamatan".to_string(), json!(wilayah)),
        ("spaldt_count".to_string(), json!(counts.get("spaldt").copied().unwrap_or(0))),
        ("spalds_count".to_string(), json!(counts.get("spalds").copied().unwrap_or(0))),
        ("iplt_count".to_string(), json!(counts.get("iplt").copied().unwrap_or(0))),
        ("mck_individu_count".to_string(), json!(counts.get("mck_individu").copied().unwrap_or(0))),
        ("mck_komunal_count".to_string(), json!(counts.get("mck_komunal").copied().unwrap_or(0))),
    ]);
    Ok(Json(json!({ "success": true, "data": Value::Object(out) })).into_response())
}

/// Peta statistik per desa (publik): satu baris per desa wilayah resmi, diurutkan nama desa.
pub async fn public_map_stats(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let jenis = truthy(input(&q, "jenis"));
    let tahun = truthy(input(&q, "tahun"));
    let data = spm_sanitasi_capaian::map_stats(&state.pool, jenis.as_deref(), tahun.as_deref()).await?;
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// Urutan PHP untuk membandingkan dua string di `sort` (angka numerik dibanding angka, sisanya byte).
pub fn php_cmp_str(a: &str, b: &str) -> Ordering {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) if x.is_finite() && y.is_finite() => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => a.cmp(b),
    }
}
