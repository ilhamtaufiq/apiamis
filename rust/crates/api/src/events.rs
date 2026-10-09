//! `/api/events`: port `EventController` dan `EventResource`.
//!
//! `index` menggabungkan event manual milik user dengan event otomatis (kontrak, register dokumen,
//! berkas). Event otomatis hanya untuk pekerjaan yang lolos `scopeByUserRole`. Event manual memakai
//! `Auditable` di Laravel: setiap create, update, dan delete menulis baris audit. Tidak ada notifikasi
//! admin dan tidak ada invalidasi cache. Upload lampiran memakai media ala Spatie (`media::attach`).
//!
//! Deviasi yang dicatat:
//! - Input string di-trim dan string kosong menjadi null (`TrimStrings`, `ConvertEmptyStringsToNull`).
//! - Aturan `boolean` memakai nilai dokumentasi Laravel: true/false, 1/0, "1"/"0". `"true"` ditolak.
//! - `after_or_equal:start` di update membandingkan dengan `start` dari request. Bila `end` dikirim
//!   tanpa `start`, validasi gagal. Itu diikuti di sini, sesuai Laravel.
//! - Respon `store` menampilkan `category` dari request (null bila tidak dikirim), karena Eloquent
//!   tidak membaca ulang default DB. GET berikutnya menampilkan `event`.
//! - Audit memakai nilai JSON yang disederhanakan, bukan nilai mentah maupun cast Laravel. Pada
//!   `created`, atribut yang tidak dikirim tetap dicatat dengan nilai default.
//! - Mime lampiran ditebak dari ekstensi (`media::mime_for_name`), bukan dari isi berkas.
//! - Parser tanggal hanya mengenal format ISO umum. `strtotime` PHP lebih longgar.
//! - Destroy juga menghapus media milik event (perilaku Spatie `InteractsWithMedia`).

use axum::{
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySql, MySqlPool, Row, Transaction};

use crate::{
    access, changes, foto, kanban::{body_object, text_rule, Errs},
    lookup::carbon_json, media, require_auth, AppState,
};

const EVENT_MODEL: &str = "App\\Models\\Event";
const ATTACHMENTS: &str = "event/attachments";
/// `max:10240` (KB) pada aturan `file`.
const MAX_UPLOAD_BYTES: usize = 10_240 * 1024;
/// Batas body untuk upload: berkas maksimal ditambah overhead multipart.
pub const BODY_LIMIT: usize = MAX_UPLOAD_BYTES + 1024 * 1024;
const CATEGORIES: &[&str] = &["event", "task", "milestone", "holiday"];
/// Default kolom `category` di tabel (`->default('event')`).
const DEFAULT_CATEGORY: &str = "event";
/// Kolom yang bisa diubah, dipakai untuk audit dan penulisan.
const EDITABLE: &[&str] = &[
    "title",
    "is_allday",
    "start",
    "end",
    "category",
    "location",
    "description",
    "color",
    "bg_color",
    "border_color",
    "attachments",
];

const SELECT_EVENT: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(user_id AS SIGNED) AS user_id, \
     title, is_allday, start, end, category, location, description, color, bg_color, border_color, \
     CAST(attachments AS CHAR) AS attachments, created_at, updated_at FROM tbl_events";

fn internal(e: impl std::fmt::Display) -> ApiError {
    media::internal(e)
}

/// Respon 422 dengan pesan `The given data was invalid.`, seperti `$request->validate()`.
fn invalid(errs: Errs) -> ApiError {
    ApiError::validation("The given data was invalid.", errs)
}

fn attr(key: &str) -> String {
    key.replace('_', " ")
}

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse().map_err(|_| ApiError::not_found())
}

/// Zona WIB (`Asia/Jakarta`). Tidak ada DST sejak 1964, jadi offset tetap.
fn wib() -> FixedOffset {
    FixedOffset::east_opt(7 * 3600).expect("offset WIB valid")
}

/// `abort(403)` tanpa pesan. Laravel menjawab `Forbidden`.
fn forbidden() -> ApiError {
    ApiError::new(StatusCode::FORBIDDEN, "Forbidden")
}

fn data(v: Value) -> Response {
    Json(json!({ "data": v })).into_response()
}

fn base_url(state: &AppState) -> String {
    state.app_url.trim_end_matches('/').to_string()
}

