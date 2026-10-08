//! Rute permission (`RoutePermissionController`, tabel `route_permissions`).
//!
//! Rute yang dipindahkan: `POST /api/route-permissions/check-access`, `GET /api/route-permissions/rules`,
//! `GET /api/route-permissions/user/accessible`, `GET|POST /api/route-permissions`,
//! `GET|PUT|PATCH|DELETE /api/route-permissions/{id}`.
//! Tidak dipindahkan: `POST /api/route-permissions/sync` (lihat bagian "Dilewati").
//!
//! Perilaku yang perlu diketahui:
//! - Di Laravel semua rute ini hanya `auth:sanctum`. Pembatasan non-admin datang dari middleware
//!   `check.route.permission` (`route_permission::check`) yang juga membungkus rute ini lewat
//!   `route_layer`. Handler tidak memeriksa admin sendiri, sama seperti Laravel. Akibatnya non-admin
//!   ditolak di `index`, `store`, `show`, `update`, `destroy`, dan `check-access` kecuali ada rule
//!   aktif untuk path+method tersebut (`/route-permissions` termasuk `ADMIN_ONLY_ROUTES`).
//! - `check-access` memakai `route_path` mentah dari body (tanpa normalisasi `/api`), seperti Laravel.
//! - `user/accessible` membalas objek ber-kunci (indeks asli) bila ada rule yang tersaring, karena
//!   Laravel memakai `Collection::filter()` lalu `response()->json()`. Bila tidak ada yang tersaring,
//!   membalas array biasa.
//! - `store` membalas model tanpa `description` dan `is_active` bila kedua field itu tidak dikirim,
//!   karena Eloquent tidak membaca ulang kolom default setelah insert.
//! - `update` tidak memeriksa duplikat `route_path` + `route_method`, sama seperti Laravel.
//! - Validasi hanya mencatat pesan pertama per field. Pesan pertama ikut menjadi `message`.
//! - String di-trim dan string kosong dianggap null (`TrimStrings` dan `ConvertEmptyStringsToNull`).
//! - Audit `tbl_audit_logs` ditulis untuk created, updated, dan deleted. Di Laravel `allowed_roles`
//!   di audit tersimpan sebagai string JSON ganda. Di sini ditulis sebagai array JSON.
//!
//! Dilewati:
//! - `POST /api/route-permissions/sync`: Laravel memindai `Route::getRoutes()` lalu membuat rule
//!   untuk setiap rute Laravel. Router Rust tidak punya daftar rute yang setara, jadi tidak bisa
//!   dipindahkan dengan hasil yang sama.

use std::collections::HashMap;

use auth::permission;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row};

use crate::{
    audit,
    desa::internal,
    lookup::carbon_json,
    pagination::{self, PageParams},
    require_auth,
    validation::Errors,
    AppState,
};

const BASE_PATH: &str = "/api/route-permissions";
const MODEL: &str = "App\\Models\\RoutePermission";
const METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];
/// Field yang dicatat di audit `updated` (dan dibandingkan saat menentukan perubahan).
const AUDIT_FIELDS: [&str; 5] = [
    "route_path",
    "route_method",
    "description",
    "allowed_roles",
    "is_active",
];

