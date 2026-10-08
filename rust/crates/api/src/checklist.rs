//! Checklist: `GET /api/checklist-items` (+ `{id}`) dan `GET /api/pekerjaan-checklist`.
//! Setara `ChecklistItemController` dan `PekerjaanChecklistController@index`.
//!
//! Hanya GET. `pekerjaan-checklist` memakai gate role yang sama dengan `/api/pekerjaan`
//! (fail-closed) karena `byUserRole()` belum dipindah. `history`, `toggle`, dan ekspor
//! belum dipindah.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, QueryBuilder, Row};

use crate::{
    desa::internal, format::iso8601_utc, pagination, pekerjaan::has_full_access, require_auth,
    AppState,
};

/// `ChecklistItemResource`. `created_at` dan `updated_at` memakai `toIso8601String()`.
#[derive(Debug, Clone)]
pub struct ItemRow {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i32,
    pub context: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

pub fn item_resource(i: &ItemRow) -> Value {
    json!({
        "id": i.id,
        "name": i.name,
        "description": i.description,
        "sort_order": i.sort_order,
        "context": i.context,
        "created_at": iso8601_utc(i.created_at),
        "updated_at": iso8601_utc(i.updated_at),
    })
}

fn map_item(r: &sqlx::mysql::MySqlRow) -> Result<ItemRow, sqlx::Error> {
    Ok(ItemRow {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        description: r.try_get("description")?,
        sort_order: r.try_get("sort_order")?,
        context: r.try_get("context")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

const ITEM_COLS: &str = "id, name, description, sort_order, context, created_at, updated_at";

/// Item untuk satu konteks, urut `sort_order` lalu `id`.
pub async fn items_in_context(
    pool: &MySqlPool,
    context: &str,
) -> Result<Vec<ItemRow>, sqlx::Error> {
    let rows = sqlx::query(&format!(
        "SELECT {ITEM_COLS} FROM tbl_checklist_items WHERE context = ? ORDER BY sort_order, id"
    ))
    .bind(context)
    .fetch_all(pool)
    .await?;
    rows.iter().map(map_item).collect()
}

pub async fn item_find(pool: &MySqlPool, id: u64) -> Result<Option<ItemRow>, sqlx::Error> {
    let row = sqlx::query(&format!(
        "SELECT {ITEM_COLS} FROM tbl_checklist_items WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(map_item).transpose()
}

/// `GET /api/checklist-items?context=`: `{"data": [...]}`, konteks default `pekerjaan`.
pub async fn items_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let context = query
        .get("context")
        .map(String::as_str)
        .unwrap_or("pekerjaan");
    let items = items_in_context(&state.pool, context)
        .await
        .map_err(internal)?;
    let data: Vec<Value> = items.iter().map(item_resource).collect();
    Ok(Json(json!({ "data": data })))
}

/// `GET /api/checklist-items/{id}`: `{"data": ChecklistItemResource}`.
pub async fn items_show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let row = match id.parse::<u64>() {
        Ok(id) => item_find(&state.pool, id).await.map_err(internal)?,
        Err(_) => None,
    };
    let row = row.ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": item_resource(&row) })))
}

/// Filter `pekerjaan-checklist`: `tahun` (lewat kegiatan), `kegiatan_id`, dan `search` pada `nama_paket`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PekerjaanFilter {
    pub tahun: Option<String>,
    pub kegiatan_id: Option<String>,
    pub search: Option<String>,
}

impl PekerjaanFilter {
    /// `filled()`: tidak kosong, termasuk `"0"`.
    pub fn from_query(query: &HashMap<String, String>) -> Self {
        let filled = |k: &str| {
            query
                .get(k)
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string())
        };
        Self {
            tahun: filled("tahun"),
            kegiatan_id: filled("kegiatan_id"),
            search: filled("search"),
        }
    }
}

fn push_pekerjaan_filter(qb: &mut QueryBuilder<MySql>, f: &PekerjaanFilter) {
    if let Some(t) = &f.tahun {
        qb.push(" AND k.tahun_anggaran = ").push_bind(t.clone());
    }
    if let Some(k) = &f.kegiatan_id {
        qb.push(" AND p.kegiatan_id = ").push_bind(k.clone());
    }
    if let Some(s) = &f.search {
        qb.push(" AND p.nama_paket LIKE ")
            .push_bind(format!("%{s}%"));
    }
}

/// Satu baris pekerjaan dengan `kegiatan` (`null` bila tidak ada).
#[derive(Debug, Clone)]
pub struct PekerjaanRowLite {
    pub id: u64,
    pub nama_paket: Option<String>,
    pub kegiatan: Option<(u64, Option<String>)>,
}

/// Halaman pekerjaan untuk `pekerjaan-checklist`, urut `id`. Mengembalikan juga total.
pub async fn pekerjaan_page(
    pool: &MySqlPool,
    f: &PekerjaanFilter,
    page: u64,
    per_page: u64,
) -> Result<(Vec<PekerjaanRowLite>, u64), sqlx::Error> {
    let mut count = QueryBuilder::<MySql>::new(
        "SELECT COUNT(*) FROM tbl_pekerjaan p LEFT JOIN tbl_kegiatan k ON k.id = p.kegiatan_id WHERE 1=1",
    );
    push_pekerjaan_filter(&mut count, f);
    let total: i64 = count.build_query_scalar().fetch_one(pool).await?;

    let mut qb = QueryBuilder::<MySql>::new(
        "SELECT p.id, p.nama_paket, k.id AS keg_id, k.nama_sub_kegiatan FROM tbl_pekerjaan p \
         LEFT JOIN tbl_kegiatan k ON k.id = p.kegiatan_id WHERE 1=1",
    );
    push_pekerjaan_filter(&mut qb, f);
    qb.push(" ORDER BY p.id LIMIT ")
        .push_bind(per_page)
        .push(" OFFSET ")
        .push_bind((page - 1) * per_page);
    let rows = qb.build().fetch_all(pool).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let keg_id: Option<u64> = r.try_get("keg_id")?;
        out.push(PekerjaanRowLite {
            id: r.try_get("id")?,
            nama_paket: r.try_get("nama_paket")?,
            kegiatan: keg_id
                .map(|id| Ok::<_, sqlx::Error>((id, r.try_get("nama_sub_kegiatan")?)))
                .transpose()?,
        });
    }
    Ok((out, total as u64))
}

/// Satu baris `pekerjaan_checklist`. Tanggal dibaca sebagai teks: `DB::table` Laravel tidak melakukan cast.
#[derive(Debug, Clone)]
struct CheckRow {
    item_id: u64,
    is_checked: bool,
    checked_at: Option<String>,
    updated_at: Option<String>,
    checked_by: Option<u64>,
    notes: Option<String>,
}

async fn checks_for(pool: &MySqlPool, pekerjaan_id: u64) -> Result<Vec<CheckRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT checklist_item_id, is_checked, CAST(checked_at AS CHAR) AS checked_at, \
         CAST(updated_at AS CHAR) AS updated_at, checked_by, notes FROM pekerjaan_checklist \
         WHERE pekerjaan_id = ?",
    )
    .bind(pekerjaan_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(CheckRow {
                item_id: r.try_get("checklist_item_id")?,
                is_checked: r.try_get("is_checked")?,
                checked_at: r.try_get("checked_at")?,
                updated_at: r.try_get("updated_at")?,
                checked_by: r.try_get("checked_by")?,
                notes: r.try_get("notes")?,
            })
        })
        .collect()
}

