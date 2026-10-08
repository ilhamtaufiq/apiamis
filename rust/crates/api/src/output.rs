//! `/api/output`: port `OutputController` (daftar, store, show, update, destroy, summary).
//!
//! Output memakai `Auditable` dan `NotifiesAdminsOnChanges` di Laravel, jadi setiap perubahan
//! menulis audit dan notifikasi (`changes.rs`). Pembatasan per pekerjaan (T36) juga berlaku di sini.

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, QueryBuilder, Row};

use crate::{
    access, changes, format::iso8601_utc, foto, lookup::carbon_json, media::internal, pagination,
    pekerjaan, penerima, require_auth, AppState,
};

const SELECT_OUTPUT: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, \
     komponen, satuan, CAST(volume AS CHAR) AS volume, penerima_is_optional, created_at, updated_at FROM tbl_output";
const TABLE: &str = "tbl_output";

/// Kolom yang bisa diubah, urut tetap untuk audit.
const COLUMNS: &[&str] = &[
    "pekerjaan_id",
    "komponen",
    "satuan",
    "volume",
    "penerima_is_optional",
];

/// Baris `tbl_output`. `volume` disimpan sebagai string dua desimal seperti cast `decimal:2`.
#[derive(Debug, Clone, PartialEq)]
pub struct OutRow {
    pub id: i64,
    pub pekerjaan_id: i64,
    pub komponen: String,
    pub satuan: String,
    pub volume: String,
    pub penerima_is_optional: bool,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl OutRow {
    fn volume_f64(&self) -> f64 {
        self.volume.parse().unwrap_or(0.0)
    }
}

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<OutRow, sqlx::Error> {
    Ok(OutRow {
        id: r.try_get("id")?,
        pekerjaan_id: r.try_get("pekerjaan_id")?,
        komponen: r.try_get("komponen")?,
        satuan: r.try_get("satuan")?,
        volume: r
            .try_get::<Option<String>, _>("volume")?
            .unwrap_or_default(),
        penerima_is_optional: r.try_get("penerima_is_optional")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn find_row<'e, E>(exec: E, id: i64) -> Result<Option<OutRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_OUTPUT} WHERE id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_row).transpose()
}

fn col_json(row: &OutRow, col: &str) -> Value {
    match col {
        "pekerjaan_id" => json!(row.pekerjaan_id),
        "komponen" => json!(row.komponen),
        "satuan" => json!(row.satuan),
        "volume" => json!(row.volume),
        "penerima_is_optional" => json!(row.penerima_is_optional),
        _ => Value::Null,
    }
}

/// `getAttributes()` untuk audit `created` dan `deleted`.
fn attributes(row: &OutRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    for col in COLUMNS {
        m.insert((*col).into(), col_json(row, col));
    }
    m.insert("created_at".into(), carbon_json(row.created_at));
    m.insert("updated_at".into(), carbon_json(row.updated_at));
    m
}

/// `OutputResource`: relasi `pekerjaan` selalu dimuat oleh setiap aksi di controller.
async fn resource(pool: &MySqlPool, row: &OutRow) -> Result<Value, ApiError> {
    let pekerjaan_json = match pekerjaan::find(pool, row.pekerjaan_id as u64)
        .await
        .map_err(internal)?
    {
        Some(p) => pekerjaan::to_resource(
            &p,
            &pekerjaan::Loaded::empty(pekerjaan::Mode {
                summary: false,
                unbounded: false,
            }),
        ),
        None => Value::Null,
    };
    Ok(json!({
        "id": row.id,
        "pekerjaan_id": row.pekerjaan_id,
        "komponen": row.komponen,
        "satuan": row.satuan,
        "volume": row.volume,
        "penerima_is_optional": row.penerima_is_optional,
        "pekerjaan": pekerjaan_json,
        "created_at": iso8601_utc(row.created_at),
        "updated_at": iso8601_utc(row.updated_at),
    }))
}

/// Pembatasan per pekerjaan (T36), sama dengan penerima dan berkas. `table` adalah nama tabel atau alias output di query.
fn scope_clause(actor_roles: &[(u64, String)], actor: u64, table: &str) -> (String, Vec<String>) {
    let r = access::restriction(actor, actor_roles, "sp");
    (
        format!(
            "{table}.pekerjaan_id IN (SELECT sp.id FROM tbl_pekerjaan sp WHERE 1=1{})",
            r.sql
        ),
        r.binds.iter().map(u64::to_string).collect(),
    )
}

async fn ensure_access(state: &AppState, actor: u64, pekerjaan_id: i64) -> Result<(), ApiError> {
    let roles = auth::login::roles_of(&state.pool, actor)
        .await
        .map_err(internal)?;
    foto::ensure_access(state, actor, &roles, Some(pekerjaan_id)).await
}

async fn ensure_pekerjaan_exists(pool: &MySqlPool, id: i64) -> Result<(), ApiError> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    if n > 0 {
        return Ok(());
    }
    let mut errs = BTreeMap::new();
    foto::add(
        &mut errs,
        "pekerjaan_id",
        "The selected pekerjaan id is invalid.".into(),
    );
    Err(ApiError::validation("The given data was invalid.", errs))
}

fn set_string(input: &mut Input, key: &str, value: Option<String>) {
    match key {
        "komponen" => input.komponen = Some(value),
        _ => input.satuan = Some(value),
    }
}

/// `numeric|min:0` untuk JSON: angka, atau string yang bisa dibaca sebagai angka.
fn as_numeric(v: &Value) -> Option<f64> {
    let n = match v {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    n.is_finite().then_some(n)
}

// ---------------------------------------------------------------------------
// Validasi input
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Input {
    pekerjaan_id: Option<Option<i64>>,
    komponen: Option<Option<String>>,
    satuan: Option<Option<String>>,
    volume: Option<Option<f64>>,
    penerima_is_optional: Option<Option<bool>>,
}

fn parse_input(body: &Value, store: bool) -> Result<Input, ApiError> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let get = |key: &str| obj.get(key).cloned().unwrap_or(Value::Null);
    let present = |key: &str| obj.get(key).is_some_and(|v| !v.is_null());
    let mut errs = BTreeMap::new();
    let mut input = Input::default();

    if store && !present("pekerjaan_id") {
        foto::add(
            &mut errs,
            "pekerjaan_id",
            "The pekerjaan id field is required.".into(),
        );
    } else if obj.contains_key("pekerjaan_id") {
        let v = get("pekerjaan_id");
        if v.is_null() {
            input.pekerjaan_id = Some(None);
        } else {
            match penerima::as_int(&v) {
                Some(n) => input.pekerjaan_id = Some(Some(n)),
                None => foto::add(
                    &mut errs,
                    "pekerjaan_id",
                    "The pekerjaan id field must be an integer.".into(),
                ),
            }
        }
    }

    for key in ["komponen", "satuan"] {
        if store && !present(key) {
            foto::add(&mut errs, key, format!("The {key} field is required."));
        } else if obj.contains_key(key) {
            match get(key) {
                Value::Null => set_string(&mut input, key, None),
                Value::String(v) if v.chars().count() <= 255 => {
                    set_string(&mut input, key, Some(v))
                }
                Value::String(_) => foto::add(
                    &mut errs,
                    key,
                    format!("The {key} field must not be greater than 255 characters."),
                ),
                _ => foto::add(&mut errs, key, format!("The {key} field must be a string.")),
            }
        }
    }

    if store && !present("volume") {
        foto::add(&mut errs, "volume", "The volume field is required.".into());
    } else if obj.contains_key("volume") {
        let v = get("volume");
        if v.is_null() {
            input.volume = Some(None);
        } else {
            match as_numeric(&v) {
                Some(n) if n >= 0.0 => input.volume = Some(Some(n)),
                Some(_) => foto::add(
                    &mut errs,
                    "volume",
                    "The volume field must be at least 0.".into(),
                ),
                None => foto::add(
                    &mut errs,
                    "volume",
                    "The volume field must be a number.".into(),
                ),
            }
        }
    }

    if obj.contains_key("penerima_is_optional") {
        let v = get("penerima_is_optional");
        if v.is_null() {
            input.penerima_is_optional = Some(None);
        } else {
            match penerima::as_bool(&v) {
                Some(b) => input.penerima_is_optional = Some(Some(b)),
                None => foto::add(
                    &mut errs,
                    "penerima_is_optional",
                    "The penerima is optional field must be true or false.".into(),
                ),
            }
        }
    }

    if errs.is_empty() {
        Ok(input)
    } else {
        Err(ApiError::validation("The given data was invalid.", errs))
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/output`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let (scope, scope_binds) = scope_clause(&roles, user.user_id, TABLE);

    let mut clauses = vec![scope];
    let mut binds = scope_binds;
    if query
        .get("tahun")
        .is_some_and(|v| !v.is_empty() && v != "0")
    {
        clauses.push("tbl_output.pekerjaan_id IN (SELECT pk.id FROM tbl_pekerjaan pk JOIN tbl_kegiatan k ON k.id = pk.kegiatan_id WHERE k.tahun_anggaran = ?)".into());
        binds.push(query["tahun"].clone());
    }
    if let Some(pid) = query.get("pekerjaan_id") {
        clauses.push("tbl_output.pekerjaan_id = ?".into());
        binds.push(pid.clone());
    }
    let where_sql = format!(" WHERE {}", clauses.join(" AND "));

    // `per_page=-1` mengembalikan semua baris tanpa paginasi (dengan atau tanpa pekerjaan_id).
    let all = query.get("per_page").map(|v| v.trim()) == Some("-1");
    let per_page = query
        .get("per_page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(20);
    let page = query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1);

    let mut sql = format!("{SELECT_OUTPUT}{where_sql} ORDER BY id");
    if !all {
        sql.push_str(" LIMIT ? OFFSET ?");
    }
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    if !all {
        q = q.bind(per_page).bind((page - 1) * per_page);
    }
    let rows = q
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?
        .iter()
        .map(map_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;

    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        data.push(resource(&state.pool, row).await?);
    }
    if all {
        return Ok(Json(json!({ "data": data })).into_response());
    }

    let count_sql = format!("SELECT COUNT(*) FROM {TABLE}{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b);
    }
    let total = cq.fetch_one(&state.pool).await.map_err(internal)? as u64;
    // Paginator Laravel tanpa `appends`.
    let base = format!("{}/api/output", state.app_url.trim_end_matches('/'));
    Ok(Json(pagination::paginate_with_query(
        data,
        total,
        pagination::PageParams { page, per_page },
        &base,
        "",
    ))
    .into_response())
}

/// `POST /api/output`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_input(&body, true)?;
    let pekerjaan_id = input.pekerjaan_id.flatten().unwrap_or_default();
    ensure_pekerjaan_exists(&state.pool, pekerjaan_id).await?;
    ensure_access(&state, user.user_id, pekerjaan_id).await?;

    let url = format!("{}/api/output", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let id = sqlx::query(
        "INSERT INTO tbl_output (pekerjaan_id, komponen, satuan, volume, penerima_is_optional, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan_id)
    .bind(input.komponen.clone().flatten().unwrap_or_default())
    .bind(input.satuan.clone().flatten().unwrap_or_default())
    .bind(input.volume.flatten().unwrap_or(0.0))
    .bind(input.penerima_is_optional.flatten().unwrap_or(false))
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id() as i64;
    let row = find_row(&mut *tx, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| internal("output baru tidak terbaca"))?;
    changes::log(
        &mut tx,
        &headers,
        user.user_id,
        &changes::OUTPUT,
        "created",
        id,
        None,
        Some(attributes(&row)),
        Some(pekerjaan_id),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    let data = resource(&state.pool, &row).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `GET /api/output/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_access(&state, user.user_id, row.pekerjaan_id).await?;
    let data = resource(&state.pool, &row).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `PUT` dan `PATCH /api/output/{id}`. Field yang tidak dikirim tidak diubah.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_access(&state, user.user_id, current.pekerjaan_id).await?;
    let input = parse_input(&body, false)?;
    if let Some(Some(p)) = input.pekerjaan_id {
        ensure_pekerjaan_exists(&state.pool, p).await?;
        ensure_access(&state, user.user_id, p).await?;
    }

    // Kolom NOT NULL yang dikirim null: Laravel gagal di database (500). Meniru itu di sini.
    for (set, col) in [
        (input.pekerjaan_id.map(|v| v.is_none()), "pekerjaan_id"),
        (input.komponen.as_ref().map(Option::is_none), "komponen"),
        (input.satuan.as_ref().map(Option::is_none), "satuan"),
        (input.volume.map(|v| v.is_none()), "volume"),
        (
            input.penerima_is_optional.map(|v| v.is_none()),
            "penerima_is_optional",
        ),
    ] {
        if set == Some(true) {
            return Err(internal(format!("Column '{col}' cannot be null")));
        }
    }

    let mut next = current.clone();
    if let Some(Some(p)) = input.pekerjaan_id {
        next.pekerjaan_id = p;
    }
    if let Some(Some(v)) = input.komponen {
        next.komponen = v;
    }
    if let Some(Some(v)) = input.satuan {
        next.satuan = v;
    }
    if let Some(Some(v)) = input.volume {
        next.volume = format!("{v:.2}");
    }
    if let Some(Some(v)) = input.penerima_is_optional {
        next.penerima_is_optional = v;
    }

    let changed: Vec<&str> = COLUMNS
        .iter()
        .copied()
        .filter(|c| col_json(&current, c) != col_json(&next, c))
        .collect();

    let url = format!("{}/api/output/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    if !changed.is_empty() {
        let mut qb = QueryBuilder::<MySql>::new("UPDATE tbl_output SET ");
        for (i, col) in changed.iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            qb.push(*col).push(" = ");
            match *col {
                "pekerjaan_id" => {
                    qb.push_bind(next.pekerjaan_id);
                }
                "komponen" => {
                    qb.push_bind(next.komponen.clone());
                }
                "satuan" => {
                    qb.push_bind(next.satuan.clone());
                }
                "volume" => {
                    qb.push_bind(next.volume_f64());
                }
                _ => {
                    qb.push_bind(next.penerima_is_optional);
                }
            }
        }
        qb.push(", updated_at = NOW() WHERE id = ").push_bind(id);
        qb.build().execute(&mut *tx).await.map_err(internal)?;
        let after = find_row(&mut *tx, id)
            .await
            .map_err(internal)?
            .ok_or_else(|| internal("output hilang saat update"))?;
        let mut old = Map::new();
        let mut new = Map::new();
        for col in &changed {
            old.insert((*col).into(), col_json(&current, col));
            new.insert((*col).into(), col_json(&after, col));
        }
        old.insert("updated_at".into(), carbon_json(current.updated_at));
        new.insert("updated_at".into(), carbon_json(after.updated_at));
        changes::log(
            &mut tx,
            &headers,
            user.user_id,
            &changes::OUTPUT,
            "updated",
            id,
            Some(old),
            Some(new),
            Some(next.pekerjaan_id),
            &url,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;

    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let data = resource(&state.pool, &row).await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// `DELETE /api/output/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    ensure_access(&state, user.user_id, row.pekerjaan_id).await?;
    let url = format!("{}/api/output/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_output WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    changes::log(
        &mut tx,
        &headers,
        user.user_id,
        &changes::OUTPUT,
        "deleted",
        id,
        Some(attributes(&row)),
        None,
        Some(row.pekerjaan_id),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok((
        StatusCode::OK,
        Json(json!({ "message": "Output deleted successfully" })),
    )
        .into_response())
}

/// Label komponen seperti `OutputController::summary`: "Sambungan Rumah" dibedakan menurut sub bidang.
fn resolve_label(komponen: &str, sub_bidang: Option<&str>) -> String {
    if komponen.to_ascii_lowercase().contains("sambungan rumah") {
        if let Some(sb) = sub_bidang.filter(|s| !s.is_empty() && *s != "0") {
            let lower = sb.to_ascii_lowercase();
            if lower.contains("air minum") {
                return "Sambungan Rumah Water Meter".into();
            } else if lower.contains("sanitasi") {
                return "Sambungan Rumah Sanitasi".into();
            }
        }
    }
    komponen.to_string()
}

/// `GET /api/output/summary`: total dan rekap per komponen dan satuan.
pub async fn summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let (scope, scope_binds) = scope_clause(&roles, user.user_id, "o");

    let mut clauses = vec![scope];
    let mut binds = scope_binds;
    if query
        .get("tahun")
        .is_some_and(|v| !v.is_empty() && v != "0")
    {
        clauses.push("k.tahun_anggaran = ?".into());
        binds.push(query["tahun"].clone());
    }
    let sql = format!(
        "SELECT CAST(o.id AS SIGNED) AS id, CAST(o.pekerjaan_id AS SIGNED) AS pekerjaan_id, o.komponen, o.satuan, \
         CAST(o.volume AS CHAR) AS volume, o.penerima_is_optional, p.nama_paket, k.sub_bidang \
         FROM tbl_output o LEFT JOIN tbl_pekerjaan p ON p.id = o.pekerjaan_id \
         LEFT JOIN tbl_kegiatan k ON k.id = p.kegiatan_id WHERE {} ORDER BY o.id",
        clauses.join(" AND ")
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;

    let total = rows.len() as i64;
    let mut wajib = 0i64;
    let mut total_volume = 0.0f64;
    // Kelompok dalam urutan kemunculan, seperti groupBy di Laravel.
    let mut groups: Vec<Group> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for r in &rows {
        let komponen: String = r.try_get("komponen").map_err(internal)?;
        let satuan: String = r.try_get("satuan").map_err(internal)?;
        let volume: f64 = r
            .try_get::<Option<String>, _>("volume")
            .map_err(internal)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let optional: bool = r.try_get("penerima_is_optional").map_err(internal)?;
        let pekerjaan_id: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        let nama: Option<String> = r.try_get("nama_paket").map_err(internal)?;
        let sub_bidang: Option<String> = r.try_get("sub_bidang").map_err(internal)?;
        let id: i64 = r.try_get("id").map_err(internal)?;
        if !optional {
            wajib += 1;
        }
        total_volume += volume;

        let label = resolve_label(&komponen, sub_bidang.as_deref());
        let key = format!("{label}|||{satuan}");
        let idx = *index.entry(key).or_insert_with(|| {
            groups.push(Group {
                komponen: label.clone(),
                satuan: satuan.clone(),
                total: 0.0,
                pekerjaan_ids: Vec::new(),
                items: Vec::new(),
            });
            groups.len() - 1
        });
        let g = &mut groups[idx];
        g.total += volume;
        let pid_s = pekerjaan_id.to_string();
        if !g.pekerjaan_ids.contains(&pid_s) {
            g.pekerjaan_ids.push(pid_s);
        }
        g.items.push(json!({
            "id": id,
            "pekerjaan_id": pekerjaan_id,
            "nama_paket": nama.unwrap_or_else(|| "-".into()),
            "volume": volume_string(volume),
            "penerima_is_optional": optional,
            "sub_bidang": sub_bidang,
        }));
    }

    let rekap: Vec<Value> = groups
        .into_iter()
        .map(|g| {
            json!({
                "komponen": g.komponen,
                "satuan": g.satuan,
                "total_volume": g.total,
                "jumlah_pekerjaan": g.pekerjaan_ids.len(),
                "pekerjaan": g.items,
            })
        })
        .collect();

    Ok(Json(json!({
        "total_output": total,
        "wajib_count": wajib,
        "opsional_count": total - wajib,
        // `sum()` pada koleksi kosong adalah int 0 di PHP.
        "total_volume": if total == 0 { json!(0) } else { json!(total_volume) },
        "rekap": rekap,
    }))
    .into_response())
}

/// Satu kelompok rekap summary: komponen (setelah label) dan satuan.
struct Group {
    komponen: String,
    satuan: String,
    total: f64,
    pekerjaan_ids: Vec<String>,
    items: Vec<Value>,
}

/// Volume dalam bentuk dua desimal, seperti cast `decimal:2`.
fn volume_string(v: f64) -> String {
    format!("{v:.2}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sambungan_rumah_label_depends_on_sub_bidang() {
        assert_eq!(
            resolve_label("Sambungan Rumah", Some("Air Minum")),
            "Sambungan Rumah Water Meter"
        );
        assert_eq!(
            resolve_label("sambungan rumah baru", Some("SANITASI")),
            "Sambungan Rumah Sanitasi"
        );
        assert_eq!(resolve_label("Sambungan Rumah", None), "Sambungan Rumah");
        assert_eq!(resolve_label("Pipa", Some("Air Minum")), "Pipa");
    }

    #[test]
    fn volume_must_be_non_negative_number() {
        let ok = parse_input(
            &json!({"pekerjaan_id": 1, "komponen": "a", "satuan": "m", "volume": "2.5"}),
            true,
        );
        assert!(ok.is_ok());
        let neg = parse_input(
            &json!({"pekerjaan_id": 1, "komponen": "a", "satuan": "m", "volume": -1}),
            true,
        );
        assert!(neg.is_err());
        let missing = parse_input(&json!({}), true).unwrap_err().body();
        assert!(missing["errors"]["komponen"].is_array());
    }
}
