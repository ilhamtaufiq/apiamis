//! Tulis user: `POST /api/users`, `PUT`/`PATCH`/`DELETE /api/users/{id}`.
//!
//! Mengikuti `UserController` dan model `User` (`Auditable`). Password di-hash bcrypt dengan
//! prefiks `$2y$` seperti `bcrypt()` Laravel. `roles` dan `permissions` disinkron seperti Spatie
//! (`syncRoles`/`syncPermissions`, guard `web`).

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, Row, Transaction};

use crate::{audit, foto, lookup::carbon_json, require_auth, users, AppState};

const MODEL: &str = "App\\Models\\User";
const PROTECTED_EMAILS: [&str; 1] = ["ilhamtaufiq@gmail.com"];
const GENDERS: [&str; 3] = ["male", "female", "other"];

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn text(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(Value::String(s)) => Some(s.trim().to_string()),
        _ => None,
    }
}

/// Aturan `email` yang disederhanakan: satu `@`, bagian lokal dan domain tidak kosong, domain punya titik.
fn looks_like_email(s: &str) -> bool {
    let mut parts = s.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && !local.contains(' ')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains(' ')
}

/// `$2y$` seperti `bcrypt()` Laravel. Algoritmanya sama dengan `$2b$` untuk input ASCII.
fn hash_password(plain: &str) -> Result<String, ApiError> {
    let hashed = bcrypt::hash(plain, bcrypt::DEFAULT_COST).map_err(internal)?;
    Ok(hashed.replacen("$2b$", "$2y$", 1))
}

/// Nilai `roles`/`permissions`: nama (string) atau id (angka).
fn name_or_id(v: &Value) -> Option<(bool, String)> {
    match v {
        Value::Number(n) => n.as_i64().map(|i| (true, i.to_string())),
        Value::String(s) if !s.trim().is_empty() => Some((false, s.trim().to_string())),
        _ => None,
    }
}

async fn resolve_role_ids(pool: &sqlx::MySqlPool, items: &[Value]) -> Result<Vec<u64>, ApiError> {
    let mut ids = Vec::new();
    for item in items {
        let Some((is_id, key)) = name_or_id(item) else {
            return Err(internal("roles harus berisi nama atau id"));
        };
        let row: Option<i64> = if is_id {
            sqlx::query_scalar(
                "SELECT CAST(id AS SIGNED) FROM roles WHERE id = ? AND guard_name = 'web'",
            )
            .bind(key.parse::<i64>().unwrap_or(0))
            .fetch_optional(pool)
            .await
            .map_err(internal)?
        } else {
            sqlx::query_scalar(
                "SELECT CAST(id AS SIGNED) FROM roles WHERE name = ? AND guard_name = 'web'",
            )
            .bind(&key)
            .fetch_optional(pool)
            .await
            .map_err(internal)?
        };
        // Spatie melempar `RoleDoesNotExist`: Laravel menjawab 500.
        let id = row
            .ok_or_else(|| internal(format!("There is no role named `{key}` for guard `web`.")))?;
        ids.push(id as u64);
    }
    Ok(ids)
}

async fn resolve_permission_ids(
    pool: &sqlx::MySqlPool,
    items: &[Value],
) -> Result<Vec<u64>, ApiError> {
    let mut ids = Vec::new();
    for item in items {
        let Some((is_id, key)) = name_or_id(item) else {
            return Err(internal("permissions harus berisi nama atau id"));
        };
        let row: Option<i64> = if is_id {
            sqlx::query_scalar(
                "SELECT CAST(id AS SIGNED) FROM permissions WHERE id = ? AND guard_name = 'web'",
            )
            .bind(key.parse::<i64>().unwrap_or(0))
            .fetch_optional(pool)
            .await
            .map_err(internal)?
        } else {
            sqlx::query_scalar(
                "SELECT CAST(id AS SIGNED) FROM permissions WHERE name = ? AND guard_name = 'web'",
            )
            .bind(&key)
            .fetch_optional(pool)
            .await
            .map_err(internal)?
        };
        let id = row.ok_or_else(|| {
            internal(format!(
                "There is no permission named `{key}` for guard `web`."
            ))
        })?;
        ids.push(id as u64);
    }
    Ok(ids)
}