async fn user_name(pool: &MySqlPool, id: u64) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT name FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map(Option::flatten)
}

/// Bentuk `pekerjaan-checklist` untuk satu pekerjaan, termasuk pencatatan pembaruan terakhir.
pub async fn pekerjaan_entry(
    pool: &MySqlPool,
    p: &PekerjaanRowLite,
    items: &[ItemRow],
) -> Result<Value, sqlx::Error> {
    let checks = checks_for(pool, p.id).await?;
    let mut checklist = serde_json::Map::new();
    let mut latest_at: Option<String> = None;
    let mut latest_by: Option<u64> = None;
    let mut latest_by_name: Option<String> = None;

    for item in items {
        let data = checks.iter().find(|c| c.item_id == item.id);
        // `$data?->updated_at ?? $data?->checked_at`
        let updated_at = data.and_then(|c| c.updated_at.clone().or_else(|| c.checked_at.clone()));
        let checked_by = data.and_then(|c| c.checked_by);
        let checked_by_name = match checked_by {
            Some(uid) => user_name(pool, uid).await?,
            None => None,
        };

        checklist.insert(
            item.id.to_string(),
            json!({
                "is_checked": data.is_some_and(|c| c.is_checked),
                "checked_at": data.and_then(|c| c.checked_at.clone()),
                "updated_at": updated_at,
                "checked_by": checked_by,
                "checked_by_name": checked_by_name,
                "notes": data.and_then(|c| c.notes.clone()),
            }),
        );

        if let Some(at) = &updated_at {
            // Perbandingan teks cukup: formatnya `Y-m-d H:i:s` yang sama.
            if latest_at.as_ref().is_none_or(|l| at > l) {
                latest_at = Some(at.clone());
                latest_by = checked_by;
                latest_by_name = checked_by_name.clone();
            }
        }
    }

    Ok(json!({
        "id": p.id,
        "nama_paket": p.nama_paket,
        "kegiatan": p.kegiatan.as_ref().map(|(id, nama)| json!({
            "id": id,
            "nama_sub_kegiatan": nama,
        })),
        // Array PHP kosong menjadi `[]`, bukan objek.
        "checklist": if checklist.is_empty() { json!([]) } else { Value::Object(checklist) },
        "last_updated_at": latest_at,
        "last_updated_by": latest_by,
        "last_updated_by_name": latest_by_name,
    }))
}