#[derive(Clone)]
struct RouteRow {
    id: u64,
    route_path: String,
    route_method: String,
    description: Option<String>,
    allowed_roles: Vec<String>,
    is_active: bool,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

const SELECT_ROW: &str = "SELECT id, route_path, route_method, description, \
    CAST(allowed_roles AS CHAR) AS allowed_roles, CAST(is_active AS SIGNED) AS is_active, \
    created_at, updated_at FROM route_permissions";

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<RouteRow, sqlx::Error> {
    let raw: Option<String> = r.try_get("allowed_roles")?;
    Ok(RouteRow {
        id: r.try_get("id")?,
        route_path: r.try_get("route_path")?,
        route_method: r.try_get("route_method")?,
        description: r.try_get("description")?,
        allowed_roles: raw
            .as_deref()
            .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
            .unwrap_or_default(),
        is_active: r.try_get::<i64, _>("is_active")? != 0,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

/// Bentuk model Eloquent `RoutePermission` (`toArray`).
fn row_json(r: &RouteRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(r.id));
    m.insert("route_path".into(), json!(r.route_path));
    m.insert("route_method".into(), json!(r.route_method));
    m.insert("description".into(), json!(r.description));
    m.insert("allowed_roles".into(), json!(r.allowed_roles));
    m.insert("is_active".into(), json!(r.is_active));
    m.insert("created_at".into(), carbon_json(r.created_at));
    m.insert("updated_at".into(), carbon_json(r.updated_at));
    m
}

async fn load(pool: &MySqlPool, id: u64) -> Result<Option<RouteRow>, ApiError> {
    let row = sqlx::query(&format!("{SELECT_ROW} WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    row.map(|r| map_row(&r).map_err(internal)).transpose()
}

async fn load_or_404(pool: &MySqlPool, id: &str) -> Result<RouteRow, ApiError> {
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    load(pool, id).await?.ok_or_else(ApiError::not_found)
}

async fn load_tx(tx: &mut sqlx::Transaction<'_, MySql>, id: u64) -> Result<RouteRow, ApiError> {
    let row = sqlx::query(&format!("{SELECT_ROW} WHERE id = ?"))
        .bind(id)
        .fetch_one(&mut **tx)
        .await
        .map_err(internal)?;
    map_row(&row).map_err(internal)
}

async fn query_rows(
    pool: &MySqlPool,
    sql: &str,
    binds: &[String],
) -> Result<Vec<RouteRow>, ApiError> {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await.map_err(internal)?;
    rows.iter().map(|r| map_row(r).map_err(internal)).collect()
}

/// Rule aktif dari semua method, urutan `id` seperti `get()` tanpa `orderBy`.
async fn active_all(pool: &MySqlPool) -> Result<Vec<RouteRow>, ApiError> {
    query_rows(
        pool,
        &format!("{SELECT_ROW} WHERE is_active = 1 ORDER BY id"),
        &[],
    )
    .await
}

fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m.into_iter().map(|(k, v)| (k, clean(v))).collect(),
        _ => Map::new(),
    }
}

/// `TrimStrings` lalu `ConvertEmptyStringsToNull`, rekursif ke array.
fn clean(v: Value) -> Value {
    match v {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Value::Null
            } else {
                Value::String(t.to_string())
            }
        }
        Value::Array(items) => Value::Array(items.into_iter().map(clean).collect()),
        other => other,
    }
}

fn json_error(e: serde_json::Error) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn attr(field: &str) -> String {
    field.replace('_', " ")
}

/// Field teks. `required` untuk store dan check; `None` bila tidak ada tanpa error.
fn text_field(
    e: &mut Errors,
    input: &Map<String, Value>,
    field: &str,
    required: bool,
    max: Option<usize>,
) -> Option<String> {
    let label = attr(field);
    let missing = format!("The {label} field is required.");
    let not_string = format!("The {label} field must be a string.");
    match input.get(field) {
        None => {
            if required {
                e.add(field, missing);
            }
            None
        }
        Some(Value::Null) => {
            e.add(field, if required { missing } else { not_string });
            None
        }
        Some(Value::String(s)) => match max {
            Some(m) if s.chars().count() > m => {
                e.add(
                    field,
                    format!("The {label} field must not be greater than {m} characters."),
                );
                None
            }
            _ => Some(s.clone()),
        },
        Some(_) => {
            e.add(field, not_string);
            None
        }
    }
}

/// `boolean`: `true`, `false`, `1`, `0`, `"1"`, `"0"`.
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

/// `$request->boolean()`: `filter_var(FILTER_VALIDATE_BOOLEAN, NULL_ON_FAILURE)`.
/// `None` bila tidak dikenali, dan Laravel lalu menulis `IS NULL`.
fn php_bool(raw: &str) -> Option<bool> {
    match raw.to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" | "" => Some(false),
        _ => None,
    }
}