// ---------------------------------------------------------------------------
// Baris dan resource
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct EventRow {
    id: i64,
    user_id: i64,
    title: String,
    is_allday: bool,
    /// Jam dinding WIB seperti yang tersimpan di kolom `start`/`end`.
    start: NaiveDateTime,
    end: NaiveDateTime,
    category: Option<String>,
    location: Option<String>,
    description: Option<String>,
    color: Option<String>,
    bg_color: Option<String>,
    border_color: Option<String>,
    attachments: Option<Value>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

fn map_event(r: &MySqlRow) -> Result<EventRow, sqlx::Error> {
    let attachments: Option<String> = r.try_get("attachments")?;
    Ok(EventRow {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        title: r.try_get("title")?,
        is_allday: r.try_get("is_allday")?,
        start: r.try_get("start")?,
        end: r.try_get("end")?,
        category: r.try_get("category")?,
        location: r.try_get("location")?,
        description: r.try_get("description")?,
        color: r.try_get("color")?,
        bg_color: r.try_get("bg_color")?,
        border_color: r.try_get("border_color")?,
        attachments: attachments.and_then(|s| serde_json::from_str(&s).ok()),
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn find_event<'e, E>(exec: E, id: i64) -> Result<Option<EventRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_EVENT} WHERE id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_event).transpose()
}

/// `wibToIso`: kolom disimpan sebagai jam dinding WIB, dibaca ulang sebagai WIB, lalu jadi ISO UTC.
fn wib_iso(wall: NaiveDateTime) -> Value {
    let utc = Utc.from_utc_datetime(&(wall - Duration::hours(7)));
    carbon_json(Some(utc))
}

/// `EventResource::toArray`.
fn resource(e: &EventRow) -> Value {
    json!({
        "id": e.id,
        "user_id": e.user_id,
        "title": e.title,
        "isAllday": e.is_allday,
        "start": wib_iso(e.start),
        "end": wib_iso(e.end),
        "category": e.category,
        "location": e.location,
        "description": e.description,
        "color": e.color,
        "backgroundColor": e.bg_color,
        "borderColor": e.border_color,
        "attachments": e.attachments.clone().unwrap_or(Value::Null),
        "created_at": carbon_json(e.created_at),
        "updated_at": carbon_json(e.updated_at),
    })
}

/// Nilai satu kolom untuk audit dan perbandingan perubahan.
fn field_json(e: &EventRow, key: &str) -> Value {
    match key {
        "title" => json!(e.title),
        "is_allday" => json!(e.is_allday),
        "start" => json!(e.start.format("%Y-%m-%d %H:%M:%S").to_string()),
        "end" => json!(e.end.format("%Y-%m-%d %H:%M:%S").to_string()),
        "category" => json!(e.category),
        "location" => json!(e.location),
        "description" => json!(e.description),
        "color" => json!(e.color),
        "bg_color" => json!(e.bg_color),
        "border_color" => json!(e.border_color),
        "attachments" => e.attachments.clone().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// `getAttributes()` untuk audit `created` dan `deleted`.
fn attrs(e: &EventRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(e.id));
    m.insert("user_id".into(), json!(e.user_id));
    for key in EDITABLE {
        m.insert((*key).into(), field_json(e, key));
    }
    m.insert("created_at".into(), carbon_json(e.created_at));
    m.insert("updated_at".into(), carbon_json(e.updated_at));
    m
}

// ---------------------------------------------------------------------------
// Validasi
// ---------------------------------------------------------------------------

/// Tanggal dari input. `wall` disimpan sebagai WIB. `cmp` dipakai untuk `after_or_equal`,
/// yang di Laravel membandingkan dengan `strtotime` (zona default UTC untuk teks tanpa offset).
#[derive(Debug, Clone, Copy)]
struct Moment {
    wall: NaiveDateTime,
    cmp: DateTime<Utc>,
}

fn parse_moment(raw: &str) -> Option<Moment> {
    let s = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(Moment {
            wall: dt.with_timezone(&wib()).naive_local(),
            cmp: dt.with_timezone(&Utc),
        });
    }
    const NAIVE: [&str; 4] = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ];
    let naive = NAIVE
        .iter()
        .find_map(|f| NaiveDateTime::parse_from_str(s, *f).ok())
        .or_else(|| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
        })?;
    Some(Moment {
        wall: naive,
        cmp: naive.and_utc(),
    })
}

/// Aturan `boolean` Laravel: true, false, 1, 0, "1", "0".
fn laravel_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_i64() {
            Some(1) => Some(true),
            Some(0) => Some(false),
            _ => None,
        },
        Value::String(s) => match s.as_str() {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// `required|date` untuk `start` dan `end`. Di `store` absen berarti wajib. Di `update` absen dilewati.
fn moment_field(errs: &mut Errs, obj: &Map<String, Value>, key: &str, store: bool) -> Option<Moment> {
    match obj.get(key) {
        None => {
            if store {
                foto::add(errs, key, format!("The {} field is required.", attr(key)));
            }
            None
        }
        Some(Value::Null) => {
            foto::add(errs, key, format!("The {} field is required.", attr(key)));
            None
        }
        Some(v) => {
            let parsed = v.as_str().and_then(parse_moment);
            if parsed.is_none() {
                foto::add(
                    errs,
                    key,
                    format!("The {} field must be a valid date.", attr(key)),
                );
            }
            parsed
        }
    }
}

/// `nullable|...`: absen tidak mengubah (`None`), null mengosongkan (`Some(None)`).
fn nullable_field(
    errs: &mut Errs,
    obj: &Map<String, Value>,
    key: &str,
    max: usize,
) -> Option<Option<String>> {
    match obj.get(key) {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => Some(text_rule(errs, key, v, max)),
    }
}

/// Field yang lolos validasi. `None` berarti tidak dikirim.
#[derive(Debug, Default)]
struct Patch {
    title: Option<String>,
    is_allday: Option<bool>,
    start: Option<Moment>,
    end: Option<Moment>,
    category: Option<String>,
    location: Option<Option<String>>,
    description: Option<Option<String>>,
    color: Option<Option<String>>,
    bg_color: Option<Option<String>>,
    border_color: Option<Option<String>>,
    attachments: Option<Option<Value>>,
}

/// Aturan `store` (`store = true`) atau `update` (`sometimes|...`).
fn parse_patch(obj: &Map<String, Value>, store: bool) -> Result<Patch, ApiError> {
    let mut errs = Errs::new();
    let mut p = Patch::default();

    // title: required|string|max:255 (store), sometimes|required|string|max:255 (update).
    match obj.get("title") {
        None => {
            if store {
                foto::add(&mut errs, "title", "The title field is required.".into());
            }
        }
        Some(Value::Null) => {
            foto::add(&mut errs, "title", "The title field is required.".into());
        }
        Some(v) => p.title = text_rule(&mut errs, "title", v, 255),
    }

    if let Some(v) = obj.get("is_allday") {
        match laravel_bool(v) {
            Some(b) => p.is_allday = Some(b),
            None => foto::add(
                &mut errs,
                "is_allday",
                "The is allday field must be true or false.".into(),
            ),
        }
    }

    p.start = moment_field(&mut errs, obj, "start", store);
    p.end = moment_field(&mut errs, obj, "end", store);
    // after_or_equal:start. Pembanding adalah `start` dari request, bukan dari database.
    if let Some(end) = p.end {
        let ok = match p.start {
            Some(start) => end.cmp >= start.cmp,
            None => false,
        };
        if !ok {
            foto::add(
                &mut errs,
                "end",
                "The end field must be a date after or equal to start.".into(),
            );
        }
    }

    match obj.get("category") {
        None => {}
        Some(Value::String(s)) => {
            if CATEGORIES.contains(&s.as_str()) {
                p.category = Some(s.clone());
            } else {
                foto::add(
                    &mut errs,
                    "category",
                    "The selected category is invalid.".into(),
                );
            }
        }
        Some(_) => foto::add(
            &mut errs,
            "category",
            "The category field must be a string.".into(),
        ),
    }

    p.location = nullable_field(&mut errs, obj, "location", 255);
    p.description = nullable_field(&mut errs, obj, "description", usize::MAX);
    p.color = nullable_field(&mut errs, obj, "color", 20);
    p.bg_color = nullable_field(&mut errs, obj, "bg_color", 20);
    p.border_color = nullable_field(&mut errs, obj, "border_color", 20);

    p.attachments = match obj.get("attachments") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v @ (Value::Array(_) | Value::Object(_))) => Some(Some(v.clone())),
        Some(_) => {
            foto::add(
                &mut errs,
                "attachments",
                "The attachments field must be an array.".into(),
            );
            None
        }
    };

    if errs.is_empty() {
        Ok(p)
    } else {
        Err(invalid(errs))
    }
}