/// `GET /api/pekerjaan-checklist`: `{columns, data, meta}` dengan `meta` paginator Laravel
/// (tanpa `links`). `per_page` default 15.
pub async fn pekerjaan_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let role_names: Vec<String> = roles.iter().map(|(_, n)| n.clone()).collect();
    if !has_full_access(&role_names) {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "Akses ditolak.".to_string(),
        ));
    }

    let page = pagination::page_params(&query).page;
    let per_page = query
        .get("per_page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|p| *p >= 1)
        .unwrap_or(15);
    let filter = PekerjaanFilter::from_query(&query);

    let items = items_in_context(&state.pool, "pekerjaan")
        .await
        .map_err(internal)?;
    let (rows, total) = pekerjaan_page(&state.pool, &filter, page, per_page)
        .await
        .map_err(internal)?;

    let mut data = Vec::with_capacity(rows.len());
    for p in &rows {
        data.push(
            pekerjaan_entry(&state.pool, p, &items)
                .await
                .map_err(internal)?,
        );
    }

    let count = rows.len() as u64;
    let from = (count > 0).then(|| (page - 1) * per_page + 1);
    let to = from.map(|f| f + count - 1);
    let last_page = total.div_ceil(per_page).max(1);

    let columns: Vec<Value> = items
        .iter()
        .map(|i| {
            json!({
                "id": i.id,
                "name": i.name,
                "description": i.description,
                "sort_order": i.sort_order,
            })
        })
        .collect();

    Ok(Json(json!({
        "columns": columns,
        "data": data,
        "meta": {
            "current_page": page,
            "from": from,
            "last_page": last_page,
            "per_page": per_page,
            "to": to,
            "total": total,
        },
    })))
}