/// `$request->has(k) && $request->input(k)`: string kosong dan "0" dianggap false.
fn truthy(s: Option<&String>) -> Option<&String> {
    s.filter(|v| !v.is_empty() && v.as_str() != "0")
}

/// Input setelah validasi. `None` berarti tidak dikirim.
#[derive(Default)]
struct Input {
    route_path: Option<String>,
    route_method: Option<String>,
    description: Option<Option<String>>,
    allowed_roles: Option<Vec<String>>,
    is_active: Option<bool>,
}

/// Validasi store (`required`) atau update (`sometimes`).
async fn validate(
    pool: &MySqlPool,
    input: &Map<String, Value>,
    required: bool,
) -> Result<Input, ApiError> {
    let mut e = Errors::default();
    let mut out = Input::default();

    out.route_path = text_field(&mut e, input, "route_path", required, Some(255));

    out.route_method = text_field(&mut e, input, "route_method", required, None);
    if let Some(m) = &out.route_method {
        if !METHODS.contains(&m.as_str()) {
            e.add("route_method", "The selected route method is invalid.");
            out.route_method = None;
        }
    }

    match input.get("description") {
        None => {}
        Some(Value::Null) => out.description = Some(None),
        Some(Value::String(s)) => out.description = Some(Some(s.clone())),
        Some(_) => e.add("description", "The description field must be a string."),
    }

    match input.get("allowed_roles") {
        None => {
            if required {
                e.add("allowed_roles", "The allowed roles field is required.");
            }
        }
        Some(Value::Null) => e.add(
            "allowed_roles",
            if required {
                "The allowed roles field is required."
            } else {
                "The allowed roles field must be an array."
            },
        ),
        Some(Value::Array(items)) => {
            if required && items.is_empty() {
                e.add("allowed_roles", "The allowed roles field is required.");
            } else {
                let mut roles = Vec::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    let key = format!("allowed_roles.{i}");
                    match item {
                        Value::String(name) => {
                            let found: i64 = sqlx::query_scalar(
                                "SELECT CAST(COUNT(*) AS SIGNED) FROM roles WHERE name = ?",
                            )
                            .bind(name)
                            .fetch_one(pool)
                            .await
                            .map_err(internal)?;
                            if found == 0 {
                                e.add(&key, format!("The selected {} is invalid.", attr(&key)));
                            }
                            roles.push(name.clone());
                        }
                        _ => e.add(&key, format!("The {} field must be a string.", attr(&key))),
                    }
                }
                out.allowed_roles = Some(roles);
            }
        }
        Some(_) => e.add("allowed_roles", "The allowed roles field must be an array."),
    }

    match input.get("is_active") {
        None => {}
        Some(v) => match bool_value(v) {
            Some(b) => out.is_active = Some(b),
            None => e.add("is_active", "The is active field must be true or false."),
        },
    }

    e.finish()?;
    Ok(out)
}

/// Pasangan (lama, baru) untuk field yang berubah, beserta `updated_at` bila ada perubahan.
fn changed_maps(
    old: &Map<String, Value>,
    new: &Map<String, Value>,
) -> (Map<String, Value>, Map<String, Value>) {
    let mut o = Map::new();
    let mut n = Map::new();
    for f in AUDIT_FIELDS {
        if old.get(f) != new.get(f) {
            o.insert(f.into(), old.get(f).cloned().unwrap_or(Value::Null));
            n.insert(f.into(), new.get(f).cloned().unwrap_or(Value::Null));
        }
    }
    if !o.is_empty() {
        o.insert(
            "updated_at".into(),
            old.get("updated_at").cloned().unwrap_or(Value::Null),
        );
        n.insert(
            "updated_at".into(),
            new.get("updated_at").cloned().unwrap_or(Value::Null),
        );
    }
    (o, n)
}

