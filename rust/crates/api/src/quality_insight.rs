//! Kualitas data dan kotak masuk tindak lanjut, plus laporan error dari klien.
//!
//! - `GET /api/data-quality/stats`, `items`, `action-inbox`: port `DataQualityController`
//!   (admin only, seperti `role:admin` di Laravel).
//! - `POST /api/client-error-reports`: port `ClientErrorReportController@store` (login, tanpa role).
//!   Tabel `error_logs` dibuat dari `rust/fixtures/error_logs_schema.sql`. Rute admin `/api/error-logs`
//!   belum dipindah (grup lain).
//!
//! Perbedaan yang diketahui dengan Laravel:
//! - Urutan `items` memakai `nama_paket, id` supaya paginasi stabil. Laravel hanya `nama_paket`, jadi
//!   urutan antar nama yang sama tidak ditentukan.
//! - `ip_address` diambil dari `X-Forwarded-For` (sama dengan `audit::client_info`). Tanpa header itu
//!   Laravel menyimpan IP socket, sedangkan Rust menyimpan NULL karena router tidak membaca `ConnectInfo`.
//! - Body form-encoded tidak didukung di `POST /api/client-error-reports`, hanya JSON.
//! - `pagu` dibaca sebagai `FLOAT` (single precision, T9), lalu ditulis ulang seperti `float_json` di
//!   `master_fase.rs`.
//! - Pengecekan `Schema::hasTable('tbl_tiket')` tidak dibuat. Tabel itu diasumsikan ada.
//! - `now()` di Laravel memakai zona `UTC` (`config/app.php`), jadi tanggal kontrak "berakhir ≤ 30 hari"
//!   dihitung dari tanggal UTC.
//! - Pencarian `LIKE` tidak di-escape, sama dengan Laravel (`%` dan `_` dari input ikut berlaku).

use std::collections::{BTreeMap, HashMap};

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Days, NaiveDate, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{audit, format::iso8601_utc, notifications::require_admin, require_auth, AppState};

const DEFAULT_PER_PAGE: u64 = 25;
const ISSUES: &[&str] = &[
    "no_coordinates",
    "no_photos",
    "started_no_photos",
    "no_contracts",
];
const SOURCES: &[&str] = &[
    "react",
    "react-native",
    "window.error",
    "unhandledrejection",
    "console.error",
    "manual",
    "fatal",
];

type Errors = BTreeMap<String, Vec<String>>;

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn invalid(errors: Errors) -> ApiError {
    ApiError::validation("The given data was invalid.", errors)
}

fn add(errors: &mut Errors, field: &str, message: impl Into<String>) {
    errors
        .entry(field.to_string())
        .or_default()
        .push(message.into());
}