/// Terapkan field yang dikirim ke baris sekarang.
fn apply_patch(current: &EventRow, p: &Patch) -> EventRow {
    let mut next = current.clone();
    if let Some(v) = &p.title {
        next.title = v.clone();
    }
    if let Some(v) = p.is_allday {
        next.is_allday = v;
    }
    if let Some(m) = p.start {
        next.start = m.wall;
    }
    if let Some(m) = p.end {
        next.end = m.wall;
    }
    if let Some(v) = &p.category {
        next.category = Some(v.clone());
    }
    if let Some(v) = &p.location {
        next.location = v.clone();
    }
    if let Some(v) = &p.description {
        next.description = v.clone();
    }
    if let Some(v) = &p.color {
        next.color = v.clone();
    }
    if let Some(v) = &p.bg_color {
        next.bg_color = v.clone();
    }
    if let Some(v) = &p.border_color {
        next.border_color = v.clone();
    }
    if let Some(v) = &p.attachments {
        next.attachments = v.clone();
    }
    next
}

// ---------------------------------------------------------------------------
// Tulis dan audit
// ---------------------------------------------------------------------------

/// Tulis ulang kolom yang bisa diubah dan `updated_at` (setara `save()` dengan kolom dirty).
async fn save_event(tx: &mut Transaction<'_, MySql>, e: &EventRow) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE tbl_events SET title = ?, is_allday = ?, start = ?, end = ?, category = ?, location = ?, \
         description = ?, color = ?, bg_color = ?, border_color = ?, attachments = ?, updated_at = NOW() \
         WHERE id = ?",
    )
    .bind(e.title.clone())
    .bind(e.is_allday)
    .bind(e.start)
    .bind(e.end)
    .bind(e.category.clone().unwrap_or_else(|| DEFAULT_CATEGORY.to_string()))
    .bind(e.location.clone())
    .bind(e.description.clone())
    .bind(e.color.clone())
    .bind(e.bg_color.clone())
    .bind(e.border_color.clone())
    .bind(e.attachments.as_ref().map(|v| v.to_string()))
    .bind(e.id)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