async fn write_audit(
    tx: &mut sqlx::Transaction<'_, MySql>,
    actor: u64,
    event: &str,
    id: u64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    headers: &HeaderMap,
) -> Result<(), ApiError> {
    let url = format!("{BASE_PATH}/{id}");
    audit::write(
        tx,
        audit::Entry {
            actor,
            event,
            auditable_type: MODEL,
            auditable_id: id,
            old,
            new,
            url: &url,
        },
        headers,
    )
    .await
    .map_err(internal)
}

/// Rule yang membolehkan user: kosong berarti semua boleh (`canAccess`).
fn rule_allows(allowed: &[String], user_roles: &[String]) -> bool {
    allowed.is_empty() || allowed.iter().any(|a| user_roles.contains(a))
}

/// Isi `accessible`: array bila tidak ada yang tersaring, objek ber-kunci indeks asli bila ada.
fn accessible_json(rows: &[RouteRow], user_roles: &[String]) -> String {
    let kept: Vec<(usize, Value)> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| rule_allows(&r.allowed_roles, user_roles))
        .map(|(i, r)| {
            (
                i,
                json!({ "route_path": r.route_path, "route_method": r.route_method }),
            )
        })
        .collect();
    let is_list = kept.iter().enumerate().all(|(k, (i, _))| k == *i);
    if is_list {
        return Value::Array(kept.into_iter().map(|(_, v)| v).collect()).to_string();
    }
    let body: Vec<String> = kept.iter().map(|(i, v)| format!("\"{i}\":{v}")).collect();
    format!("{{{}}}", body.join(","))
}

/// `POST /api/route-permissions/check-access`.
pub async fn check(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();
    let path = text_field(&mut e, &input, "route_path", true, None);
    let method = text_field(&mut e, &input, "route_method", false, None);
    if let Some(m) = &method {
        if !METHODS.contains(&m.as_str()) {
            e.add("route_method", "The selected route method is invalid.");
        }
    }
    e.finish()?;

    let path = path.unwrap_or_default();
    let method = method.unwrap_or_else(|| "GET".to_string());
    let rules = permission::active_rules(&state.pool, &method)
        .await
        .map_err(internal)?;
    let Some(rule) = permission::find_rule(&rules, &path) else {
        return Ok(Json(json!({
            "allowed": true,
            "message": "No restrictions for this route",
        }))
        .into_response());
    };
    let user_roles = permission::user_role_names(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let allowed = rule_allows(&rule.allowed_roles, &user_roles);
    Ok(Json(json!({
        "allowed": allowed,
        "allowed_roles": rule.allowed_roles,
        "user_roles": user_roles,
        "message": if allowed { "Access granted" } else { "Access denied" },
    }))
    .into_response())
}

/// `GET /api/route-permissions/rules`: semua rule aktif (semua method).
pub async fn rules(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let rows = active_all(&state.pool).await?;
    let data: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "route_path": r.route_path,
                "route_method": r.route_method,
                "allowed_roles": r.allowed_roles,
            })
        })
        .collect();
    Ok(Json(Value::Array(data)).into_response())
}