/// Teks query setelah `TrimStrings`: kosong dihitung tidak ada (`ConvertEmptyStringsToNull`).
fn text(query: &HashMap<String, String>, key: &str) -> Option<String> {
    query
        .get(key)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `tahun` yang dipakai untuk memfilter: `if ($tahun)` di PHP, jadi `"0"` juga dianggap tidak ada.
fn tahun_filter(query: &HashMap<String, String>) -> Option<String> {
    text(query, "tahun").filter(|t| t != "0")
}

/// Kondisi SQL dengan parameter `?` terurut.
#[derive(Clone, Default)]
struct Cond {
    sql: String,
    binds: Vec<String>,
}

impl Cond {
    fn new(sql: &str) -> Self {
        Self {
            sql: sql.to_string(),
            binds: Vec::new(),
        }
    }

    fn bind(sql: &str, binds: Vec<String>) -> Self {
        Self {
            sql: sql.to_string(),
            binds,
        }
    }
}

/// Gabungan semua kondisi dengan AND, masing-masing dalam kurung.
fn all(parts: Vec<Cond>) -> Cond {
    let sql = parts
        .iter()
        .map(|p| format!("({})", p.sql))
        .collect::<Vec<_>>()
        .join(" AND ");
    let binds = parts.into_iter().flat_map(|p| p.binds).collect();
    Cond { sql, binds }
}

/// `basePekerjaanQuery`: `notCanceled()` dan, bila ada, `whereHas('kegiatan', tahun_anggaran)`.
fn base(tahun: Option<&str>) -> Cond {
    let mut parts = vec![Cond::new("p.status IS NULL OR p.status <> 'canceled'")];
    if let Some(t) = tahun {
        parts.push(Cond::bind(
            "EXISTS (SELECT 1 FROM tbl_kegiatan g WHERE g.id = p.kegiatan_id AND g.tahun_anggaran = ?)",
            vec![t.to_string()],
        ));
    }
    all(parts)
}

/// Pekerjaan yang punya foto dengan koordinat. `koordinat` NOT NULL, jadi `whereNotNull` selalu benar.
const NO_COORDS: &str =
    "NOT EXISTS (SELECT 1 FROM tbl_foto f WHERE f.pekerjaan_id = p.id AND f.koordinat IS NOT NULL)";
const NO_PHOTOS: &str = "NOT EXISTS (SELECT 1 FROM tbl_foto f WHERE f.pekerjaan_id = p.id)";
/// `withKontrak()`: legacy `id_pekerjaan` atau pivot `kontrak_pekerjaan`.
const WITH_KONTRAK: &str = "(EXISTS (SELECT 1 FROM tbl_kontrak k WHERE k.id_pekerjaan = p.id) \
     OR EXISTS (SELECT 1 FROM kontrak_pekerjaan kp WHERE kp.pekerjaan_id = p.id))";
/// `withoutKontrak()`.
const WITHOUT_KONTRAK: &str =
    "(NOT EXISTS (SELECT 1 FROM tbl_kontrak k WHERE k.id_pekerjaan = p.id) \
     AND NOT EXISTS (SELECT 1 FROM kontrak_pekerjaan kp WHERE kp.pekerjaan_id = p.id))";

/// Kondisi tambahan untuk `issue`. Nilai sudah divalidasi terhadap `ISSUES`.
fn issue_cond(issue: &str) -> Cond {
    match issue {
        "no_coordinates" => Cond::new(NO_COORDS),
        "no_photos" => Cond::new(NO_PHOTOS),
        "started_no_photos" => all(vec![Cond::new(WITH_KONTRAK), Cond::new(NO_PHOTOS)]),
        _ => Cond::new(WITHOUT_KONTRAK),
    }
}

async fn count_pekerjaan(pool: &MySqlPool, cond: &Cond) -> Result<i64, ApiError> {
    let sql = format!(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_pekerjaan p WHERE {}",
        cond.sql
    );
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    for b in &cond.binds {
        q = q.bind(b);
    }
    q.fetch_one(pool).await.map_err(internal)
}

/// `getStats`: hitungan untuk kartu dashboard, sudah dengan filter tahun.
async fn stats_value(
    pool: &MySqlPool,
    tahun: Option<&str>,
) -> Result<Map<String, Value>, ApiError> {
    let b = base(tahun);
    let no_coordinates =
        count_pekerjaan(pool, &all(vec![b.clone(), issue_cond("no_coordinates")])).await?;
    let no_photos = count_pekerjaan(pool, &all(vec![b.clone(), issue_cond("no_photos")])).await?;
    let started_no_photos =
        count_pekerjaan(pool, &all(vec![b.clone(), issue_cond("started_no_photos")])).await?;
    let no_contracts =
        count_pekerjaan(pool, &all(vec![b.clone(), issue_cond("no_contracts")])).await?;
    let total_jobs = count_pekerjaan(pool, &b).await?;
    let mut m = Map::new();
    m.insert("no_coordinates".into(), json!(no_coordinates));
    m.insert("no_photos".into(), json!(no_photos));
    m.insert("started_no_photos".into(), json!(started_no_photos));
    m.insert("no_contracts".into(), json!(no_contracts));
    m.insert("total_jobs".into(), json!(total_jobs));
    Ok(m)
}

/// `GET /api/data-quality/stats`
pub async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let tahun = tahun_filter(&query);
    let data = stats_value(&state.pool, tahun.as_deref()).await?;
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// Input `getItems` yang sudah divalidasi.
struct ItemsInput {
    issue: &'static str,
    search: Option<String>,
    per_page: u64,
    page: u64,
}

/// Validasi `getItems`. Error dikumpulkan per field, dengan pesan standar `The given data was invalid.`.
fn validate_items(query: &HashMap<String, String>) -> Result<ItemsInput, ApiError> {
    let mut errors = Errors::new();

    let issue = match text(query, "issue") {
        None => {
            add(&mut errors, "issue", "The issue field is required.");
            ""
        }
        Some(v) => match ISSUES.iter().copied().find(|i| *i == v) {
            Some(i) => i,
            None => {
                add(&mut errors, "issue", "The selected issue is invalid.");
                ""
            }
        },
    };

    if let Some(t) = text(query, "tahun") {
        if t.parse::<i64>().is_err() {
            add(&mut errors, "tahun", "The tahun field must be an integer.");
        }
    }

    let search = text(query, "search");
    if let Some(s) = &search {
        if s.chars().count() > 200 {
            add(
                &mut errors,
                "search",
                "The search field must not be greater than 200 characters.",
            );
        }
    }

    let mut per_page = DEFAULT_PER_PAGE;
    if let Some(raw) = text(query, "per_page") {
        match raw.parse::<i64>() {
            Err(_) => add(
                &mut errors,
                "per_page",
                "The per page field must be an integer.",
            ),
            Ok(v) if v < 1 => add(
                &mut errors,
                "per_page",
                "The per page field must be at least 1.",
            ),
            Ok(v) if v > 100 => add(
                &mut errors,
                "per_page",
                "The per page field must not be greater than 100.",
            ),
            Ok(v) => per_page = v as u64,
        }
    }

    if !errors.is_empty() {
        return Err(invalid(errors));
    }

    // Paginator Laravel: `page` harus bilangan bulat >= 1, selain itu halaman 1.
    let page = text(query, "page")
        .and_then(|p| p.parse::<u64>().ok())
        .filter(|p| *p >= 1)
        .unwrap_or(1);

    Ok(ItemsInput {
        issue,
        search,
        per_page,
        page,
    })
}

/// `pagu` FLOAT (single precision): dibaca sebagai f32, lalu ditulis lewat teksnya seperti `float_json`.
fn float_json(v: f32) -> Value {
    let d: f64 = v.to_string().parse().unwrap_or(v as f64);
    serde_json::Number::from_f64(d)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// `GET /api/data-quality/items`: daftar kerja untuk satu isu, dengan paginasi Laravel.
pub async fn items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let input = validate_items(&query)?;

    let tahun = tahun_filter(&query);
    let mut cond = all(vec![base(tahun.as_deref()), issue_cond(input.issue)]);
    if let Some(s) = &input.search {
        if s != "0" {
            let like = format!("%{s}%");
            cond = all(vec![
                cond,
                Cond::bind(
                    "p.nama_paket LIKE ? OR p.kode_rekening LIKE ?",
                    vec![like.clone(), like],
                ),
            ]);
        }
    }

    let total = count_pekerjaan(&state.pool, &cond).await?;
    let last_page = (total as u64).div_ceil(input.per_page).max(1);
    let offset = (input.page - 1) * input.per_page;
    let sql = format!(
        "SELECT CAST(p.id AS SIGNED) AS id, p.kode_rekening, p.nama_paket, p.pagu, \
         k.n_kec, d.n_desa, g.nama AS pengawas_nama \
         FROM tbl_pekerjaan p \
         LEFT JOIN tbl_kecamatan k ON k.id = p.kecamatan_id \
         LEFT JOIN tbl_desa d ON d.id = p.desa_id \
         LEFT JOIN pengawas g ON g.id = p.pengawas_id \
         WHERE {} ORDER BY p.nama_paket, p.id LIMIT {} OFFSET {}",
        cond.sql, input.per_page, offset
    );
    let mut q = sqlx::query(&sql);
    for b in &cond.binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;

    let mut data = Vec::with_capacity(rows.len());
    for r in &rows {
        let id: i64 = r.try_get("id").map_err(internal)?;
        let href = match input.issue {
            "no_coordinates" => format!("/pekerjaan/{id}"),
            "no_photos" | "started_no_photos" => format!("/foto?pekerjaanId={id}"),
            _ => format!("/kontrak/new?pekerjaanId={id}"),
        };
        let pagu: f32 = r.try_get("pagu").map_err(internal)?;
        data.push(json!({
            "id": id,
            "nama_paket": r.try_get::<String, _>("nama_paket").map_err(internal)?,
            "kode_rekening": r.try_get::<Option<String>, _>("kode_rekening").map_err(internal)?,
            "pagu": float_json(pagu),
            "kecamatan": r.try_get::<Option<String>, _>("n_kec").map_err(internal)?,
            "desa": r.try_get::<Option<String>, _>("n_desa").map_err(internal)?,
            "pengawas": r.try_get::<Option<String>, _>("pengawas_nama").map_err(internal)?,
            "issue": input.issue,
            "href": href,
        }));
    }

    Ok(Json(json!({
        "success": true,
        "data": data,
        "meta": {
            "current_page": input.page,
            "last_page": last_page,
            "per_page": input.per_page,
            "total": total,
        },
    }))
    .into_response())
}

/// Satu item kotak masuk (`actions[]`).
struct Action {
    id: String,
    source: &'static str,
    title: String,
    detail: &'static str,
    severity: &'static str,
    count: i64,
    href: String,
}

fn rank(severity: &str) -> u8 {
    match severity {
        "high" => 0,
        "medium" => 1,
        "low" => 2,
        _ => 9,
    }
}

/// Hitungan tiket terbuka (`open`/`pending`) yang tidak terikat paket dibatalkan.
async fn count_tiket(pool: &MySqlPool, high_only: bool) -> Result<i64, ApiError> {
    let prioritas = if high_only {
        " AND t.prioritas = 'high'"
    } else {
        ""
    };
    let sql = format!(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_tiket t \
         WHERE t.status IN ('open', 'pending'){prioritas} \
         AND (t.pekerjaan_id IS NULL OR EXISTS (SELECT 1 FROM tbl_pekerjaan p \
             WHERE p.id = t.pekerjaan_id AND (p.status IS NULL OR p.status <> 'canceled')))"
    );
    sqlx::query_scalar::<_, i64>(&sql)
        .fetch_one(pool)
        .await
        .map_err(internal)
}

/// Kontrak yang selesai dalam 30 hari dan punya minimal satu paket aktif (legacy atau pivot).
async fn count_kontrak_ending(
    pool: &MySqlPool,
    tahun: Option<&str>,
    today: NaiveDate,
) -> Result<i64, ApiError> {
    let end = today.checked_add_days(Days::new(30)).unwrap_or(today);
    let mut binds = vec![
        today.format("%Y-%m-%d").to_string(),
        end.format("%Y-%m-%d").to_string(),
    ];
    let mut sql = String::from(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_kontrak k \
         WHERE k.tgl_selesai IS NOT NULL AND k.tgl_selesai >= ? AND k.tgl_selesai <= ? \
         AND (EXISTS (SELECT 1 FROM tbl_pekerjaan p WHERE p.id = k.id_pekerjaan \
              AND (p.status IS NULL OR p.status <> 'canceled')) \
           OR EXISTS (SELECT 1 FROM kontrak_pekerjaan kp JOIN tbl_pekerjaan p ON p.id = kp.pekerjaan_id \
              WHERE kp.kontrak_id = k.id AND (p.status IS NULL OR p.status <> 'canceled')))",
    );
    if let Some(t) = tahun {
        sql.push_str(
            " AND (EXISTS (SELECT 1 FROM tbl_pekerjaan p JOIN tbl_kegiatan g ON g.id = p.kegiatan_id \
               WHERE p.id = k.id_pekerjaan AND g.tahun_anggaran = ?) \
             OR EXISTS (SELECT 1 FROM kontrak_pekerjaan kp JOIN tbl_pekerjaan p ON p.id = kp.pekerjaan_id \
               JOIN tbl_kegiatan g ON g.id = p.kegiatan_id WHERE kp.kontrak_id = k.id AND g.tahun_anggaran = ?) \
             OR EXISTS (SELECT 1 FROM tbl_kegiatan g WHERE g.id = k.id_kegiatan AND g.tahun_anggaran = ?))",
        );
        binds.extend([t.to_string(), t.to_string(), t.to_string()]);
    }
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    q.fetch_one(pool).await.map_err(internal)
}

/// `GET /api/data-quality/action-inbox`: daftar tindak lanjut untuk operator, diurutkan high > medium > low.
pub async fn action_inbox(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;

    // `$request->query('tahun')` tanpa validasi. Nilai yang lolos `if ($tahun)` juga dipakai di href.
    let tahun = tahun_filter(&query);
    let pool = &state.pool;
    let now = Utc::now();
    let today = now.date_naive();

    let stats = stats_value(pool, tahun.as_deref()).await?;
    let count_of = |key: &str| stats.get(key).and_then(Value::as_i64).unwrap_or(0);
    let tahun_href = tahun
        .as_deref()
        .map(|t| format!("&tahun={t}"))
        .unwrap_or_default();

    let mut actions: Vec<Action> = Vec::new();
    for (key, label, severity, href) in [
        (
            "no_coordinates",
            "Tanpa koordinat",
            "high",
            "/data-quality?issue=no_coordinates",
        ),
        (
            "started_no_photos",
            "Berkontrak tanpa foto",
            "high",
            "/data-quality?issue=started_no_photos",
        ),
        (
            "no_photos",
            "Tanpa foto",
            "medium",
            "/data-quality?issue=no_photos",
        ),
        (
            "no_contracts",
            "Tanpa kontrak",
            "medium",
            "/data-quality?issue=no_contracts",
        ),
    ] {
        let count = count_of(key);
        if count > 0 {
            actions.push(Action {
                id: format!("dq-{key}"),
                source: "data_quality",
                title: format!("{count} pekerjaan {label}"),
                detail: "Perbaiki kelengkapan data pekerjaan (paket dibatalkan dikecualikan).",
                severity,
                count,
                href: format!("{href}{tahun_href}"),
            });
        }
    }

    let open_high = count_tiket(pool, true).await?;
    if open_high > 0 {
        actions.push(Action {
            id: "tiket-high".into(),
            source: "tiket",
            title: format!("{open_high} tiket prioritas tinggi terbuka"),
            detail: "Perlu penanganan / eskalasi segera.",
            severity: "high",
            count: open_high,
            href: "/tiket".into(),
        });
    }

    let open_all = count_tiket(pool, false).await?;
    if open_all > 0 {
        actions.push(Action {
            id: "tiket-open".into(),
            source: "tiket",
            title: format!("{open_all} tiket masih terbuka"),
            detail: "Termasuk pending dan open (paket dibatalkan dikecualikan).",
            severity: if open_all > 20 { "medium" } else { "low" },
            count: open_all,
            href: "/tiket".into(),
        });
    }

    let ending = count_kontrak_ending(pool, tahun.as_deref(), today).await?;
    if ending > 0 {
        actions.push(Action {
            id: "kontrak-h30".into(),
            source: "kontrak",
            title: format!("{ending} kontrak berakhir ≤ 30 hari"),
            detail:
                "Siapkan BA / addendum / perpanjangan bila perlu (paket dibatalkan dikecualikan).",
            severity: "high",
            count: ending,
            href: "/kontrak".into(),
        });
    }

    // `usort` di PHP 8 stabil, jadi urutan penambahan dipertahankan dalam satu severity.
    actions.sort_by_key(|a| rank(a.severity));
    let total_actions = actions.len();
    let actions: Vec<Value> = actions
        .into_iter()
        .map(|a| {
            json!({
                "id": a.id.to_string(),
                "source": a.source,
                "title": a.title,
                "detail": a.detail,
                "severity": a.severity,
                "count": a.count,
                "href": a.href,
            })
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "data": {
            "generated_at": iso8601_utc(Some(now)),
            "stats": stats,
            "actions": actions,
            "total_actions": total_actions,
            "excludes_canceled_pekerjaan": true,
        },
    }))
    .into_response())
}