/// Audit `updated`: hanya kolom yang berbeda, plus `updated_at`.
async fn audit_updated(
    tx: &mut Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    url: &str,
    before: &EventRow,
    after: &EventRow,
) -> Result<(), ApiError> {
    let mut old = Map::new();
    let mut new = Map::new();
    for key in EDITABLE {
        let (a, b) = (field_json(before, key), field_json(after, key));
        if a != b {
            old.insert((*key).into(), a);
            new.insert((*key).into(), b);
        }
    }
    old.insert("updated_at".into(), carbon_json(before.updated_at));
    new.insert("updated_at".into(), carbon_json(after.updated_at));
    changes::audit_only(
        tx,
        headers,
        actor,
        EVENT_MODEL,
        "updated",
        after.id,
        Some(old),
        Some(new),
        url,
    )
    .await
}

// ---------------------------------------------------------------------------
// Event otomatis (`automaticEvents`)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Style {
    category: &'static str,
    color: &'static str,
    bg: &'static str,
    border: &'static str,
}

const KONTRAK_STYLE: Style = Style {
    category: "milestone",
    color: "#0369a1",
    bg: "#e0f2fe",
    border: "#0284c7",
};
const REGISTER_STYLE: Style = Style {
    category: "milestone",
    color: "#7c2d12",
    bg: "#ffedd5",
    border: "#fb923c",
};
const BERKAS_STYLE: Style = Style {
    category: "task",
    color: "#166534",
    bg: "#dcfce7",
    border: "#22c55e",
};

/// Teks yang tidak kosong dan bukan "0" (truthy di PHP).
fn nonempty(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|v| !v.is_empty() && *v != "0")
}