/// `GET /api/route-permissions/user/accessible`.
pub async fn accessible(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let user_roles = permission::user_role_names(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let rows = active_all(&state.pool).await?;
    let body = accessible_json(&rows, &user_roles);
    Ok(([(header::CONTENT_TYPE, "application/json")], body).into_response())
}

/// `GET /api/route-permissions`: paginasi 15, atau semua bila `per_page=-1`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;

    let mut parts: Vec<&str> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(s) = truthy(query.get("search")) {
        parts.push("(route_path LIKE ? OR description LIKE ?)");
        binds.push(format!("%{s}%"));
        binds.push(format!("%{s}%"));
    }
    if let Some(m) = truthy(query.get("method")) {
        parts.push("route_method = ?");
        binds.push(m.to_ascii_uppercase());
    }
    if let Some(raw) = query.get("is_active") {
        match php_bool(raw) {
            Some(b) => {
                parts.push("is_active = ?");
                binds.push(if b { "1" } else { "0" }.to_string());
            }
            None => parts.push("is_active IS NULL"),
        }
    }
    let where_sql = if parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", parts.join(" AND "))
    };

    let all = query
        .get("per_page")
        .filter(|s| !s.is_empty())
        .and_then(|s| s.trim().parse::<f64>().ok())
        == Some(-1.0);
    if all {
        let rows = query_rows(
            &state.pool,
            &format!("{SELECT_ROW}{where_sql} ORDER BY route_path, id"),
            &binds,
        )
        .await?;
        let data: Vec<Value> = rows.iter().map(|r| Value::Object(row_json(r))).collect();
        return Ok(Json(Value::Array(data)).into_response());
    }

    let params: PageParams = pagination::page_params(&query);
    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM route_permissions{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b);
    }
    let total = cq.fetch_one(&state.pool).await.map_err(internal)?;

    let sql = format!(
        "{SELECT_ROW}{where_sql} ORDER BY route_path, id LIMIT {} OFFSET {}",
        params.per_page,
        (params.page - 1) * params.per_page
    );
    let rows = query_rows(&state.pool, &sql, &binds).await?;
    let data: Vec<Value> = rows.iter().map(|r| Value::Object(row_json(r))).collect();
    let base = format!("{}{BASE_PATH}", state.app_url.trim_end_matches('/'));
    Ok(Json(pagination::paginate(data, total as u64, params, &base)).into_response())
}

/// `POST /api/route-permissions`: 201. Duplikat `route_path` + `route_method` ditolak dengan 422.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let v = validate(&state.pool, &input, true).await?;
    let route_path = v.route_path.unwrap_or_default();
    let route_method = v.route_method.unwrap_or_default();
    let allowed_roles = v.allowed_roles.unwrap_or_default();
    let description = v.description.flatten();
    let is_active = v.is_active.unwrap_or(true);

    let exists: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM route_permissions WHERE route_path = ? AND route_method = ?",
    )
    .bind(&route_path)
    .bind(&route_method)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;
    if exists > 0 {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Route permission already exists for this path and method",
        ));
    }

    let roles_json = serde_json::to_string(&allowed_roles).map_err(json_error)?;
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let id = sqlx::query(
        "INSERT INTO route_permissions (route_path, route_method, description, allowed_roles, is_active, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(&route_path)
    .bind(&route_method)
    .bind(&description)
    .bind(&roles_json)
    .bind(is_active)
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .last_insert_id();
    let row = load_tx(&mut tx, id).await?;

    // Model hasil `create()` hanya memuat kolom yang dikirim, plus id dan timestamp.
    let mut out = row_json(&row);
    if !input.contains_key("description") {
        out.remove("description");
    }
    if !input.contains_key("is_active") {
        out.remove("is_active");
    }
    write_audit(
        &mut tx,
        user.user_id,
        "created",
        id,
        None,
        Some(out.clone()),
        &headers,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    Ok((StatusCode::CREATED, Json(Value::Object(out))).into_response())
}

/// `GET /api/route-permissions/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let row = load_or_404(&state.pool, &id).await?;
    Ok(Json(Value::Object(row_json(&row))).into_response())
}

