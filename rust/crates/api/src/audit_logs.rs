//! Log audit untuk admin (`AuditLogController` dan `AuditLogResource`). Tabel `tbl_audit_logs`.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    lookup::carbon_json, notifications::require_admin, pagination, require_auth, AppState,
};

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

const SELECT_LOGS: &str =
    "SELECT CAST(a.id AS SIGNED) AS id, CAST(a.user_id AS SIGNED) AS user_id, \
     a.event, a.auditable_type, CAST(a.auditable_id AS SIGNED) AS auditable_id, \
     CAST(a.old_values AS CHAR) AS old_values, CAST(a.new_values AS CHAR) AS new_values, \
     a.url, a.ip_address, a.user_agent, a.created_at, a.updated_at \
     FROM tbl_audit_logs a";

struct AuditRow {
    id: i64,
    user_id: Option<i64>,
    event: String,
    auditable_type: String,
    auditable_id: i64,
    old_values: Option<String>,
    new_values: Option<String>,
    url: Option<String>,
    ip_address: Option<String>,
    user_agent: Option<String>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<AuditRow, sqlx::Error> {
    Ok(AuditRow {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        event: r.try_get("event")?,
        auditable_type: r.try_get("auditable_type")?,
        auditable_id: r.try_get("auditable_id")?,
        old_values: r.try_get("old_values")?,
        new_values: r.try_get("new_values")?,
        url: r.try_get("url")?,
        ip_address: r.try_get("ip_address")?,
        user_agent: r.try_get("user_agent")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

/// Kolom `array` di Laravel: JSON didekode. Objek kosong menjadi `[]`, seperti PHP.
fn values_json(raw: &Option<String>) -> Value {
    match raw.as_deref().map(serde_json::from_str::<Value>) {
        Some(Ok(Value::Object(map))) if map.is_empty() => Value::Array(Vec::new()),
        Some(Ok(v)) => v,
        _ => Value::Null,
    }
}

/// `(int)` PHP untuk nilai JSON: angka, atau awalan digit dari string.
fn php_int(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(0),
        Value::String(s) => {
            let t = s.trim_start();
            let bytes = t.as_bytes();
            let mut end = 0;
            if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
                end += 1;
            }
            let start = end;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end == start {
                0
            } else {
                t[..end].parse().unwrap_or(0)
            }
        }
        Value::Bool(b) => i64::from(*b),
        _ => 0,
    }
}

/// Truthy PHP untuk nilai JSON: tidak null, bukan `""`, `"0"`, `0`, atau `false`.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty() && s != "0",
        _ => true,
    }
}

/// `AuditLogResource::resolvePekerjaanId`. Nol dianggap tidak ada.
fn pekerjaan_id(row: &AuditRow, values: &Value) -> Option<i64> {
    let kind = row.auditable_type.rsplit('\\').next().unwrap_or("");
    let id = match kind {
        "Pekerjaan" => row.auditable_id,
        "Kontrak" => values
            .get("id_pekerjaan")
            .filter(|v| !v.is_null())
            .map(php_int)?,
        "Output" | "Penerima" | "Foto" | "Berkas" | "Progress" => values
            .get("pekerjaan_id")
            .filter(|v| !v.is_null())
            .map(php_int)?,
        _ => return None,
    };
    (id != 0).then_some(id)
}

fn pekerjaan_tab(kind: &str) -> Value {
    match kind {
        "Kontrak" => json!("kontrak"),
        "Output" => json!("output"),
        "Penerima" => json!("penerima"),
        "Foto" => json!("foto"),
        "Berkas" => json!("berkas"),
        "Progress" => json!("progress"),
        _ => Value::Null,
    }
}

/// `$this->new_values ?? $this->old_values ?? []` untuk konteks pekerjaan.
fn context_values(row: &AuditRow) -> Value {
    let new = values_json(&row.new_values);
    let values = if new.is_null() {
        values_json(&row.old_values)
    } else {
        new
    };
    if values.is_null() {
        json!({})
    } else {
        values
    }
}

/// Bentuk `AuditLogResource`. `users` dan `names` berisi relasi yang sudah dimuat untuk halaman ini.
fn resource(row: &AuditRow, users: &HashMap<i64, Value>, names: &HashMap<i64, String>) -> Value {
    let old = values_json(&row.old_values);
    let new = values_json(&row.new_values);
    let values = context_values(row);

    let pekerjaan = pekerjaan_id(row, &values).map(|id| {
        let kind = row.auditable_type.rsplit('\\').next().unwrap_or("");
        let nama = match values.get("nama_paket") {
            Some(v) if truthy(v) => v.clone(),
            _ => names.get(&id).map(|n| json!(n)).unwrap_or(Value::Null),
        };
        json!({ "id": id, "nama_paket": nama, "tab": pekerjaan_tab(kind) })
    });

    let user = row
        .user_id
        .and_then(|uid| users.get(&uid).cloned())
        .unwrap_or(Value::Null);

    json!({
        "id": row.id,
        "user_id": row.user_id,
        "event": row.event,
        "auditable_type": row.auditable_type,
        "auditable_id": row.auditable_id,
        "old_values": old,
        "new_values": new,
        "url": row.url,
        "ip_address": row.ip_address,
        "user_agent": row.user_agent,
        "created_at": carbon_json(row.created_at),
        "updated_at": carbon_json(row.updated_at),
        "user": user,
        "pekerjaan": pekerjaan.unwrap_or(Value::Null),
    })
}