// ---------------------------------------------------------------------------
// POST /api/client-error-reports
// ---------------------------------------------------------------------------

/// Nilai input setelah `TrimStrings` dan `ConvertEmptyStringsToNull` (berlaku rekursif di Laravel).
fn normalize_input(v: &Value) -> Value {
    match v {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Value::Null
            } else {
                Value::String(t.to_string())
            }
        }
        Value::Array(items) => Value::Array(items.iter().map(normalize_input).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), normalize_input(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Field teks untuk aturan `nullable|string`: `None` bila tidak ada atau null.
enum Field {
    Missing,
    Text(String),
    NotText,
}

fn field(input: &Map<String, Value>, key: &str) -> Field {
    match input.get(key) {
        None | Some(Value::Null) => Field::Missing,
        Some(Value::String(s)) => Field::Text(s.clone()),
        Some(_) => Field::NotText,
    }
}

/// Teks opsional dengan batas panjang (dihitung dalam karakter, seperti `mb_strlen`).
fn optional_text(
    errors: &mut Errors,
    input: &Map<String, Value>,
    key: &str,
    attribute: &str,
    max: Option<usize>,
) -> Option<String> {
    match field(input, key) {
        Field::Missing => None,
        Field::NotText => {
            add(
                errors,
                key,
                format!("The {attribute} field must be a string."),
            );
            None
        }
        Field::Text(s) => {
            if let Some(max) = max {
                if s.chars().count() > max {
                    add(
                        errors,
                        key,
                        format!("The {attribute} field must not be greater than {max} characters."),
                    );
                    return None;
                }
            }
            Some(s)
        }
    }
}

/// `metadata` sebagai JSON: `array`, jadi list atau objek. Hasil kosong menjadi NULL (`?: null`).
fn merge_metadata(metadata: Option<Value>, app: Option<&str>) -> Option<String> {
    let Some(app) = app else {
        // Tanpa `app`, metadata disimpan apa adanya (list tetap list). Kosong menjadi NULL.
        return match metadata {
            Some(Value::Array(items)) if !items.is_empty() => Some(Value::Array(items).to_string()),
            Some(Value::Object(map)) if !map.is_empty() => Some(Value::Object(map).to_string()),
            _ => None,
        };
    };
    // Dengan `app`, list berubah menjadi objek (`$metadata['app'] = ...` di PHP).
    let mut out: Map<String, Value> = match metadata {
        Some(Value::Object(map)) => map,
        Some(Value::Array(items)) => items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        _ => Map::new(),
    };
    out.insert("app".into(), Value::String(app.to_string()));
    Some(Value::Object(out).to_string())
}

/// `POST /api/client-error-reports`: menyimpan laporan error dari klien. Respons 201 `{success: true}`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;

    // Body yang bukan objek JSON diperlakukan sebagai input kosong, seperti `$request->validate` di Laravel.
    let raw: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let normalized = normalize_input(&raw);
    let input = normalized.as_object().cloned().unwrap_or_default();

    let mut errors = Errors::new();

    let source = match field(&input, "source") {
        Field::Missing => {
            add(&mut errors, "source", "The source field is required.");
            None
        }
        Field::NotText => {
            add(&mut errors, "source", "The source field must be a string.");
            None
        }
        Field::Text(s) => {
            if SOURCES.contains(&s.as_str()) {
                Some(s)
            } else {
                add(&mut errors, "source", "The selected source is invalid.");
                None
            }
        }
    };

    let message = match field(&input, "message") {
        Field::Missing => {
            add(&mut errors, "message", "The message field is required.");
            None
        }
        Field::NotText => {
            add(
                &mut errors,
                "message",
                "The message field must be a string.",
            );
            None
        }
        Field::Text(s) => {
            if s.chars().count() > 5000 {
                add(
                    &mut errors,
                    "message",
                    "The message field must not be greater than 5000 characters.",
                );
                None
            } else {
                Some(s)
            }
        }
    };

    let stack = optional_text(&mut errors, &input, "stack", "stack", None);
    let component_stack = optional_text(
        &mut errors,
        &input,
        "component_stack",
        "component stack",
        None,
    );
    let url = optional_text(&mut errors, &input, "url", "url", Some(5000));
    let user_agent = optional_text(&mut errors, &input, "user_agent", "user agent", Some(2000));
    let app = optional_text(&mut errors, &input, "app", "app", Some(64));

    // `nullable|array`: objek dan list lolos, selain itu 422.
    let metadata = match input.get("metadata") {
        None | Some(Value::Null) => None,
        Some(v @ (Value::Array(_) | Value::Object(_))) => Some(v.clone()),
        Some(_) => {
            add(
                &mut errors,
                "metadata",
                "The metadata field must be an array.",
            );
            None
        }
    };

    if !errors.is_empty() {
        return Err(invalid(errors));
    }

    let (Some(source), Some(message)) = (source, message) else {
        return Err(invalid(errors));
    };
    let metadata = merge_metadata(metadata, app.as_deref());
    let (ip, _) = audit::client_info(&headers);

    sqlx::query(
        "INSERT INTO error_logs (user_id, source, message, stack, component_stack, url, user_agent, \
         ip_address, metadata, resolved_at, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, NOW(), NOW())",
    )
    .bind(user.user_id)
    .bind(source)
    .bind(message)
    .bind(stack)
    .bind(component_stack)
    .bind(url)
    .bind(user_agent)
    .bind(ip)
    .bind(metadata)
    .execute(&state.pool)
    .await
    .map_err(internal)?;

    Ok((StatusCode::CREATED, Json(json!({ "success": true }))).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_list_stays_list_without_app() {
        assert_eq!(
            merge_metadata(Some(json!([1, 2])), None),
            Some("[1,2]".to_string())
        );
    }

    #[test]
    fn metadata_empty_becomes_null() {
        assert_eq!(merge_metadata(Some(json!({})), None), None);
        assert_eq!(merge_metadata(Some(json!([])), None), None);
        assert_eq!(merge_metadata(None, None), None);
    }

    #[test]
    fn metadata_with_app_is_object_and_list_becomes_keyed() {
        let v: Value =
            serde_json::from_str(&merge_metadata(Some(json!({"k": 1})), Some("arumanis")).unwrap())
                .unwrap();
        assert_eq!(v, json!({"k": 1, "app": "arumanis"}));
        let v: Value =
            serde_json::from_str(&merge_metadata(Some(json!(["a"])), Some("arumanis")).unwrap())
                .unwrap();
        assert_eq!(v, json!({"0": "a", "app": "arumanis"}));
        let v: Value =
            serde_json::from_str(&merge_metadata(None, Some("arumanis")).unwrap()).unwrap();
        assert_eq!(v, json!({"app": "arumanis"}));
    }

    #[test]
    fn query_helpers_follow_php_truthiness() {
        let mut q = HashMap::new();
        q.insert("tahun".to_string(), " 0 ".to_string());
        assert_eq!(tahun_filter(&q), None);
        q.insert("tahun".to_string(), "2026".to_string());
        assert_eq!(tahun_filter(&q).as_deref(), Some("2026"));
        q.insert("tahun".to_string(), "   ".to_string());
        assert_eq!(text(&q, "tahun"), None);
    }
}