/// `syncRoles` dan `syncPermissions`: pivot diganti sesuai daftar baru.
async fn sync_pivots(
    tx: &mut Transaction<'_, MySql>,
    user_id: u64,
    role_ids: Option<&[u64]>,
    perm_ids: Option<&[u64]>,
) -> Result<(), ApiError> {
    if let Some(ids) = role_ids {
        sqlx::query("DELETE FROM model_has_roles WHERE model_type = ? AND model_id = ?")
            .bind(MODEL)
            .bind(user_id)
            .execute(&mut **tx)
            .await
            .map_err(internal)?;
        for rid in ids {
            sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, ?, ?)")
                .bind(rid)
                .bind(MODEL)
                .bind(user_id)
                .execute(&mut **tx)
                .await
                .map_err(internal)?;
        }
    }
    if let Some(ids) = perm_ids {
        sqlx::query("DELETE FROM model_has_permissions WHERE model_type = ? AND model_id = ?")
            .bind(MODEL)
            .bind(user_id)
            .execute(&mut **tx)
            .await
            .map_err(internal)?;
        for pid in ids {
            sqlx::query("INSERT IGNORE INTO model_has_permissions (permission_id, model_type, model_id) VALUES (?, ?, ?)")
                .bind(pid)
                .bind(MODEL)
                .bind(user_id)
                .execute(&mut **tx)
                .await
                .map_err(internal)?;
        }
    }
    Ok(())
}

/// Atribut audit user. Password dan remember_token tidak ikut (`$hidden`).
async fn attributes<'e, E>(exec: E, id: u64) -> Result<Option<Map<String, Value>>, ApiError>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED), name, email, avatar, gender, nip, jabatan, created_at, updated_at FROM users WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(exec)
    .await
    .map_err(internal)?;
    let Some(r) = row else {
        return Ok(None);
    };
    let created: Option<chrono::DateTime<chrono::Utc>> = r.try_get(7).map_err(internal)?;
    let updated: Option<chrono::DateTime<chrono::Utc>> = r.try_get(8).map_err(internal)?;
    let mut m = Map::new();
    m.insert(
        "id".into(),
        json!(r.try_get::<i64, _>(0).map_err(internal)?),
    );
    m.insert(
        "name".into(),
        json!(r.try_get::<String, _>(1).map_err(internal)?),
    );
    m.insert(
        "email".into(),
        json!(r.try_get::<String, _>(2).map_err(internal)?),
    );
    m.insert(
        "avatar".into(),
        json!(r.try_get::<Option<String>, _>(3).map_err(internal)?),
    );
    m.insert(
        "gender".into(),
        json!(r.try_get::<Option<String>, _>(4).map_err(internal)?),
    );
    m.insert(
        "nip".into(),
        json!(r.try_get::<Option<String>, _>(5).map_err(internal)?),
    );
    m.insert(
        "jabatan".into(),
        json!(r.try_get::<Option<String>, _>(6).map_err(internal)?),
    );
    m.insert("created_at".into(), carbon_json(created));
    m.insert("updated_at".into(), carbon_json(updated));
    Ok(Some(m))
}