/// `formatPekerjaanLocation`: "desa, kecamatan", atau null bila kosong.
fn location_of(desa: &Option<String>, kecamatan: &Option<String>) -> Option<String> {
    let parts: Vec<&str> = [nonempty(desa), nonempty(kecamatan)]
        .into_iter()
        .flatten()
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Baris deskripsi yang kosong dibuang, lalu digabung dengan "\n".
fn description_of(lines: Vec<Option<String>>) -> String {
    lines.into_iter().flatten().collect::<Vec<_>>().join("\n")
}

/// `makeAutomaticEvent`: satu hari penuh WIB, dinyatakan dalam UTC.
fn auto_event(
    user_id: u64,
    id: String,
    title: String,
    day: NaiveDate,
    style: Style,
    location: Option<String>,
    description: String,
) -> Value {
    let start_wall = day.and_hms_opt(0, 0, 0).expect("tengah malam valid");
    let start = Utc.from_utc_datetime(&(start_wall - Duration::hours(7)));
    let end = start + Duration::days(1) - Duration::microseconds(1);
    json!({
        "id": id,
        "user_id": user_id,
        "title": title,
        "isAllday": true,
        "start": carbon_json(Some(start)),
        "end": carbon_json(Some(end)),
        "category": style.category,
        "location": location,
        "description": description,
        "color": style.color,
        "backgroundColor": style.bg,
        "borderColor": style.border,
        "attachments": [],
        "created_at": null,
        "updated_at": null,
        "isAutomatic": true,
    })
}

/// Query dengan klausa `scopeByUserRole` (alias `p` untuk `tbl_pekerjaan`).
async fn fetch_scoped(
    pool: &MySqlPool,
    sql: &str,
    scope: &access::Restriction,
) -> Result<Vec<MySqlRow>, ApiError> {
    let mut q = sqlx::query(sql);
    for b in &scope.binds {
        q = q.bind(*b);
    }
    q.fetch_all(pool).await.map_err(internal)
}

async fn kontrak_events(
    pool: &MySqlPool,
    actor: u64,
    scope: &access::Restriction,
) -> Result<Vec<Value>, ApiError> {
    let sql = format!(
        "SELECT CAST(k.id AS SIGNED) AS id, k.tanggal_penawaran, k.tgl_sppbj, k.tgl_spk, k.tgl_spmk, \
         k.tgl_selesai, k.spk, k.spmk, k.sppbj, p.nama_paket, kc.n_kec, d.n_desa, py.nama AS penyedia_nama \
         FROM tbl_kontrak k \
         JOIN tbl_pekerjaan p ON p.id = k.id_pekerjaan \
         LEFT JOIN tbl_kecamatan kc ON kc.id = p.kecamatan_id \
         LEFT JOIN tbl_desa d ON d.id = p.desa_id \
         LEFT JOIN tbl_penyedia py ON py.id = k.id_penyedia \
         WHERE 1 = 1{} ORDER BY k.id",
        scope.sql
    );
    let rows = fetch_scoped(pool, &sql, scope).await?;

    let mut out = Vec::new();
    for r in &rows {
        let id: i64 = r.try_get("id").map_err(internal)?;
        let nama: Option<String> = r.try_get("nama_paket").map_err(internal)?;
        let kec: Option<String> = r.try_get("n_kec").map_err(internal)?;
        let desa: Option<String> = r.try_get("n_desa").map_err(internal)?;
        let spk: Option<String> = r.try_get("spk").map_err(internal)?;
        let spmk: Option<String> = r.try_get("spmk").map_err(internal)?;
        let sppbj: Option<String> = r.try_get("sppbj").map_err(internal)?;
        let penyedia: Option<String> = r.try_get("penyedia_nama").map_err(internal)?;
        let fields: [(&str, &str, Option<NaiveDate>); 5] = [
            (
                "tanggal_penawaran",
                "Tanggal Penawaran",
                r.try_get("tanggal_penawaran").map_err(internal)?,
            ),
            ("tgl_sppbj", "Tanggal SPPBJ", r.try_get("tgl_sppbj").map_err(internal)?),
            ("tgl_spk", "Tanggal SPK", r.try_get("tgl_spk").map_err(internal)?),
            ("tgl_spmk", "Tanggal SPMK", r.try_get("tgl_spmk").map_err(internal)?),
            (
                "tgl_selesai",
                "Tanggal Selesai Kontrak",
                r.try_get("tgl_selesai").map_err(internal)?,
            ),
        ];
        for (field, label, date) in fields {
            let Some(day) = date else {
                continue;
            };
            let title = format!(
                "{label}: {}",
                nama.clone().unwrap_or_else(|| format!("Kontrak #{id}"))
            );
            let lines = vec![
                Some("Sumber: Kontrak".to_string()),
                nonempty(&spk).map(|s| format!("No. SPK: {s}")),
                nonempty(&spmk).map(|s| format!("No. SPMK: {s}")),
                nonempty(&sppbj).map(|s| format!("No. SPPBJ: {s}")),
                nonempty(&penyedia).map(|s| format!("Penyedia: {s}")),
            ];
            out.push(auto_event(
                actor,
                format!("auto:kontrak:{id}:{field}"),
                title,
                day,
                KONTRAK_STYLE,
                location_of(&desa, &kec),
                description_of(lines),
            ));
        }
    }
    Ok(out)
}

async fn register_events(
    pool: &MySqlPool,
    actor: u64,
    scope: &access::Restriction,
) -> Result<Vec<Value>, ApiError> {
    let sql = format!(
        "SELECT CAST(dr.id AS SIGNED) AS id, dr.tanggal, dr.nomor, dr.description, dt.name AS type_name, \
         p.nama_paket, kc.n_kec, d.n_desa \
         FROM tbl_document_registers dr \
         JOIN tbl_kontrak k ON k.id = dr.kontrak_id \
         JOIN tbl_pekerjaan p ON p.id = k.id_pekerjaan \
         LEFT JOIN tbl_document_types dt ON dt.id = dr.type_id \
         LEFT JOIN tbl_kecamatan kc ON kc.id = p.kecamatan_id \
         LEFT JOIN tbl_desa d ON d.id = p.desa_id \
         WHERE dr.tanggal IS NOT NULL{} ORDER BY dr.id",
        scope.sql
    );
    let rows = fetch_scoped(pool, &sql, scope).await?;

    let mut out = Vec::new();
    for r in &rows {
        let id: i64 = r.try_get("id").map_err(internal)?;
        let tanggal: Option<NaiveDate> = r.try_get("tanggal").map_err(internal)?;
        let nomor: Option<String> = r.try_get("nomor").map_err(internal)?;
        let keterangan: Option<String> = r.try_get("description").map_err(internal)?;
        let type_name: Option<String> = r.try_get("type_name").map_err(internal)?;
        let nama: Option<String> = r.try_get("nama_paket").map_err(internal)?;
        let kec: Option<String> = r.try_get("n_kec").map_err(internal)?;
        let desa: Option<String> = r.try_get("n_desa").map_err(internal)?;
        let Some(day) = tanggal else {
            continue;
        };
        let title = format!(
            "Surat/Dokumen: {}",
            nonempty(&type_name).unwrap_or("Register Dokumen")
        );
        let lines = vec![
            Some("Sumber: Register surat/dokumen".to_string()),
            nonempty(&nomor).map(|s| format!("Nomor: {s}")),
            nonempty(&keterangan).map(|s| format!("Keterangan: {s}")),
            nonempty(&nama).map(|s| format!("Pekerjaan: {s}")),
        ];
        out.push(auto_event(
            actor,
            format!("auto:document-register:{id}"),
            title,
            day,
            REGISTER_STYLE,
            location_of(&desa, &kec),
            description_of(lines),
        ));
    }
    Ok(out)
}

async fn berkas_events(
    pool: &MySqlPool,
    actor: u64,
    scope: &access::Restriction,
) -> Result<Vec<Value>, ApiError> {
    let sql = format!(
        "SELECT CAST(b.id AS SIGNED) AS id, b.jenis_dokumen, b.created_at, p.nama_paket, kc.n_kec, d.n_desa \
         FROM tbl_berkas b \
         JOIN tbl_pekerjaan p ON p.id = b.pekerjaan_id \
         LEFT JOIN tbl_kecamatan kc ON kc.id = p.kecamatan_id \
         LEFT JOIN tbl_desa d ON d.id = p.desa_id \
         WHERE b.created_at IS NOT NULL{} ORDER BY b.id",
        scope.sql
    );
    let rows = fetch_scoped(pool, &sql, scope).await?;

    let mut out = Vec::new();
    for r in &rows {
        let id: i64 = r.try_get("id").map_err(internal)?;
        let jenis: Option<String> = r.try_get("jenis_dokumen").map_err(internal)?;
        let created: Option<DateTime<Utc>> = r.try_get("created_at").map_err(internal)?;
        let nama: Option<String> = r.try_get("nama_paket").map_err(internal)?;
        let kec: Option<String> = r.try_get("n_kec").map_err(internal)?;
        let desa: Option<String> = r.try_get("n_desa").map_err(internal)?;
        let Some(created) = created else {
            continue;
        };
        // Tanggal lokal WIB dari instant UTC.
        let day = created.with_timezone(&wib()).date_naive();
        let title = format!(
            "Berkas diunggah: {}",
            jenis.clone().unwrap_or_else(|| "Dokumen".to_string())
        );
        let lines = vec![
            Some("Sumber: Berkas pekerjaan".to_string()),
            nonempty(&nama).map(|s| format!("Pekerjaan: {s}")),
            nonempty(&jenis).map(|s| format!("Jenis dokumen: {s}")),
        ];
        out.push(auto_event(
            actor,
            format!("auto:berkas:{id}"),
            title,
            day,
            BERKAS_STYLE,
            location_of(&desa, &kec),
            description_of(lines),
        ));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/events`: event manual milik user, lalu event otomatis. Tanpa paginasi.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let sql = format!("{SELECT_EVENT} WHERE user_id = ? ORDER BY id");
    let rows = sqlx::query(&sql)
        .bind(user.user_id as i64)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let mut data_items: Vec<Value> = Vec::with_capacity(rows.len());
    for r in &rows {
        let e = map_event(r).map_err(internal)?;
        data_items.push(resource(&e));
    }

    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let scope = access::restriction(user.user_id, &roles, "p");
    data_items.extend(kontrak_events(&state.pool, user.user_id, &scope).await?);
    data_items.extend(register_events(&state.pool, user.user_id, &scope).await?);
    data_items.extend(berkas_events(&state.pool, user.user_id, &scope).await?);

    Ok(Json(json!({ "data": data_items })).into_response())
}

/// `POST /api/events`. Respon memakai nilai dari request untuk `category` (lihat catatan modul).
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let obj = body_object(&body);
    let patch = parse_patch(&obj, true)?;
    let (Some(title), Some(start), Some(end)) = (patch.title.clone(), patch.start, patch.end) else {
        return Err(internal("validasi event store tidak lengkap"));
    };
    let url = format!("{}/api/events", base_url(&state));

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let category_db = patch
        .category
        .clone()
        .unwrap_or_else(|| DEFAULT_CATEGORY.to_string());
    let res = sqlx::query(
        "INSERT INTO tbl_events (user_id, title, is_allday, start, end, category, location, description, \
         color, bg_color, border_color, attachments, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(user.user_id as i64)
    .bind(title)
    .bind(patch.is_allday.unwrap_or(false))
    .bind(start.wall)
    .bind(end.wall)
    .bind(category_db)
    .bind(patch.location.clone().flatten())
    .bind(patch.description.clone().flatten())
    .bind(patch.color.clone().flatten())
    .bind(patch.bg_color.clone().flatten())
    .bind(patch.border_color.clone().flatten())
    .bind(patch.attachments.clone().flatten().map(|v| v.to_string()))
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;

    let mut row = find_event(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("event baru tidak terbaca"))?;
    // Eloquent tidak membaca ulang default DB: `category` ikut dari request.
    row.category = patch.category.clone();

    changes::audit_only(
        &mut tx,
        &headers,
        user.user_id,
        EVENT_MODEL,
        "created",
        id,
        None,
        Some(attrs(&row)),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    Ok(data(resource(&row)))
}

/// `GET /api/events/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = find_event(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    if row.user_id != user.user_id as i64 {
        return Err(forbidden());
    }
    Ok(data(resource(&row)))
}

/// `PUT` dan `PATCH /api/events/{id}`. Bila tidak ada kolom yang berbeda, tidak ada tulis dan tidak ada audit.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let current = find_event(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    if current.user_id != user.user_id as i64 {
        return Err(forbidden());
    }

    let obj = body_object(&body);
    let patch = parse_patch(&obj, false)?;
    let next = apply_patch(&current, &patch);
    if next == current {
        return Ok(data(resource(&current)));
    }

    let url = format!("{}/api/events/{id}", base_url(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    save_event(&mut tx, &next).await?;
    let after = find_event(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("event hilang saat update"))?;
    audit_updated(&mut tx, &headers, user.user_id, &url, &current, &after).await?;
    tx.commit().await.map_err(internal)?;

    Ok(data(resource(&after)))
}

/// `DELETE /api/events/{id}`. Hard delete, audit `deleted`, dan media milik event ikut dihapus.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let row = find_event(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    if row.user_id != user.user_id as i64 {
        return Err(forbidden());
    }
    let url = format!("{}/api/events/{id}", base_url(&state));

    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_events WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    // Spatie `InteractsWithMedia` menghapus semua media model saat model dihapus.
    let media_ids: Vec<u64> = sqlx::query_scalar(
        "SELECT id FROM media WHERE model_type = ? AND model_id = ? ORDER BY id",
    )
    .bind(EVENT_MODEL)
    .bind(id as u64)
    .fetch_all(&mut *tx)
    .await
    .map_err(internal)?;
    let mut dirs = Vec::new();
    for mid in media_ids {
        sqlx::query("DELETE FROM media WHERE id = ?")
            .bind(mid)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        dirs.push(media::media_dir(mid));
    }
    changes::audit_only(
        &mut tx,
        &headers,
        user.user_id,
        EVENT_MODEL,
        "deleted",
        id,
        Some(attrs(&row)),
        None,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&dirs).await;

    Ok(Json(json!({ "message": "Event deleted successfully" })).into_response())
}

fn bad_multipart(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        format!("Permintaan multipart tidak valid: {e}"),
    )
}

