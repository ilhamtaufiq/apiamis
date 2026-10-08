//! Tulis item checklist (`ChecklistItemController`): buat, ubah, hapus, dan urutkan ulang.
//! Baca (`index`, `show`) ada di `checklist.rs`.
//!
//! `store` memakai `ChecklistItemResource` tanpa `->response()->setStatusCode(201)`, jadi Laravel
//! menjawab 200. Bentuk respons memakai `{"data": ...}` seperti `JsonResource`.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::MySqlPool;

use crate::{
    checklist::{item_find, item_resource},
    require_auth,
    validation::Errors,
    AppState,
};

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

const CONTEXTS: [&str; 2] = ["pekerjaan", "post_pekerjaan"];

fn parse_body(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn attr(field: &str) -> String {
    field.replace('_', " ")
}

/// `string|max:N` dengan pesan Laravel. `None` bila tidak valid (error sudah dicatat).
fn string_max(e: &mut Errors, field: &str, v: &Value, max: usize) -> Option<String> {
    match v {
        Value::String(s) if s.chars().count() <= max => Some(s.clone()),
        Value::String(_) => {
            e.add(field, format!("The {} field must not be greater than {max} characters.", attr(field)));
            None
        }
        _ => {
            e.add(field, format!("The {} field must be a string.", attr(field)));
            None
        }
    }
}

/// `integer|min:0` dari angka JSON atau string angka.
fn int_min0(e: &mut Errors, field: &str, v: &Value) -> Option<i64> {
    let parsed = match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    };
    match parsed {
        None => {
            e.add(field, format!("The {} field must be an integer.", attr(field)));
            None
        }
        Some(v) if v < 0 => {
            e.add(field, format!("The {} field must be at least 0.", attr(field)));
            None
        }
        Some(v) => Some(v),
    }
}

/// `nullable|string|max:255` untuk `description`: `Some(None)` bila null.
fn description(e: &mut Errors, v: &Value) -> Option<Option<String>> {
    match v {
        Value::Null => Some(None),
        other => string_max(e, "description", other, 255).map(Some),
    }
}

/// `POST /api/checklist-items`: `sort_order` = maks + 1 dalam konteks yang sama. Respons 200.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();

    let name = match input.get("name") {
        None | Some(Value::Null) => {
            e.add("name", "The name field is required.");
            None
        }
        Some(v) => string_max(&mut e, "name", v, 100),
    };
    let desc = match input.get("description") {
        None => None,
        Some(v) => description(&mut e, v),
    };
    let context = match input.get("context") {
        None | Some(Value::Null) => "pekerjaan".to_string(),
        Some(Value::String(s)) if CONTEXTS.contains(&s.as_str()) => s.clone(),
        Some(_) => {
            e.add("context", "The selected context is invalid.");
            "pekerjaan".to_string()
        }
    };
    e.finish()?;

    let max_order: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(MAX(sort_order) AS SIGNED) FROM tbl_checklist_items WHERE context = ?",
    )
    .bind(&context)
    .fetch_one(&state.pool)
    .await
    .map_err(internal)?;
    let sort_order = max_order.unwrap_or(0) + 1;

    let result = sqlx::query(
        "INSERT INTO tbl_checklist_items (name, description, sort_order, context, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(name.unwrap_or_default())
    .bind(desc.flatten())
    .bind(sort_order as i32)
    .bind(&context)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    let id = result.last_insert_id();
    let row = item_find(&state.pool, id).await.map_err(internal)?.ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": item_resource(&row) })).into_response())
}

/// `PUT` dan `PATCH /api/checklist-items/{id}`: `name` dan `sort_order` bersifat `sometimes`.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    item_find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let input = parse_body(&body);
    let mut e = Errors::default();

    let name = input.get("name").and_then(|v| string_max(&mut e, "name", v, 100));
    let desc = match input.get("description") {
        None => None,
        Some(v) => description(&mut e, v),
    };
    let sort = input.get("sort_order").and_then(|v| int_min0(&mut e, "sort_order", v));
    // `sometimes`: name dan sort_order yang tidak lolos validasi sudah dicatat; yang lolos disimpan
    // hanya bila tidak ada error sama sekali.
    e.finish()?;

    let mut sets: Vec<&str> = Vec::new();
    if name.is_some() {
        sets.push("name = ?");
    }
    if desc.is_some() {
        sets.push("description = ?");
    }
    if sort.is_some() {
        sets.push("sort_order = ?");
    }
    if !sets.is_empty() {
        let sql = format!(
            "UPDATE tbl_checklist_items SET {}, updated_at = NOW() WHERE id = ?",
            sets.join(", ")
        );
        let mut q = sqlx::query(&sql);
        if let Some(n) = &name {
            q = q.bind(n);
        }
        if let Some(d) = &desc {
            q = q.bind(d.clone());
        }
        if let Some(s) = sort {
            q = q.bind(s as i32);
        }
        q.bind(id).execute(&state.pool).await.map_err(internal)?;
    }
    let row = item_find(&state.pool, id).await.map_err(internal)?.ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": item_resource(&row) })).into_response())
}

/// `DELETE /api/checklist-items/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    item_find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    sqlx::query("DELETE FROM tbl_checklist_items WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "message": "Checklist item deleted successfully" })).into_response())
}

/// Item `items.N.id` harus ada di tabel (`exists`).
async fn exists_item(pool: &MySqlPool, id: i64) -> Result<bool, ApiError> {
    let n: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_checklist_items WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    Ok(n > 0)
}

/// `POST /api/checklist-items/reorder` dengan `{items: [{id, sort_order}]}`.
pub async fn reorder(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let mut e = Errors::default();

    let items = match input.get("items") {
        None | Some(Value::Null) => {
            e.add("items", "The items field is required.");
            None
        }
        Some(Value::Array(a)) => Some(a.clone()),
        Some(_) => {
            e.add("items", "The items field must be an array.");
            None
        }
    };
    let mut pairs: Vec<(i64, i64)> = Vec::new();
    if let Some(items) = &items {
        for (i, item) in items.iter().enumerate() {
            let id_key = format!("items.{i}.id");
            let order_key = format!("items.{i}.sort_order");
            let id = match item.get("id") {
                None | Some(Value::Null) => {
                    e.add(&id_key, format!("The {} field is required.", attr(&id_key)));
                    None
                }
                Some(v) => {
                    let parsed = v.as_i64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()));
                    let found = match parsed {
                        Some(id) => Some((id, exists_item(&state.pool, id).await?)),
                        None => None,
                    };
                    match found {
                        Some((id, true)) => Some(id),
                        _ => {
                            e.add(&id_key, format!("The selected {} is invalid.", attr(&id_key)));
                            None
                        }
                    }
                }
            };
            let order = match item.get("sort_order") {
                None | Some(Value::Null) => {
                    e.add(&order_key, format!("The {} field is required.", attr(&order_key)));
                    None
                }
                Some(v) => int_min0(&mut e, &order_key, v),
            };
            if let (Some(id), Some(order)) = (id, order) {
                pairs.push((id, order));
            }
        }
    }
    e.finish()?;

    for (id, order) in pairs {
        sqlx::query("UPDATE tbl_checklist_items SET sort_order = ? WHERE id = ?")
            .bind(order as i32)
            .bind(id)
            .execute(&state.pool)
            .await
            .map_err(internal)?;
    }
    Ok(Json(json!({ "message": "Reorder successful" })).into_response())
}