fn validate_gender(
    v: Option<&Value>,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<Option<String>> {
    match text(v) {
        None => Some(None),
        Some(g) if GENDERS.contains(&g.as_str()) => Some(Some(g)),
        Some(_) => {
            foto::add(errs, "gender", "The selected gender is invalid.".into());
            None
        }
    }
}

fn array_of(
    body: &Value,
    key: &str,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<Vec<Value>> {
    match body.get(key) {
        None => None,
        Some(Value::Array(a)) => Some(a.clone()),
        Some(Value::Null) => None,
        Some(_) => {
            foto::add(errs, key, format!("The {} field must be an array.", key));
            None
        }
    }
}

/// `POST /api/users`: 201 dengan `UserResource` (roles dan permissions dimuat).
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let actor = require_auth(&state, &headers).await?;
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let name = text(body.get("name"));
    if name.is_none() {
        foto::add(&mut errs, "name", "The name field is required.".into());
    } else if name.as_deref().is_some_and(|n| n.chars().count() > 255) {
        foto::add(
            &mut errs,
            "name",
            "The name field must not be greater than 255 characters.".into(),
        );
    }
    let email = text(body.get("email"));
    match &email {
        None => foto::add(&mut errs, "email", "The email field is required.".into()),
        Some(e) if !looks_like_email(e) => foto::add(
            &mut errs,
            "email",
            "The email field must be a valid email address.".into(),
        ),
        Some(e) => {
            let taken: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = ?")
                .bind(e)
                .fetch_one(&state.pool)
                .await
                .map_err(internal)?;
            if taken > 0 {
                foto::add(
                    &mut errs,
                    "email",
                    "The email has already been taken.".into(),
                );
            }
        }
    }
    let password = text(body.get("password"));
    match &password {
        None => foto::add(
            &mut errs,
            "password",
            "The password field is required.".into(),
        ),
        Some(p) if p.chars().count() < 6 => foto::add(
            &mut errs,
            "password",
            "The password field must be at least 6 characters.".into(),
        ),
        Some(_) => {}
    }
    if body
        .get("nip")
        .is_some_and(|v| text(Some(v)).is_some_and(|s| s.chars().count() > 50))
    {
        foto::add(
            &mut errs,
            "nip",
            "The nip field must not be greater than 50 characters.".into(),
        );
    }
    if body
        .get("jabatan")
        .is_some_and(|v| text(Some(v)).is_some_and(|s| s.chars().count() > 255))
    {
        foto::add(
            &mut errs,
            "jabatan",
            "The jabatan field must not be greater than 255 characters.".into(),
        );
    }
    let gender = validate_gender(body.get("gender"), &mut errs);
    let roles = array_of(&body, "roles", &mut errs);
    let permissions = array_of(&body, "permissions", &mut errs);
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    let (Some(name), Some(email), Some(password), Some(gender)) = (name, email, password, gender)
    else {
        return Err(internal("validasi user tidak lengkap"));
    };
    let role_ids = match &roles {
        Some(items) => Some(resolve_role_ids(&state.pool, items).await?),
        None => None,
    };
    let perm_ids = match &permissions {
        Some(items) => Some(resolve_permission_ids(&state.pool, items).await?),
        None => None,
    };
    let hashed = hash_password(&password)?;
    let nip = text(body.get("nip"));
    let jabatan = text(body.get("jabatan"));

    let url = format!("{}/api/users", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query("INSERT INTO users (name, email, password, gender, nip, jabatan, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, NOW(), NOW())")
        .bind(name)
        .bind(email)
        .bind(hashed)
        .bind(gender)
        .bind(nip)
        .bind(jabatan)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let id = res.last_insert_id();
    sync_pivots(&mut tx, id, role_ids.as_deref(), perm_ids.as_deref()).await?;
    let created = attributes(&mut *tx, id)
        .await?
        .ok_or_else(|| internal("user baru tidak terbaca"))?;
    audit::write(
        &mut tx,
        audit::Entry {
            actor: actor.user_id,
            event: "created",
            auditable_type: MODEL,
            auditable_id: id,
            old: None,
            new: Some(created),
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    let out = users::resource(&state.pool, &state.app_url, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok((StatusCode::CREATED, Json(out)).into_response())
}

/// `PUT` dan `PATCH /api/users/{id}`. Kunci yang tidak dikirim tidak diubah.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let actor = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let before = attributes(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let mut sets: Vec<(&'static str, Option<String>)> = Vec::new();
    if let Some(v) = body.get("name").filter(|v| !v.is_null()) {
        match text(Some(v)) {
            None => foto::add(&mut errs, "name", "The name field must be a string.".into()),
            Some(n) if n.chars().count() > 255 => foto::add(
                &mut errs,
                "name",
                "The name field must not be greater than 255 characters.".into(),
            ),
            Some(n) => sets.push(("name", Some(n))),
        }
    }
    if let Some(v) = body.get("email").filter(|v| !v.is_null()) {
        match text(Some(v)) {
            Some(e) if looks_like_email(&e) => {
                let taken: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = ? AND id <> ?")
                        .bind(&e)
                        .bind(id)
                        .fetch_one(&state.pool)
                        .await
                        .map_err(internal)?;
                if taken > 0 {
                    foto::add(
                        &mut errs,
                        "email",
                        "The email has already been taken.".into(),
                    );
                } else {
                    sets.push(("email", Some(e)));
                }
            }
            _ => foto::add(
                &mut errs,
                "email",
                "The email field must be a valid email address.".into(),
            ),
        }
    }
    // `password` kosong dilewati (`!empty`), panjang minimal 6 bila diisi.
    let mut new_password: Option<String> = None;
    if let Some(p) = text(body.get("password")) {
        if p.chars().count() < 6 {
            foto::add(
                &mut errs,
                "password",
                "The password field must be at least 6 characters.".into(),
            );
        } else {
            new_password = Some(hash_password(&p)?);
        }
    }
    for key in ["nip", "jabatan"] {
        if let Some(v) = body.get(key) {
            let t = text(Some(v));
            let max = if key == "nip" { 50 } else { 255 };
            match t {
                Some(s) if s.chars().count() > max => foto::add(
                    &mut errs,
                    key,
                    format!("The {key} field must not be greater than {max} characters."),
                ),
                other => sets.push((if key == "nip" { "nip" } else { "jabatan" }, other)),
            }
        }
    }
    if body.get("gender").is_some() {
        if let Some(g) = validate_gender(body.get("gender"), &mut errs) {
            sets.push(("gender", g));
        }
    }
    let roles = array_of(&body, "roles", &mut errs);
    let permissions = array_of(&body, "permissions", &mut errs);
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    let role_ids = match &roles {
        Some(items) => Some(resolve_role_ids(&state.pool, items).await?),
        None => None,
    };
    let perm_ids = match &permissions {
        Some(items) => Some(resolve_permission_ids(&state.pool, items).await?),
        None => None,
    };

    let url = format!("{}/api/users/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    if !sets.is_empty() || new_password.is_some() {
        let mut cols: Vec<String> = sets.iter().map(|(k, _)| format!("{k} = ?")).collect();
        if new_password.is_some() {
            cols.push("password = ?".into());
        }
        let sql = format!(
            "UPDATE users SET {}, updated_at = NOW() WHERE id = ?",
            cols.join(", ")
        );
        let mut q = sqlx::query(&sql);
        for (_, v) in &sets {
            q = q.bind(v.clone());
        }
        if let Some(p) = &new_password {
            q = q.bind(p.clone());
        }
        q.bind(id).execute(&mut *tx).await.map_err(internal)?;
    }
    sync_pivots(&mut tx, id, role_ids.as_deref(), perm_ids.as_deref()).await?;
    let after = attributes(&mut *tx, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut old = Map::new();
    let mut new = Map::new();
    for (k, v) in &after {
        if k == "updated_at" {
            continue;
        }
        if before.get(k) != Some(v) {
            old.insert(k.clone(), before.get(k).cloned().unwrap_or(Value::Null));
            new.insert(k.clone(), v.clone());
        }
    }
    if !new.is_empty() {
        old.insert(
            "updated_at".into(),
            before.get("updated_at").cloned().unwrap_or(Value::Null),
        );
        new.insert(
            "updated_at".into(),
            after.get("updated_at").cloned().unwrap_or(Value::Null),
        );
        audit::write(
            &mut tx,
            audit::Entry {
                actor: actor.user_id,
                event: "updated",
                auditable_type: MODEL,
                auditable_id: id,
                old: Some(old),
                new: Some(new),
                url: &url,
            },
            &headers,
        )
        .await
        .map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;
    let out = users::resource(&state.pool, &state.app_url, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(out))
}

/// `DELETE /api/users/{id}`. Akun yang dilindungi ditolak 403.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let actor = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let before = attributes(&state.pool, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let email = before
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if PROTECTED_EMAILS.contains(&email.as_str()) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Akun ini dilindungi dan tidak dapat dihapus.",
        ));
    }
    let url = format!("{}/api/users/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sync_pivots(&mut tx, id, Some(&[]), Some(&[])).await?;
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    audit::write(
        &mut tx,
        audit::Entry {
            actor: actor.user_id,
            event: "deleted",
            auditable_type: MODEL,
            auditable_id: id,
            old: Some(before),
            new: None,
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "message": "User deleted" })))
}

/// `GET /api/users`: paginasi (default 15), pencarian nama, email, nip, jabatan, atau nama role.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let per_page = query
        .get("per_page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(15);
    let page = query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1);
    let search = query
        .get("search")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut where_sql = String::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(s) = &search {
        let like = format!("%{s}%");
        where_sql.push_str(
            " WHERE (name LIKE ? OR email LIKE ? OR nip LIKE ? OR jabatan LIKE ? \
             OR id IN (SELECT mr.model_id FROM model_has_roles mr JOIN roles r ON r.id = mr.role_id \
             WHERE mr.model_type = 'App\\\\Models\\\\User' AND r.name LIKE ?))",
        );
        binds.extend(std::iter::repeat_n(like, 5));
    }
    let count_sql = format!("SELECT COUNT(*) FROM users{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b);
    }
    let total = cq.fetch_one(&state.pool).await.map_err(internal)?;

    let list_sql =
        format!("SELECT CAST(id AS SIGNED) FROM users{where_sql} ORDER BY id LIMIT ? OFFSET ?");
    let mut lq = sqlx::query_scalar::<_, i64>(&list_sql);
    for b in &binds {
        lq = lq.bind(b);
    }
    let ids: Vec<i64> = lq
        .bind(per_page as i64)
        .bind(((page - 1) * per_page) as i64)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let mut data = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(v) = users::resource(&state.pool, &state.app_url, id as u64)
            .await
            .map_err(internal)?
        {
            data.push(v);
        }
    }
    let base = format!("{}/api/users", state.app_url.trim_end_matches('/'));
    Ok(Json(crate::pagination::paginate_with_query(
        data,
        total as u64,
        crate::pagination::PageParams { page, per_page },
        &base,
        "",
    )))
}

/// `GET /api/users/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let v = users::resource(&state.pool, &state.app_url, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": v })))
}