/// Gate role untuk endpoint Checklist yang memakai `byUserRole()`. Role lain ditolak (fail-closed).
async fn require_full_access(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let user = require_auth(state, headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let names: Vec<String> = roles.into_iter().map(|(_, n)| n).collect();
    if has_full_access(&names) {
        Ok(())
    } else {
        Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "Akses ditolak.".to_string(),
        ))
    }
}

/// Filter `pekerjaan-checklist/history`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryFilter {
    pub pekerjaan_id: Option<u64>,
    pub checklist_item_id: Option<u64>,
    pub user_id: Option<u64>,
    pub tahun: Option<String>,
    pub search: Option<String>,
}

/// `!empty()` di PHP: kosong dan `"0"` dianggap tidak ada.
fn non_empty(query: &HashMap<String, String>, key: &str) -> Option<String> {
    query
        .get(key)
        .filter(|v| !v.is_empty() && *v != "0")
        .cloned()
}

fn push_history_filter(qb: &mut QueryBuilder<MySql>, f: &HistoryFilter) {
    if let Some(p) = f.pekerjaan_id {
        qb.push(" AND h.pekerjaan_id = ").push_bind(p);
    }
    if let Some(c) = f.checklist_item_id {
        qb.push(" AND h.checklist_item_id = ").push_bind(c);
    }
    if let Some(u) = f.user_id {
        qb.push(" AND h.user_id = ").push_bind(u);
    }
    if let Some(t) = &f.tahun {
        qb.push(" AND k.tahun_anggaran = ").push_bind(t.clone());
    }
    if let Some(s) = &f.search {
        let like = format!("%{s}%");
        qb.push(" AND (p.nama_paket LIKE ")
            .push_bind(like.clone())
            .push(" OR ci.name LIKE ")
            .push_bind(like.clone())
            .push(" OR u.name LIKE ")
            .push_bind(like)
            .push(")");
    }
}

const HISTORY_FROM: &str = " FROM pekerjaan_checklist_histories h \
     JOIN tbl_pekerjaan p ON p.id = h.pekerjaan_id \
     LEFT JOIN tbl_kegiatan k ON k.id = p.kegiatan_id \
     LEFT JOIN tbl_checklist_items ci ON ci.id = h.checklist_item_id \
     LEFT JOIN users u ON u.id = h.user_id WHERE 1=1";