/// `PUT` dan `PATCH /api/route-permissions/{id}`. Tanpa cek duplikat, seperti Laravel.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let current = load_or_404(&state.pool, &id).await?;
    let input = parse_body(&body);
    let v = validate(&state.pool, &input, false).await?;

    let mut next = current.clone();
    if let Some(p) = v.route_path {
        next.route_path = p;
    }
    if let Some(m) = v.route_method {
        next.route_method = m;
    }
    if let Some(d) = v.description {
        next.description = d;
    }
    if let Some(r) = v.allowed_roles {
        next.allowed_roles = r;
    }
    if let Some(a) = v.is_active {
        next.is_active = a;
    }

    let before = row_json(&current);
    let proposed = row_json(&next);
    let (changed_old, _) = changed_maps(&before, &proposed);
    if changed_old.is_empty() {
        return Ok(Json(Value::Object(before)).into_response());
    }

    let roles_json = serde_json::to_string(&next.allowed_roles).map_err(json_error)?;
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query(
        "UPDATE route_permissions SET route_path = ?, route_method = ?, description = ?, \
         allowed_roles = ?, is_active = ?, updated_at = NOW() WHERE id = ?",
    )
    .bind(&next.route_path)
    .bind(&next.route_method)
    .bind(&next.description)
    .bind(&roles_json)
    .bind(next.is_active)
    .bind(current.id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let after = load_tx(&mut tx, current.id).await?;
    let after_json = row_json(&after);
    let (old, new) = changed_maps(&before, &after_json);
    write_audit(
        &mut tx,
        user.user_id,
        "updated",
        current.id,
        Some(old),
        Some(new),
        &headers,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    Ok(Json(Value::Object(after_json)).into_response())
}

/// `DELETE /api/route-permissions/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let current = load_or_404(&state.pool, &id).await?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM route_permissions WHERE id = ?")
        .bind(current.id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    write_audit(
        &mut tx,
        user.user_id,
        "deleted",
        current.id,
        Some(row_json(&current)),
        None,
        &headers,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    Ok((
        StatusCode::OK,
        Json(json!({ "message": "Route permission deleted" })),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u64, roles: &[&str]) -> RouteRow {
        RouteRow {
            id,
            route_path: format!("/uji-rp-{id}"),
            route_method: "GET".into(),
            description: None,
            allowed_roles: roles.iter().map(|s| s.to_string()).collect(),
            is_active: true,
            created_at: None,
            updated_at: None,
        }
    }

    fn roles(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn accessible_is_list_when_nothing_filtered() {
        let rows = [row(1, &["pengawas"]), row(2, &[])];
        let body = accessible_json(&rows, &roles(&["pengawas"]));
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!([
                {"route_path": "/uji-rp-1", "route_method": "GET"},
                {"route_path": "/uji-rp-2", "route_method": "GET"}
            ])
        );
    }

    #[test]
    fn accessible_is_keyed_object_when_filtered() {
        let rows = [row(1, &["admin"]), row(2, &[]), row(3, &["pengawas"])];
        let body = accessible_json(&rows, &roles(&["pengawas"]));
        // Kunci memakai indeks asli (1 dan 2), bukan 0 dan 1.
        assert!(body.starts_with(r#"{"1":"#), "{body}");
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({
                "1": {"route_path": "/uji-rp-2", "route_method": "GET"},
                "2": {"route_path": "/uji-rp-3", "route_method": "GET"}
            })
        );
    }

    #[test]
    fn accessible_empty_is_array() {
        let rows = [row(1, &["admin"])];
        assert_eq!(accessible_json(&rows, &roles(&["tamu"])), "[]");
    }

    #[test]
    fn clean_trims_and_nulls_empty_strings() {
        assert_eq!(clean(json!("  /x  ")), json!("/x"));
        assert_eq!(clean(json!("   ")), Value::Null);
        assert_eq!(clean(json!(["a", " "])), json!(["a", null]));
    }

    #[test]
    fn php_boolean_matches_filter_var() {
        assert_eq!(php_bool("1"), Some(true));
        assert_eq!(php_bool("On"), Some(true));
        assert_eq!(php_bool(""), Some(false));
        assert_eq!(php_bool("no"), Some(false));
        assert_eq!(php_bool("maybe"), None);
    }

    #[test]
    fn truthy_skips_zero_and_empty() {
        assert!(truthy(Some(&"0".to_string())).is_none());
        assert!(truthy(Some(&String::new())).is_none());
        assert!(truthy(Some(&"x".to_string())).is_some());
    }

    #[test]
    fn changed_maps_only_lists_dirty_fields() {
        let old = row_json(&row(1, &["admin"]));
        let mut new = old.clone();
        new.insert("is_active".into(), json!(false));
        let (o, n) = changed_maps(&old, &new);
        assert_eq!(o.get("is_active"), Some(&json!(true)));
        assert_eq!(n.get("is_active"), Some(&json!(false)));
        assert!(!o.contains_key("route_path"));
        assert!(o.contains_key("updated_at"));
        let (o2, _) = changed_maps(&old, &old);
        assert!(o2.is_empty());
    }
}