/// Baca field `file`. Validasi: required, file, max 10240 KB.
async fn read_file(multipart: &mut Multipart) -> Result<media::Upload, ApiError> {
    let mut part: Option<(Option<String>, Vec<u8>)> = None;
    while let Some(field) = multipart.next_field().await.map_err(bad_multipart)? {
        if field.name() != Some("file") {
            continue;
        }
        let name = field.file_name().map(str::to_string);
        let bytes = field.bytes().await.map_err(bad_multipart)?;
        part = Some((name, bytes.to_vec()));
    }

    let message = match &part {
        None => Some("The file field is required."),
        Some((None, bytes)) if bytes.is_empty() => Some("The file field is required."),
        Some((None, _)) => Some("The file must be a file."),
        Some((Some(_), bytes)) if bytes.is_empty() => Some("The file must be a file."),
        Some((Some(_), bytes)) if bytes.len() > MAX_UPLOAD_BYTES => {
            Some("The file must not be greater than 10240 kilobytes.")
        }
        Some((Some(_), _)) => None,
    };
    if let Some(msg) = message {
        let mut errs = Errs::new();
        foto::add(&mut errs, "file", msg.to_string());
        return Err(invalid(errs));
    }
    match part {
        Some((Some(name), bytes)) => Ok(media::Upload {
            original_name: name,
            bytes,
        }),
        _ => Err(internal("berkas upload tidak terbaca")),
    }
}