/// `GET /api/pekerjaan-checklist/history`: paginasi dengan `meta` tanpa `links`, `per_page` default 20.
pub async fn history_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_full_access(&state, &headers).await?;

    let per_page = query
        .get("per_page")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(20);
    if !(1..=100).contains(&per_page) {
        return Err(ApiError::validation(
            "The per page field must be between 1 and 100.",
            Default::default(),
        ));
    }
    let page = pagination::page_params(&query).page;

    let filter = HistoryFilter {
        pekerjaan_id: non_empty(&query, "pekerjaan_id").and_then(|v| v.parse().ok()),
        checklist_item_id: non_empty(&query, "checklist_item_id").and_then(|v| v.parse().ok()),
        user_id: non_empty(&query, "user_id").and_then(|v| v.parse().ok()),
        tahun: non_empty(&query, "tahun"),
        search: non_empty(&query, "search"),
    };

    let mut count = QueryBuilder::<MySql>::new("SELECT COUNT(*)");
    count.push(HISTORY_FROM);
    push_history_filter(&mut count, &filter);
    let total: i64 = count
        .build_query_scalar()
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;

    let mut qb = QueryBuilder::<MySql>::new(
        "SELECT h.id, h.pekerjaan_id, p.nama_paket, k.nama_sub_kegiatan, h.checklist_item_id, \
         ci.name AS item_name, h.is_checked, h.notes, h.user_id, u.name AS user_name, u.email AS user_email, \
         DATE_FORMAT(h.created_at, '%Y-%m-%d %H:%i:%s') AS created_at",
    );
    qb.push(HISTORY_FROM);
    push_history_filter(&mut qb, &filter);
    qb.push(" ORDER BY h.created_at DESC, h.id DESC LIMIT ")
        .push_bind(per_page)
        .push(" OFFSET ")
        .push_bind((page - 1) * per_page);
    let rows = qb.build().fetch_all(&state.pool).await.map_err(internal)?;

    let mut data = Vec::with_capacity(rows.len());
    for r in &rows {
        data.push(json!({
            "id": r.try_get::<u64, _>("id").map_err(internal)?,
            "pekerjaan_id": r.try_get::<u64, _>("pekerjaan_id").map_err(internal)?,
            "pekerjaan_nama": r.try_get::<Option<String>, _>("nama_paket").map_err(internal)?,
            "kegiatan": r.try_get::<Option<String>, _>("nama_sub_kegiatan").map_err(internal)?,
            "checklist_item_id": r.try_get::<u64, _>("checklist_item_id").map_err(internal)?,
            "checklist_item_name": r.try_get::<Option<String>, _>("item_name").map_err(internal)?,
            "is_checked": r.try_get::<bool, _>("is_checked").map_err(internal)?,
            "notes": r.try_get::<Option<String>, _>("notes").map_err(internal)?,
            "user_id": r.try_get::<Option<u64>, _>("user_id").map_err(internal)?,
            "user_name": r.try_get::<Option<String>, _>("user_name").map_err(internal)?,
            "user_email": r.try_get::<Option<String>, _>("user_email").map_err(internal)?,
            "created_at": r.try_get::<Option<String>, _>("created_at").map_err(internal)?,
        }));
    }

    let count_items = rows.len() as u64;
    let from = (count_items > 0).then(|| (page - 1) * per_page + 1);
    let to = from.map(|f| f + count_items - 1);
    let total = total as u64;
    Ok(Json(json!({
        "data": data,
        "meta": {
            "current_page": page,
            "from": from,
            "last_page": total.div_ceil(per_page).max(1),
            "per_page": per_page,
            "to": to,
            "total": total,
        },
    })))
}