/// Pengguna dan nama pekerjaan untuk sekumpulan log (`with('user')` dan `pluck('nama_paket', 'id')`).
async fn related(
    pool: &MySqlPool,
    rows: &[AuditRow],
) -> Result<(HashMap<i64, Value>, HashMap<i64, String>), ApiError> {
    let user_ids: Vec<i64> = rows.iter().filter_map(|r| r.user_id).collect();
    let pekerjaan_ids: Vec<i64> = rows
        .iter()
        .filter_map(|r| pekerjaan_id(r, &context_values(r)))
        .collect();

    let mut users = HashMap::new();
    if !user_ids.is_empty() {
        let sql = format!(
            "SELECT CAST(id AS SIGNED) AS id, name, email, avatar, gender FROM users WHERE id IN ({})",
            vec!["?"; user_ids.len()].join(", ")
        );
        let mut q = sqlx::query(&sql);
        for id in &user_ids {
            q = q.bind(id);
        }
        for r in q.fetch_all(pool).await.map_err(internal)? {
            let id: i64 = r.try_get("id").map_err(internal)?;
            users.insert(
                id,
                json!({
                    "id": id,
                    "name": r.try_get::<String, _>("name").map_err(internal)?,
                    "email": r.try_get::<String, _>("email").map_err(internal)?,
                    "avatar": r.try_get::<Option<String>, _>("avatar").map_err(internal)?,
                    "gender": r.try_get::<Option<String>, _>("gender").map_err(internal)?,
                }),
            );
        }
    }

    let mut names = HashMap::new();
    if !pekerjaan_ids.is_empty() {
        let sql = format!(
            "SELECT CAST(id AS SIGNED) AS id, nama_paket FROM tbl_pekerjaan WHERE id IN ({})",
            vec!["?"; pekerjaan_ids.len()].join(", ")
        );
        let mut q = sqlx::query(&sql);
        for id in &pekerjaan_ids {
            q = q.bind(id);
        }
        for r in q.fetch_all(pool).await.map_err(internal)? {
            let id: i64 = r.try_get("id").map_err(internal)?;
            if let Some(nama) = r
                .try_get::<Option<String>, _>("nama_paket")
                .map_err(internal)?
            {
                names.insert(id, nama);
            }
        }
    }
    Ok((users, names))
}

/// `GET /api/audit-logs`: admin. Filter `type` (mengandung), `event`, dan `user_id`. Kosong = tidak difilter.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let params = pagination::page_params(&query);

    // `$request->has()` bernilai false untuk string kosong.
    let filter = |key: &str| query.get(key).filter(|v| !v.is_empty()).cloned();
    let mut clauses: Vec<&str> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(t) = filter("type") {
        clauses.push("a.auditable_type LIKE ?");
        binds.push(format!("%{t}%"));
    }
    if let Some(e) = filter("event") {
        clauses.push("a.event = ?");
        binds.push(e);
    }
    if let Some(u) = filter("user_id") {
        clauses.push("a.user_id = ?");
        binds.push(u);
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM tbl_audit_logs a{where_sql}");
    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        count_q = count_q.bind(b);
    }
    let total = count_q.fetch_one(&state.pool).await.map_err(internal)?;

    let offset = (params.page - 1) * params.per_page;
    let list_sql = format!(
        "{SELECT_LOGS}{where_sql} ORDER BY a.id DESC LIMIT {} OFFSET {}",
        params.per_page, offset
    );
    let mut list_q = sqlx::query(&list_sql);
    for b in &binds {
        list_q = list_q.bind(b);
    }
    let rows: Vec<AuditRow> = list_q
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?
        .iter()
        .map(map_row)
        .collect::<Result<_, _>>()
        .map_err(internal)?;

    let (users, names) = related(&state.pool, &rows).await?;
    let data: Vec<Value> = rows.iter().map(|r| resource(r, &users, &names)).collect();
    let total = total as u64;
    // Bentuk `meta` tanpa `links`, seperti `AuditLogController::index`.
    let last_page = total.div_ceil(params.per_page).max(1);
    Ok(Json(json!({
        "success": true,
        "data": data,
        "meta": {
            "current_page": params.page,
            "last_page": last_page,
            "per_page": params.per_page,
            "total": total,
        },
    }))
    .into_response())
}

/// `GET /api/audit-logs/{id}`: admin. Detail satu log.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    require_admin(&state.pool, user.user_id).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let sql = format!("{SELECT_LOGS} WHERE a.id = ?");
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?
        .map(|r| map_row(&r))
        .transpose()
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let (users, names) = related(&state.pool, std::slice::from_ref(&row)).await?;
    Ok(Json(json!({ "success": true, "data": resource(&row, &users, &names) })).into_response())
}
