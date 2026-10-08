//! `/api/draft-pekerjaan`: port `DraftPekerjaanController` (daftar, store, show, update, destroy).
//! Ekspor Excel (`export/excel`) masih di Laravel.
//!
//! Model draft hanya memakai `Auditable` di Laravel: perubahan menulis audit tanpa notifikasi admin.
//! Pembatasan per pekerjaan (T36) berlaku pada tulis dan baca, sama dengan penerima dan output.

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
    access, audit, format::iso8601_utc, foto, lookup::carbon_json, media::internal, pagination,
    pekerjaan, penerima, penyedia, require_auth, AppState,
};

const MODEL: &str = "App\\Models\\DraftPekerjaan";
const SELECT_DRAFT: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, \
     CAST(penyedia_id AS SIGNED) AS penyedia_id, nama_pelaksana, kode_rup, kode_paket, created_at, updated_at \
     FROM tbl_draft_pekerjaan";

/// Kolom yang bisa berubah pada draft, urut tetap untuk audit.
const COLUMNS: &[&str] = &[
    "pekerjaan_id",
    "penyedia_id",
    "nama_pelaksana",
    "kode_rup",
    "kode_paket",
];

/// Baris `tbl_draft_pekerjaan`.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftRow {
    pub id: i64,
    pub pekerjaan_id: i64,
    pub penyedia_id: Option<i64>,
    pub nama_pelaksana: Option<String>,
    pub kode_rup: Option<String>,
    pub kode_paket: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<DraftRow, sqlx::Error> {
    Ok(DraftRow {
        id: r.try_get("id")?,
        pekerjaan_id: r.try_get("pekerjaan_id")?,
        penyedia_id: r.try_get("penyedia_id")?,
        nama_pelaksana: r.try_get("nama_pelaksana")?,
        kode_rup: r.try_get("kode_rup")?,
        kode_paket: r.try_get("kode_paket")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn find_row<'e, E>(exec: E, id: i64) -> Result<Option<DraftRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_DRAFT} WHERE id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_row).transpose()
}

fn col_json(row: &DraftRow, col: &str) -> Value {
    match col {
        "pekerjaan_id" => json!(row.pekerjaan_id),
        "penyedia_id" => json!(row.penyedia_id),
        "nama_pelaksana" => json!(row.nama_pelaksana),
        "kode_rup" => json!(row.kode_rup),
        "kode_paket" => json!(row.kode_paket),
        _ => Value::Null,
    }
}

fn attributes(row: &DraftRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    for col in COLUMNS {
        m.insert((*col).into(), col_json(row, col));
    }
    m.insert("created_at".into(), carbon_json(row.created_at));
    m.insert("updated_at".into(), carbon_json(row.updated_at));
    m
}

/// `DraftPekerjaanResource` dengan `pekerjaan` dan `penyedia` dimuat.
async fn resource(state: &AppState, row: &DraftRow) -> Result<Value, ApiError> {
    let pool = &state.pool;
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
    let penyedia_json = match row.penyedia_id {
        Some(p) => match penyedia::find(pool, p as u64).await.map_err(internal)? {
            Some(pr) => penyedia::with_dokumen(state, &pr).await.map_err(internal)?,
            None => Value::Null,
        },
        None => Value::Null,
    };
    Ok(json!({
        "id": row.id,
        "pekerjaan_id": row.pekerjaan_id,
        "penyedia_id": row.penyedia_id,
        "nama_pelaksana": row.nama_pelaksana,
        "kode_rup": row.kode_rup,
        "kode_paket": row.kode_paket,
        "pekerjaan": pekerjaan_json,
        "penyedia": penyedia_json,
        "created_at": iso8601_utc(row.created_at),
        "updated_at": iso8601_utc(row.updated_at),
    }))
}

async fn ensure_access(state: &AppState, actor: u64, pekerjaan_id: i64) -> Result<(), ApiError> {
    let roles = auth::login::roles_of(&state.pool, actor)
        .await
        .map_err(internal)?;
    foto::ensure_access(state, actor, &roles, Some(pekerjaan_id)).await
}

async fn ensure_exists(
    pool: &MySqlPool,
    table: &str,
    id: i64,
    key: &str,
    message: &str,
) -> Result<(), ApiError> {
    let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE id = ?"))
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    if n > 0 {
        return Ok(());
    }
    let mut errs = BTreeMap::new();
    foto::add(&mut errs, key, message.into());
    Err(ApiError::validation("The given data was invalid.", errs))
}

// ---------------------------------------------------------------------------
// Validasi input
// ---------------------------------------------------------------------------

/// `Some(None)` = null, `Some(Some(v))` = nilai, `None` = tidak dikirim.
type Field<T> = Option<Option<T>>;

#[derive(Debug, Default)]
struct Input {
    pekerjaan_id: Field<i64>,
    penyedia_id: Field<i64>,
    nama_pelaksana: Field<String>,
    kode_rup: Field<String>,
    kode_paket: Field<String>,
}

fn parse_input(body: &Value, store: bool) -> Result<Input, ApiError> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut errs = BTreeMap::new();
    let mut input = Input::default();

    // pekerjaan_id: `required|exists` saat store, `sometimes|required|exists` saat update.
    match obj.get("pekerjaan_id") {
        None | Some(Value::Null) if store => {
            foto::add(
                &mut errs,
                "pekerjaan_id",
                "The pekerjaan id field is required.".into(),
            );
        }
        Some(Value::Null) => {
            foto::add(
                &mut errs,
                "pekerjaan_id",
                "The pekerjaan id field is required.".into(),
            );
        }
        None => {}
        Some(v) => match penerima::as_int(v) {
            Some(n) => input.pekerjaan_id = Some(Some(n)),
            None => foto::add(
                &mut errs,
                "pekerjaan_id",
                "The pekerjaan id field must be an integer.".into(),
            ),
        },
    }

    match obj.get("penyedia_id") {
        None => {}
        Some(Value::Null) => input.penyedia_id = Some(None),
        Some(v) => match penerima::as_int(v) {
            Some(n) => input.penyedia_id = Some(Some(n)),
            None => foto::add(
                &mut errs,
                "penyedia_id",
                "The penyedia id field must be an integer.".into(),
            ),
        },
    }

    for key in ["nama_pelaksana", "kode_rup", "kode_paket"] {
        let slot = match key {
            "nama_pelaksana" => 0,
            "kode_rup" => 1,
            _ => 2,
        };
        match obj.get(key) {
            None => {}
            Some(Value::Null) => set_text(&mut input, slot, None),
            Some(Value::String(s)) => set_text(&mut input, slot, Some(s.clone())),
            Some(_) => foto::add(
                &mut errs,
                key,
                format!("The {} field must be a string.", key.replace('_', " ")),
            ),
        }
    }

    if errs.is_empty() {
        Ok(input)
    } else {
        Err(ApiError::validation("The given data was invalid.", errs))
    }
}

fn set_text(input: &mut Input, slot: u8, value: Option<String>) {
    match slot {
        0 => input.nama_pelaksana = Some(value),
        1 => input.kode_rup = Some(value),
        _ => input.kode_paket = Some(value),
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/draft-pekerjaan`: pekerjaan yang bisa diakses user dengan draft (paginasi, default 10).
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let scope = access::restriction(user.user_id, &roles, "p");

    let mut clauses = vec![format!("1=1{}", scope.sql)];
    let mut binds: Vec<String> = scope.binds.iter().map(u64::to_string).collect();
    if query.get("tahun").is_some_and(|v| !v.is_empty()) {
        clauses.push(
            "p.kegiatan_id IN (SELECT k.id FROM tbl_kegiatan k WHERE k.tahun_anggaran = ?)".into(),
        );
        binds.push(query["tahun"].clone());
    }
    if let Some(term) = query
        .get("search")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        clauses.push("(p.nama_paket LIKE ? OR p.kode_rekening LIKE ?)".into());
        binds.push(format!("%{term}%"));
        binds.push(format!("%{term}%"));
    }
    let where_sql = clauses.join(" AND ");
    let per_page = query
        .get("per_page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(10);
    let page = query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1);

    let total: i64 = {
        let sql = format!("SELECT COUNT(*) FROM tbl_pekerjaan p WHERE {where_sql}");
        let mut q = sqlx::query_scalar::<_, i64>(&sql);
        for b in &binds {
            q = q.bind(b);
        }
        q.fetch_one(&state.pool).await.map_err(internal)?
    };
    let sql = format!("SELECT CAST(p.id AS SIGNED) FROM tbl_pekerjaan p WHERE {where_sql} ORDER BY p.id LIMIT ? OFFSET ?");
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let ids: Vec<i64> = q
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;

    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(p) = pekerjaan::find(&state.pool, id as u64)
            .await
            .map_err(internal)?
        {
            rows.push(p);
        }
    }
    let viewer = crate::pekerjaan_rel::viewer(&state.pool, user.user_id, &roles)
        .await
        .map_err(internal)?;
    let rel = pekerjaan::load(
        &state.pool,
        &rows,
        pekerjaan::Mode {
            summary: false,
            unbounded: false,
        },
        &viewer,
    )
    .await
    .map_err(internal)?;
    let data: Vec<Value> = rows
        .iter()
        .map(|p| pekerjaan::to_resource(p, &rel))
        .collect();

    let base = format!(
        "{}/api/draft-pekerjaan",
        state.app_url.trim_end_matches('/')
    );
    Ok(Json(pagination::paginate_with_query(
        data,
        total as u64,
        pagination::PageParams { page, per_page },
        &base,
        "",
    ))
    .into_response())
}

/// `POST /api/draft-pekerjaan`: `updateOrCreate` per pekerjaan. Field yang tidak dikirim di-set null.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_input(&body, true)?;
    let pekerjaan_id = input.pekerjaan_id.flatten().unwrap_or_default();
    ensure_exists(
        &state.pool,
        "tbl_pekerjaan",
        pekerjaan_id,
        "pekerjaan_id",
        "The selected pekerjaan id is invalid.",
    )
    .await?;
    if let Some(Some(p)) = input.penyedia_id {
        ensure_exists(
            &state.pool,
            "tbl_penyedia",
            p,
            "penyedia_id",
            "The selected penyedia id is invalid.",
        )
        .await?;
    }
    ensure_access(&state, user.user_id, pekerjaan_id).await?;

    let next_penyedia = input.penyedia_id.flatten();
    let next_nama = input.nama_pelaksana.flatten();
    let next_rup = input.kode_rup.flatten();
    let next_paket = input.kode_paket.flatten();

    let url = format!(
        "{}/api/draft-pekerjaan",
        state.app_url.trim_end_matches('/')
    );
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let existing: Option<i64> = sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_draft_pekerjaan WHERE pekerjaan_id = ? ORDER BY id LIMIT 1")
        .bind(pekerjaan_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(internal)?;

    let id = match existing {
        Some(id) => {
            let current = find_row(&mut *tx, id)
                .await
                .map_err(internal)?
                .ok_or_else(|| internal("draft hilang"))?;
            let mut next = current.clone();
            next.penyedia_id = next_penyedia;
            next.nama_pelaksana = next_nama.clone();
            next.kode_rup = next_rup.clone();
            next.kode_paket = next_paket.clone();
            let changed: Vec<&str> = COLUMNS
                .iter()
                .copied()
                .filter(|c| col_json(&current, c) != col_json(&next, c))
                .collect();
            if !changed.is_empty() {
                let mut qb = QueryBuilder::<MySql>::new("UPDATE tbl_draft_pekerjaan SET ");
                for (i, col) in changed.iter().enumerate() {
                    if i > 0 {
                        qb.push(", ");
                    }
                    qb.push(*col).push(" = ");
                    match *col {
                        "penyedia_id" => qb.push_bind(next.penyedia_id),
                        "nama_pelaksana" => qb.push_bind(next.nama_pelaksana.clone()),
                        "kode_rup" => qb.push_bind(next.kode_rup.clone()),
                        "kode_paket" => qb.push_bind(next.kode_paket.clone()),
                        _ => qb.push_bind(next.pekerjaan_id),
                    };
                }
                qb.push(", updated_at = NOW() WHERE id = ").push_bind(id);
                qb.build().execute(&mut *tx).await.map_err(internal)?;
                let after = find_row(&mut *tx, id)
                    .await
                    .map_err(internal)?
                    .ok_or_else(|| internal("draft hilang"))?;
                let mut old = Map::new();
                let mut new = Map::new();
                for col in &changed {
                    old.insert((*col).into(), col_json(&current, col));
                    new.insert((*col).into(), col_json(&after, col));
                }
                old.insert("updated_at".into(), carbon_json(current.updated_at));
                new.insert("updated_at".into(), carbon_json(after.updated_at));
                audit::write(
                    &mut tx,
                    audit::Entry {
                        actor: user.user_id,
                        event: "updated",
                        auditable_type: MODEL,
                        auditable_id: id as u64,
                        old: Some(old),
                        new: Some(new),
                        url: &url,
                    },
                    &headers,
                )
                .await
                .map_err(internal)?;
            }
            id
        }
        None => {
            let res = sqlx::query(
                "INSERT INTO tbl_draft_pekerjaan (pekerjaan_id, penyedia_id, nama_pelaksana, kode_rup, kode_paket, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, NOW(), NOW())",
            )
            .bind(pekerjaan_id)
            .bind(next_penyedia)
            .bind(&next_nama)
            .bind(&next_rup)
            .bind(&next_paket)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
            let id = res.last_insert_id() as i64;
            let row = find_row(&mut *tx, id)
                .await
                .map_err(internal)?
                .ok_or_else(|| internal("draft baru tidak terbaca"))?;
            audit::write(
                &mut tx,
                audit::Entry {
                    actor: user.user_id,
                    event: "created",
                    auditable_type: MODEL,
                    auditable_id: id as u64,
                    old: None,
                    new: Some(attributes(&row)),
                    url: &url,
                },
                &headers,
            )
            .await
            .map_err(internal)?;
            id
        }
    };
    tx.commit().await.map_err(internal)?;

    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": resource(&state, &row).await? })).into_response())
}

/// `GET /api/draft-pekerjaan/{id}`.
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
    Ok(Json(json!({ "data": resource(&state, &row).await? })).into_response())
}

/// `PUT` dan `PATCH /api/draft-pekerjaan/{id}`. Hanya field yang dikirim yang diubah.
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
        ensure_exists(
            &state.pool,
            "tbl_pekerjaan",
            p,
            "pekerjaan_id",
            "The selected pekerjaan id is invalid.",
        )
        .await?;
        ensure_access(&state, user.user_id, p).await?;
    }
    if let Some(Some(p)) = input.penyedia_id {
        ensure_exists(
            &state.pool,
            "tbl_penyedia",
            p,
            "penyedia_id",
            "The selected penyedia id is invalid.",
        )
        .await?;
    }
    if matches!(input.pekerjaan_id, Some(None)) {
        return Err(internal("Column 'pekerjaan_id' cannot be null"));
    }

    let mut next = current.clone();
    if let Some(Some(v)) = input.pekerjaan_id {
        next.pekerjaan_id = v;
    }
    if let Some(v) = input.penyedia_id {
        next.penyedia_id = v;
    }
    if let Some(v) = input.nama_pelaksana {
        next.nama_pelaksana = v;
    }
    if let Some(v) = input.kode_rup {
        next.kode_rup = v;
    }
    if let Some(v) = input.kode_paket {
        next.kode_paket = v;
    }
    let changed: Vec<&str> = COLUMNS
        .iter()
        .copied()
        .filter(|c| col_json(&current, c) != col_json(&next, c))
        .collect();

    let url = format!(
        "{}/api/draft-pekerjaan/{id}",
        state.app_url.trim_end_matches('/')
    );
    let mut tx = state.pool.begin().await.map_err(internal)?;
    if !changed.is_empty() {
        let mut qb = QueryBuilder::<MySql>::new("UPDATE tbl_draft_pekerjaan SET ");
        for (i, col) in changed.iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            qb.push(*col).push(" = ");
            match *col {
                "pekerjaan_id" => qb.push_bind(next.pekerjaan_id),
                "penyedia_id" => qb.push_bind(next.penyedia_id),
                "nama_pelaksana" => qb.push_bind(next.nama_pelaksana.clone()),
                "kode_rup" => qb.push_bind(next.kode_rup.clone()),
                _ => qb.push_bind(next.kode_paket.clone()),
            };
        }
        qb.push(", updated_at = NOW() WHERE id = ").push_bind(id);
        qb.build().execute(&mut *tx).await.map_err(internal)?;
        let after = find_row(&mut *tx, id)
            .await
            .map_err(internal)?
            .ok_or_else(|| internal("draft hilang"))?;
        let mut old = Map::new();
        let mut new = Map::new();
        for col in &changed {
            old.insert((*col).into(), col_json(&current, col));
            new.insert((*col).into(), col_json(&after, col));
        }
        old.insert("updated_at".into(), carbon_json(current.updated_at));
        new.insert("updated_at".into(), carbon_json(after.updated_at));
        audit::write(
            &mut tx,
            audit::Entry {
                actor: user.user_id,
                event: "updated",
                auditable_type: MODEL,
                auditable_id: id as u64,
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

    let row = find_row(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": resource(&state, &row).await? })).into_response())
}

/// `DELETE /api/draft-pekerjaan/{id}`: 204 tanpa isi, seperti `response()->json(null, 204)`.
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
    let url = format!(
        "{}/api/draft-pekerjaan/{id}",
        state.app_url.trim_end_matches('/')
    );
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_draft_pekerjaan WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    audit::write(
        &mut tx,
        audit::Entry {
            actor: user.user_id,
            event: "deleted",
            auditable_type: MODEL,
            auditable_id: id as u64,
            old: Some(attributes(&row)),
            new: None,
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_requires_pekerjaan_and_accepts_null_optionals() {
        let err = parse_input(&json!({}), true).unwrap_err().body();
        assert!(err["errors"]["pekerjaan_id"].is_array());
        let ok = parse_input(
            &json!({"pekerjaan_id": 3, "penyedia_id": null, "kode_rup": "RUP-1"}),
            true,
        )
        .unwrap();
        assert_eq!(ok.penyedia_id, Some(None));
        assert_eq!(ok.kode_rup, Some(Some("RUP-1".into())));
        assert!(ok.kode_paket.is_none());
    }
}