/// `GET /api/post-pekerjaan-checklist`: hanya pekerjaan yang punya kontrak, konteks `post_pekerjaan`.
pub async fn post_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_full_access(&state, &headers).await?;

    let page = pagination::page_params(&query).page;
    let per_page = query
        .get("per_page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|p| *p >= 1)
        .unwrap_or(15);
    let items = items_in_context(&state.pool, "post_pekerjaan")
        .await
        .map_err(internal)?;

    let filter_tahun = non_empty(&query, "tahun");
    let filter_keg = non_empty(&query, "kegiatan_id");
    let filter_search = query.get("search").filter(|v| !v.is_empty()).cloned();

    let push = |qb: &mut QueryBuilder<MySql>| {
        qb.push(" FROM tbl_pekerjaan p LEFT JOIN tbl_kegiatan k ON k.id = p.kegiatan_id WHERE EXISTS (SELECT 1 FROM kontrak_pekerjaan kp WHERE kp.pekerjaan_id = p.id)");
        if let Some(t) = &filter_tahun {
            qb.push(" AND k.tahun_anggaran = ").push_bind(t.clone());
        }
        if let Some(g) = &filter_keg {
            qb.push(" AND p.kegiatan_id = ").push_bind(g.clone());
        }
        if let Some(s) = &filter_search {
            let like = format!("%{s}%");
            qb.push(" AND (p.nama_paket LIKE ")
                .push_bind(like.clone())
                .push(" OR EXISTS (SELECT 1 FROM kontrak_pekerjaan kp2 JOIN tbl_kontrak kt ON kt.id = kp2.kontrak_id WHERE kp2.pekerjaan_id = p.id AND (kt.nomor_penawaran LIKE ")
                .push_bind(like.clone())
                .push(" OR kt.spk LIKE ")
                .push_bind(like.clone())
                .push(" OR kt.kode_paket LIKE ")
                .push_bind(like)
                .push(")))");
        }
    };

    let mut count = QueryBuilder::<MySql>::new("SELECT COUNT(*)");
    push(&mut count);
    let total: i64 = count
        .build_query_scalar()
        .fetch_one(&state.pool)
        .await
        .map_err(internal)?;

    let mut qb = QueryBuilder::<MySql>::new(
        "SELECT p.id, p.nama_paket, k.id AS keg_id, k.nama_sub_kegiatan",
    );
    push(&mut qb);
    qb.push(" ORDER BY p.id LIMIT ")
        .push_bind(per_page)
        .push(" OFFSET ")
        .push_bind((page - 1) * per_page);
    let rows = qb.build().fetch_all(&state.pool).await.map_err(internal)?;

    let mut data = Vec::with_capacity(rows.len());
    for r in &rows {
        let pid: u64 = r.try_get("id").map_err(internal)?;
        let keg_id: Option<u64> = r.try_get("keg_id").map_err(internal)?;
        let kontrak = sqlx::query(
            "SELECT kt.id, kt.nomor_penawaran, kt.spk, kt.kode_paket, p2.nama AS penyedia_nama \
             FROM kontrak_pekerjaan kp JOIN tbl_kontrak kt ON kt.id = kp.kontrak_id \
             LEFT JOIN tbl_penyedia p2 ON p2.id = kt.id_penyedia \
             WHERE kp.pekerjaan_id = ? ORDER BY kt.id LIMIT 1",
        )
        .bind(pid)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?;
        let kontrak_json = match kontrak {
            Some(k) => json!({
                "id": k.try_get::<u64, _>("id").map_err(internal)?,
                "nomor_penawaran": k.try_get::<Option<String>, _>("nomor_penawaran").map_err(internal)?,
                "spk": k.try_get::<Option<String>, _>("spk").map_err(internal)?,
                "kode_paket": k.try_get::<Option<String>, _>("kode_paket").map_err(internal)?,
                "penyedia": k.try_get::<Option<String>, _>("penyedia_nama").map_err(internal)?,
            }),
            None => Value::Null,
        };

        let checks = checks_for(&state.pool, pid).await.map_err(internal)?;
        let mut checklist = serde_json::Map::new();
        for item in &items {
            let data = checks.iter().find(|c| c.item_id == item.id);
            checklist.insert(
                item.id.to_string(),
                json!({
                    "is_checked": data.is_some_and(|c| c.is_checked),
                    "checked_at": data.and_then(|c| c.checked_at.clone()),
                    "checked_by": data.and_then(|c| c.checked_by),
                    "notes": data.and_then(|c| c.notes.clone()),
                }),
            );
        }

        data.push(json!({
            "id": pid,
            "nama_paket": r.try_get::<Option<String>, _>("nama_paket").map_err(internal)?,
            "kegiatan": keg_id.map(|id| json!({
                "id": id,
                "nama_sub_kegiatan": r.try_get::<Option<String>, _>("nama_sub_kegiatan").ok().flatten(),
            })),
            "kontrak": kontrak_json,
            "checklist": if checklist.is_empty() { json!([]) } else { Value::Object(checklist) },
        }));
    }

    let columns: Vec<Value> = items
        .iter()
        .map(|i| {
            json!({
                "id": i.id,
                "name": i.name,
                "description": i.description,
                "sort_order": i.sort_order,
                "context": i.context,
            })
        })
        .collect();
    let total = total as u64;
    Ok(Json(json!({
        "columns": columns,
        "data": data,
        "meta": {
            "current_page": page,
            "last_page": total.div_ceil(per_page).max(1),
            "per_page": per_page,
            "total": total,
        },
    })))
}
