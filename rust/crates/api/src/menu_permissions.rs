//! Izin menu (`MenuPermissionController`): tabel `menu_permissions`, model `App\Models\MenuPermission`.
//! Model hanya memakai `Auditable` (tanpa notifikasi admin dan tanpa invalidasi cache Spatie).
//!
//! Rute:
//! - `GET /api/menu-permissions`: daftar 15 per halaman, urut `menu_label`, filter `search` dan `is_active`.
//! - `POST /api/menu-permissions`: 201, respons JSON langsung (tanpa `data`).
//! - `GET /api/menu-permissions/{id}`: respons dibungkus `{"data": ...}` seperti `JsonResource`.
//! - `PUT` dan `PATCH /api/menu-permissions/{id}`: 200, respons JSON langsung.
//! - `DELETE /api/menu-permissions/{id}`: `{"message": "Menu permission deleted"}`.
//! - `GET /api/menu-permissions/user/menus`: menu yang boleh diakses user yang login.
//!
//! Akses: `/menu-permissions` ada di `ADMIN_ONLY_ROUTES` (`CheckRoutePermission`), jadi non-admin ditolak
//! oleh `route_permission::check` kecuali ada rule di `route_permissions`. Handler hanya memeriksa login,
//! seperti `permissions.rs`. `user/menus` masuk whitelist middleware dan hanya membutuhkan login.
//!
//! Urutan pemeriksaan sama dengan Laravel: `{id}` dicari dulu (404), lalu login (401), lalu validasi (422).
//!
//! Perbedaan dengan Laravel:
//! - Validasi hanya mencatat satu pesan per field. Laravel bisa mencatat lebih dari satu (mis. `required` dan `string`).
//! - `boolean` memakai aturan strict Laravel: `true`, `false`, `0`, `1`, `"0"`, `"1"`.
//! - `index` diurutkan `menu_label, id` (Laravel hanya `menu_label`). `user/menus` diurutkan `id`.
//! - `store`, `update`, dan `destroy` memakai transaksi.
//! - 404 karena `{id}` tidak ada memakai `Not Found.` seperti modul Rust lain. Laravel mengirim pesan
//!   `No query results for model [...]` (perlu dicek terhadap produksi).
//! - `old_values` dan `new_values` audit diambil dari baris DB, jadi tipe kecil bisa berbeda.

use std::collections::{BTreeMap, HashMap};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySql, MySqlPool, Row};

use crate::{
    changes,
    lookup::carbon_json,
    media::internal,
    pagination::{self, PageParams},
    require_auth,
    survey_lokasi::audit_update,
    AppState,
};

const TABLE: &str = "menu_permissions";
const MODEL: &str = "App\\Models\\MenuPermission";
/// `paginate(15)`: jumlah per halaman tetap.
const PER_PAGE: u64 = 15;
/// Kolom yang bisa dikirim lewat `store` dan `update` (`$fillable`).
const FIELDS: &[&str] = &[
    "menu_key",
    "menu_label",
    "menu_parent",
    "allowed_roles",
    "is_active",
];
const INVALID_MESSAGE: &str = "The given data was invalid.";

// ---------------------------------------------------------------------------
// Helper umum
// ---------------------------------------------------------------------------

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse::<i64>().map_err(|_| ApiError::not_found())
}

fn app_base(state: &AppState) -> String {
    state.app_url.trim_end_matches('/').to_string()
}

/// `TrimStrings` dan `ConvertEmptyStringsToNull`, rekursif seperti middleware Laravel.
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
        Value::Array(a) => Value::Array(a.into_iter().map(normalize).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, normalize(v))).collect()),
        other => other,
    }
}

/// Body JSON sebagai objek yang sudah dinormalisasi. Selain objek dianggap kosong.
fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m.into_iter().map(|(k, v)| (k, normalize(v))).collect(),
        _ => Map::new(),
    }
}

fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn opt_text(input: &Map<String, Value>, key: &str) -> Option<String> {
    input.get(key).and_then(text_of)
}

/// Nilai boolean seperti aturan `boolean` Laravel (`in_array` strict): `true`, `false`, `0`, `1`, `"0"`, `"1"`.
fn bool_value(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_i64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
        Value::String(s) => match s.as_str() {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        },
        _ => None,
    }
}

/// `$request->has('search') && $request->search`: di-trim, dan kosong atau "0" dianggap false.
fn search_term(q: &HashMap<String, String>) -> Option<String> {
    q.get("search")
        .map(|v| v.trim())
        .filter(|v| !v.is_empty() && *v != "0")
        .map(str::to_string)
}

/// `$request->is_active` untuk query string: di-trim, dan kosong menjadi null.
fn filled_q(q: &HashMap<String, String>, key: &str) -> Option<String> {
    q.get(key)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn pick(full: &Map<String, Value>, keys: &[&str]) -> Map<String, Value> {
    full.iter()
        .filter(|(k, _)| keys.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Kolom `allowed_roles` sebagai JSON teks. Absen atau null menjadi NULL.
fn allowed_json(input: &Map<String, Value>) -> Option<String> {
    input
        .get("allowed_roles")
        .filter(|v| !v.is_null())
        .map(|v| v.to_string())
}

/// Bentuk `empty()` PHP untuk nilai JSON.
fn php_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty() || s == "0",
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

// ---------------------------------------------------------------------------
// Validasi (`$request->validate`)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Errs(BTreeMap<String, Vec<String>>);

impl Errs {
    fn add(&mut self, field: &str, message: impl Into<String>) {
        self.0
            .entry(field.to_string())
            .or_default()
            .push(message.into());
    }

    fn finish(self) -> Result<(), ApiError> {
        if self.0.is_empty() {
            return Ok(());
        }
        Err(ApiError::validation(INVALID_MESSAGE, self.0))
    }
}

/// `unique:menu_permissions,menu_key` dengan pengecualian `except` (0 = tidak ada).
async fn key_taken(pool: &MySqlPool, key: &str, except: i64) -> Result<bool, ApiError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM menu_permissions WHERE menu_key = ? AND id <> ?",
    )
    .bind(key)
    .bind(except)
    .fetch_one(pool)
    .await
    .map_err(internal)?;
    Ok(n > 0)
}

/// Aturan `store` (`required`) dan `update` (`sometimes`). `current` = id baris yang diubah.
async fn validate(
    pool: &MySqlPool,
    input: &Map<String, Value>,
    current: Option<i64>,
) -> Result<(), ApiError> {
    let update = current.is_some();
    let mut e = Errs::default();

    // menu_key: required|string|unique:menu_permissions,menu_key
    match input.get("menu_key") {
        None if update => {}
        None => e.add("menu_key", "The menu key field is required."),
        Some(Value::Null) if !update => e.add("menu_key", "The menu key field is required."),
        Some(Value::String(s)) => {
            if key_taken(pool, s, current.unwrap_or(0)).await? {
                e.add("menu_key", "The menu key has already been taken.");
            }
        }
        Some(_) => e.add("menu_key", "The menu key must be a string."),
    }

    // menu_label: required|string
    match input.get("menu_label") {
        None if update => {}
        None => e.add("menu_label", "The menu label field is required."),
        Some(Value::Null) if !update => e.add("menu_label", "The menu label field is required."),
        Some(Value::String(_)) => {}
        Some(_) => e.add("menu_label", "The menu label must be a string."),
    }

    // menu_parent: nullable|string
    if let Some(v) = input.get("menu_parent").filter(|v| !v.is_null()) {
        if !v.is_string() {
            e.add("menu_parent", "The menu parent must be a string.");
        }
    }

    // allowed_roles: nullable|array
    if let Some(v) = input.get("allowed_roles").filter(|v| !v.is_null()) {
        if !v.is_array() {
            e.add("allowed_roles", "The allowed roles field must be an array.");
        }
    }

    // is_active: boolean (tanpa nullable, jadi null yang ada ikut divalidasi)
    if let Some(v) = input.get("is_active") {
        if bool_value(v).is_none() {
            e.add("is_active", "The is active field must be true or false.");
        }
    }

    e.finish()
}

// ---------------------------------------------------------------------------
// Baca data
// ---------------------------------------------------------------------------

struct MenuRow {
    id: i64,
    menu_key: String,
    menu_label: String,
    menu_parent: Option<String>,
    allowed_roles: Value,
    is_active: bool,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

const SELECT_MENU: &str = "SELECT CAST(id AS SIGNED) AS id, menu_key, menu_label, menu_parent, \
    CAST(allowed_roles AS CHAR) AS allowed_roles, CAST(is_active AS SIGNED) AS is_active, \
    created_at, updated_at FROM menu_permissions";

fn map_row(row: &MySqlRow) -> Result<MenuRow, sqlx::Error> {
    let raw: Option<String> = row.try_get("allowed_roles")?;
    let is_active: i64 = row.try_get("is_active")?;
    Ok(MenuRow {
        id: row.try_get("id")?,
        menu_key: row.try_get("menu_key")?,
        menu_label: row.try_get("menu_label")?,
        menu_parent: row.try_get("menu_parent")?,
        allowed_roles: raw
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(Value::Null),
        is_active: is_active != 0,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// Bentuk JSON `MenuPermissionResource`.
fn menu_json(r: &MenuRow) -> Value {
    json!({
        "id": r.id,
        "menu_key": r.menu_key,
        "menu_label": r.menu_label,
        "menu_parent": r.menu_parent,
        "allowed_roles": r.allowed_roles,
        "is_active": r.is_active,
        "created_at": carbon_json(r.created_at),
        "updated_at": carbon_json(r.updated_at),
    })
}

async fn load_menu(pool: &MySqlPool, id: i64) -> Result<Option<MenuRow>, ApiError> {
    let sql = format!("{SELECT_MENU} WHERE id = ?");
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    row.map(|r| map_row(&r).map_err(internal)).transpose()
}

/// Setara route model binding: baris dengan id tersebut harus ada.
async fn ensure_exists(pool: &MySqlPool, id: i64) -> Result<(), ApiError> {
    let n: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM menu_permissions WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    if n == 0 {
        return Err(ApiError::not_found());
    }
    Ok(())
}

/// Atribut seperti `getAttributes()` untuk audit (kunci = kolom).
const SELECT_ATTRS: &str = "SELECT CAST(id AS SIGNED) AS id, menu_key, menu_label, menu_parent, \
    CAST(allowed_roles AS CHAR) AS allowed_roles, CAST(is_active AS SIGNED) AS is_active, \
    CAST(created_at AS CHAR) AS created_at, CAST(updated_at AS CHAR) AS updated_at \
    FROM menu_permissions WHERE id = ?";

async fn attributes<'c, E>(exec: E, id: i64) -> Result<Map<String, Value>, ApiError>
where
    E: sqlx::Executor<'c, Database = MySql>,
{
    let row = sqlx::query(SELECT_ATTRS)
        .bind(id)
        .fetch_optional(exec)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let raw: Option<String> = row.try_get("allowed_roles").map_err(internal)?;
    let is_active: Option<i64> = row.try_get("is_active").map_err(internal)?;
    let mut m = Map::new();
    m.insert(
        "id".into(),
        json!(row.try_get::<Option<i64>, _>("id").map_err(internal)?),
    );
    for col in ["menu_key", "menu_label", "menu_parent", "created_at", "updated_at"] {
        m.insert(
            col.into(),
            json!(row.try_get::<Option<String>, _>(col).map_err(internal)?),
        );
    }
    m.insert(
        "allowed_roles".into(),
        raw.and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(Value::Null),
    );
    m.insert("is_active".into(), json!(is_active.map(|v| v != 0)));
    Ok(m)
}

// ---------------------------------------------------------------------------
// Tulis data
// ---------------------------------------------------------------------------

/// Nilai untuk `UPDATE ... SET kolom = ?`.
#[derive(Clone)]
enum Bind {
    Int(Option<i64>),
    Text(Option<String>),
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/menu-permissions`: 15 per halaman, urut `menu_label`. Filter `search` (`menu_key` atau
/// `menu_label`) dan `is_active` (nilai dibandingkan mentah seperti `where` Laravel).
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;

    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(s) = search_term(&q) {
        conds.push("(menu_key LIKE ? OR menu_label LIKE ?)".into());
        let pattern = format!("%{s}%");
        binds.push(pattern.clone());
        binds.push(pattern);
    }
    if q.contains_key("is_active") {
        match filled_q(&q, "is_active") {
            Some(v) => {
                conds.push("is_active = ?".into());
                binds.push(v);
            }
            None => conds.push("is_active IS NULL".into()),
        }
    }
    let where_sql = if conds.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conds.join(" AND "))
    };

    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM menu_permissions{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b.clone());
    }
    let total = cq.fetch_one(&state.pool).await.map_err(internal)?;

    let page = pagination::page_params(&q).page;
    let sql = format!(
        "{SELECT_MENU}{where_sql} ORDER BY menu_label, id LIMIT {PER_PAGE} OFFSET {}",
        (page - 1).saturating_mul(PER_PAGE)
    );
    let mut dq = sqlx::query(&sql);
    for b in &binds {
        dq = dq.bind(b.clone());
    }
    let rows = dq.fetch_all(&state.pool).await.map_err(internal)?;
    let data: Vec<Value> = rows
        .iter()
        .map(|r| map_row(r).map(|m| menu_json(&m)).map_err(internal))
        .collect::<Result<_, _>>()?;

    let base = format!("{}/api/menu-permissions", app_base(&state));
    Ok(Json(pagination::paginate(
        data,
        total as u64,
        PageParams {
            page,
            per_page: PER_PAGE,
        },
        &base,
    ))
    .into_response())
}

/// `POST /api/menu-permissions`: 201 dengan resource langsung (tanpa `data`).
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    validate(&state.pool, &input, None).await?;

    let url = format!("{}/api/menu-permissions", app_base(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO menu_permissions (menu_key, menu_label, menu_parent, allowed_roles, is_active, \
         created_at, updated_at) VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(opt_text(&input, "menu_key").unwrap_or_default())
    .bind(opt_text(&input, "menu_label").unwrap_or_default())
    .bind(opt_text(&input, "menu_parent"))
    .bind(allowed_json(&input))
    // Default kolom `is_active` di database adalah true.
    .bind(input.get("is_active").and_then(bool_value).unwrap_or(true))
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;

    // Audit `created`: kunci yang di-insert, plus id dan timestamp.
    let mut keys: Vec<&str> = FIELDS
        .iter()
        .copied()
        .filter(|k| input.contains_key(*k))
        .collect();
    keys.extend(["id", "created_at", "updated_at"]);
    let full = attributes(&mut *tx, id).await?;
    let new = pick(&full, &keys);
    changes::audit_only(
        &mut tx,
        &headers,
        user.user_id,
        MODEL,
        "created",
        id,
        None,
        Some(new),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    let row = load_menu(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok((StatusCode::CREATED, Json(menu_json(&row))).into_response())
}

/// `GET /api/menu-permissions/{id}`: dibungkus `{"data": ...}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    ensure_exists(&state.pool, id).await?;
    require_auth(&state, &headers).await?;
    let row = load_menu(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": menu_json(&row) })).into_response())
}

/// `PUT` dan `PATCH /api/menu-permissions/{id}`: hanya kolom yang dikirim yang diubah.
/// Tanpa perubahan nyata, tidak ada `UPDATE` dan tidak ada audit (seperti `save()` Laravel).
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    ensure_exists(&state.pool, id).await?;
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    validate(&state.pool, &input, Some(id)).await?;

    let mut sets: Vec<(&str, Bind)> = Vec::new();
    for &key in FIELDS {
        if !input.contains_key(key) {
            continue;
        }
        let bind = match key {
            "allowed_roles" => Bind::Text(allowed_json(&input)),
            "is_active" => Bind::Int(
                input
                    .get(key)
                    .and_then(bool_value)
                    .map(|b| if b { 1 } else { 0 }),
            ),
            _ => Bind::Text(opt_text(&input, key)),
        };
        sets.push((key, bind));
    }

    let url = format!("{}/api/menu-permissions/{id}", app_base(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let before = attributes(&mut *tx, id).await?;
    if !sets.is_empty() {
        let cols: Vec<String> = sets.iter().map(|(k, _)| format!("{k} = ?")).collect();
        let sql = format!("UPDATE {TABLE} SET {} WHERE id = ?", cols.join(", "));
        let mut q = sqlx::query(&sql);
        for (_, b) in &sets {
            q = match b {
                Bind::Int(v) => q.bind(*v),
                Bind::Text(v) => q.bind(v.clone()),
            };
        }
        q.bind(id).execute(&mut *tx).await.map_err(internal)?;
    }
    let after = attributes(&mut *tx, id).await?;
    audit_update(
        &mut tx,
        &headers,
        user.user_id,
        TABLE,
        MODEL,
        id,
        &before,
        after,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    let row = load_menu(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(menu_json(&row)).into_response())
}

/// `DELETE /api/menu-permissions/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    ensure_exists(&state.pool, id).await?;
    let user = require_auth(&state, &headers).await?;

    let url = format!("{}/api/menu-permissions/{id}", app_base(&state));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let before = attributes(&mut *tx, id).await?;
    changes::audit_only(
        &mut tx,
        &headers,
        user.user_id,
        MODEL,
        "deleted",
        id,
        Some(before),
        None,
        &url,
    )
    .await?;
    sqlx::query("DELETE FROM menu_permissions WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    Ok(Json(json!({ "message": "Menu permission deleted" })).into_response())
}

/// `CanAccess`: `allowed_roles` kosong berarti boleh semua. Selain itu harus ada irisan dengan role user.
fn can_access(allowed: &Value, roles: &[String]) -> bool {
    if php_empty(allowed) {
        return true;
    }
    let items: Vec<&Value> = match allowed {
        Value::Array(a) => a.iter().collect(),
        Value::Object(o) => o.values().collect(),
        _ => Vec::new(),
    };
    items.into_iter().any(|v| {
        let s = match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(true) => "1".to_string(),
            Value::Bool(false) => String::new(),
            _ => return false,
        };
        roles.contains(&s)
    })
}

/// `GET /api/menu-permissions/user/menus`: `configured_menus` (semua menu aktif) dan
/// `allowed_menus` (yang lolos `CanAccess` untuk user ini).
pub async fn get_user_menus(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::permission::user_role_names(&state.pool, user.user_id)
        .await
        .map_err(internal)?;

    let rows = sqlx::query(
        "SELECT menu_key, CAST(allowed_roles AS CHAR) AS allowed_roles \
         FROM menu_permissions WHERE is_active = 1 ORDER BY id",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    let mut configured: Vec<String> = Vec::new();
    let mut allowed: Vec<String> = Vec::new();
    for row in &rows {
        let key: String = row.try_get("menu_key").map_err(internal)?;
        let raw: Option<String> = row.try_get("allowed_roles").map_err(internal)?;
        let allowed_roles = raw
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(Value::Null);
        if can_access(&allowed_roles, &roles) {
            allowed.push(key.clone());
        }
        configured.push(key);
    }

    Ok(Json(json!({
        "allowed_menus": allowed,
        "configured_menus": configured,
    }))
    .into_response())
}