/// `POST /api/events/{id}/upload`. Simpan berkas ke media `event/attachments`, lalu tambahkan ke `attachments`.
pub async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let current = find_event(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    if current.user_id != user.user_id as i64 {
        return Err(forbidden());
    }
    let upload = read_file(&mut multipart).await?;
    let mime = media::mime_for_name(&upload.original_name);
    let url = format!("{}/api/events/{id}/upload", base_url(&state));

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let stored = media::attach(
        &mut tx,
        EVENT_MODEL,
        id as u64,
        ATTACHMENTS,
        &upload,
        mime,
        false,
    )
    .await?;

    let base = base_url(&state);
    let mut list = match &current.attachments {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    list.push(json!({
        "id": stored.media_id,
        "name": stored.file_name,
        "url": format!("{base}/storage/{}/{}", stored.media_id, stored.file_name),
        "type": mime,
        "size": upload.bytes.len(),
    }));
    let mut next = current.clone();
    next.attachments = Some(Value::Array(list));

    let saved = async {
        save_event(&mut tx, &next).await?;
        let after = find_event(&mut *tx, id)
            .await
            .map_err(internal)?
            .ok_or_else(|| internal("event hilang saat upload"))?;
        audit_updated(&mut tx, &headers, user.user_id, &url, &current, &after).await?;
        Ok::<EventRow, ApiError>(after)
    }
    .await;

    match saved {
        Ok(after) => {
            tx.commit().await.map_err(internal)?;
            Ok(data(resource(&after)))
        }
        Err(e) => {
            // Transaksi di-rollback saat `tx` dibuang. Berkas di disk dihapus manual.
            media::remove_dirs(&[stored.dir.clone()]).await;
            Err(e)
        }
    }
}
