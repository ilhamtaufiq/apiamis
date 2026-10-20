//! Catatan pribadi user. Dibuat dari aplikasi (`POST /api/catatan`) atau dari perintah
//! "@arumanis catat" di WhatsApp (`simpan_dari_wa`). Setiap user hanya bisa mengubah catatannya sendiri.
//! Admin bisa melihat semua catatan dengan `?semua=1`.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    media::internal,
    notifications::require_admin,
    pagination::{self, PageParams},
    require_auth, AppState,
};

/// Batas panjang satu catatan, dalam karakter.
pub const ISI_MAKS: usize = 10_000;
const PER_PAGE: u64 = 20;

fn cek_isi(isi: &str) -> Result<String, ApiError> {
    let bersih = isi.trim().to_string();
    if bersih.is_empty() {
        return Err(ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "Isi catatan wajib diisi."));
    }
    if bersih.chars().count() > ISI_MAKS {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Catatan maksimal {ISI_MAKS} karakter."),
        ));
    }
    Ok(bersih)
}

/// Menyimpan catatan dari WhatsApp atas nama user yang nomornya cocok. Mengembalikan id catatan.
pub(crate) async fn simpan_dari_wa(
    pool: &MySqlPool,
    user_id: u64,
    isi: &str,
    chat_id: &str,
) -> Result<u64, ApiError> {
    let isi = cek_isi(isi)?;
    let hasil = sqlx::query(
        "INSERT INTO catatan (user_id, isi, sumber, chat_id, created_at, updated_at) VALUES (?, ?, 'whatsapp', ?, NOW(), NOW())",
    )
    .bind(user_id)
    .bind(isi)
    .bind(chat_id)
    .execute(pool)
    .await
    .map_err(internal)?;
    Ok(hasil.last_insert_id())
}

/// Model media untuk lampiran catatan (morph type seperti di Laravel).
const CATATAN_MODEL: &str = "App\\Models\\Catatan";

/// Simpan catatan dari WhatsApp beserta satu lampiran media (foto, dokumen, dll).
pub(crate) async fn simpan_dari_wa_media(
    pool: &MySqlPool,
    user_id: u64,
    isi: &str,
    chat_id: &str,
    upload: &crate::media::Upload,
    mime: &str,
) -> Result<u64, ApiError> {
    let isi = cek_isi(isi)?;
    let mut tx = pool.begin().await.map_err(internal)?;
    let hasil = sqlx::query(
        "INSERT INTO catatan (user_id, isi, sumber, chat_id, created_at, updated_at) VALUES (?, ?, 'whatsapp', ?, NOW(), NOW())",
    )
    .bind(user_id)
    .bind(isi)
    .bind(chat_id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = hasil.last_insert_id();
    let stored = crate::media::attach(&mut tx, CATATAN_MODEL, id, "lampiran", upload, mime, mime.starts_with("image/")).await?;
    if let Err(e) = tx.commit().await {
        crate::media::remove_dirs(&[stored.dir]).await;
        return Err(internal(e));
    }
    Ok(id)
}

/// Lampiran per catatan: URL asli dan thumbnail, hanya untuk media di disk `public`.
async fn lampiran_untuk(pool: &MySqlPool, app_url: &str, ids: &[u64]) -> Result<HashMap<u64, Vec<Value>>, ApiError> {
    let mut hasil: HashMap<u64, Vec<Value>> = HashMap::new();
    if ids.is_empty() {
        return Ok(hasil);
    }
    let daftar = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT CAST(model_id AS UNSIGNED) AS model_id, disk, file_name, CAST(id AS UNSIGNED) AS id, \
         COALESCE(mime_type, '') AS mime_type, CAST(generated_conversions AS CHAR) AS generated_conversions \
         FROM media WHERE model_type = ? AND collection_name = 'lampiran' AND model_id IN ({daftar}) ORDER BY order_column, id"
    );
    let mut q = sqlx::query(&sql).bind(CATATAN_MODEL);
    for id in ids {
        q = q.bind(id);
    }
    let rows = q.fetch_all(pool).await.map_err(internal)?;
    for r in &rows {
        let disk: String = r.try_get("disk").map_err(internal)?;
        if disk != "public" {
            continue;
        }
        let model_id: u64 = r.try_get("model_id").map_err(internal)?;
        let media_id: u64 = r.try_get("id").map_err(internal)?;
        let file_name: String = r.try_get("file_name").map_err(internal)?;
        let mime: String = r.try_get("mime_type").map_err(internal)?;
        let generated: String = r.try_get("generated_conversions").map_err(internal)?;
        let (url, thumb) = crate::media::public_urls(app_url, media_id, &file_name, &generated);
        hasil.entry(model_id).or_default().push(json!({ "url": url, "thumb": thumb, "mime_type": mime }));
    }
    Ok(hasil)
}

fn catatan_json(r: &sqlx::mysql::MySqlRow, lampiran: &HashMap<u64, Vec<Value>>) -> Result<Value, sqlx::Error> {
    let id: u64 = r.try_get("id")?;
    Ok(json!({
        "id": id,
        "lampiran": lampiran.get(&id).cloned().unwrap_or_default(),
        "isi": r.try_get::<String, _>("isi")?,
        "sumber": r.try_get::<String, _>("sumber")?,
        "user_id": r.try_get::<u64, _>("user_id")?,
        "user_nama": r.try_get::<Option<String>, _>("user_nama")?,
        "created_at": r.try_get::<Option<chrono::NaiveDateTime>, _>("created_at")?.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
        "updated_at": r.try_get::<Option<chrono::NaiveDateTime>, _>("updated_at")?.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
    }))
}

/// `GET /api/catatan?q=&page=&semua=1`: catatan sendiri, atau semua catatan bila admin dan `semua=1`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let is_admin = require_admin(&state.pool, user.user_id).await.is_ok();
    let semua = is_admin && query.get("semua").map(String::as_str) == Some("1");
    let cari = query.get("q").map(|q| q.trim().to_string()).filter(|q| !q.is_empty());
    let page_params = pagination::page_params(&query);
    let page = page_params.page;

    let mut where_sql = String::from(" WHERE 1=1");
    if !semua {
        where_sql.push_str(" AND c.user_id = ?");
    }
    if cari.is_some() {
        where_sql.push_str(" AND c.isi LIKE ?");
    }

    let count_sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM catatan c{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    if !semua {
        cq = cq.bind(user.user_id);
    }
    if let Some(c) = &cari {
        cq = cq.bind(format!("%{c}%"));
    }
    let total: i64 = cq.fetch_one(&state.pool).await.map_err(internal)?;

    let list_sql = format!(
        "SELECT CAST(c.id AS UNSIGNED) AS id, c.isi, c.sumber, CAST(c.user_id AS UNSIGNED) AS user_id, \
         u.name AS user_nama, c.created_at, c.updated_at \
         FROM catatan c LEFT JOIN users u ON u.id = c.user_id{where_sql} ORDER BY c.created_at DESC, c.id DESC LIMIT ? OFFSET ?"
    );
    let mut lq = sqlx::query(&list_sql);
    if !semua {
        lq = lq.bind(user.user_id);
    }
    if let Some(c) = &cari {
        lq = lq.bind(format!("%{c}%"));
    }
    let rows = lq
        .bind(PER_PAGE)
        .bind((page - 1) * PER_PAGE)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let ids: Vec<u64> = rows.iter().map(|r| r.try_get::<u64, _>("id")).collect::<Result<_, _>>().map_err(internal)?;
    let lampiran = lampiran_untuk(&state.pool, &state.app_url, &ids).await?;
    let data = rows.iter().map(|r| catatan_json(r, &lampiran)).collect::<Result<Vec<_>, _>>().map_err(internal)?;

    let base = format!("{}/api/catatan", state.app_url.trim_end_matches('/'));
    Ok(Json(json!({
        "catatan": pagination::paginate(
            data,
            total as u64,
            PageParams { page, per_page: PER_PAGE },
            &base,
        ),
        "semua": semua,
    })))
}

/// `POST /api/catatan`: buat catatan dari aplikasi.
pub async fn store(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let isi = cek_isi(body["isi"].as_str().unwrap_or(""))?;
    let hasil = sqlx::query(
        "INSERT INTO catatan (user_id, isi, sumber, created_at, updated_at) VALUES (?, ?, 'web', NOW(), NOW())",
    )
    .bind(user.user_id)
    .bind(&isi)
    .execute(&state.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({ "id": hasil.last_insert_id(), "isi": isi })))
}

/// Pastikan catatan ada dan milik user. Mengembalikan galat 404 bila tidak.
async fn milik_user(pool: &MySqlPool, id: u64, user_id: u64) -> Result<(), ApiError> {
    let pemilik: Option<u64> = sqlx::query_scalar("SELECT CAST(user_id AS UNSIGNED) FROM catatan WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    match pemilik {
        Some(p) if p == user_id => Ok(()),
        _ => Err(ApiError::new(StatusCode::NOT_FOUND, "Catatan tidak ditemukan.")),
    }
}

/// `PUT /api/catatan/{id}`: ubah isi catatan sendiri.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    milik_user(&state.pool, id, user.user_id).await?;
    let isi = cek_isi(body["isi"].as_str().unwrap_or(""))?;
    sqlx::query("UPDATE catatan SET isi = ?, updated_at = NOW() WHERE id = ?")
        .bind(&isi)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "id": id, "isi": isi })))
}

/// `DELETE /api/catatan/{id}`: hapus catatan sendiri.
pub async fn destroy(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<u64>) -> Result<StatusCode, ApiError> {
    let user = require_auth(&state, &headers).await?;
    milik_user(&state.pool, id, user.user_id).await?;
    sqlx::query("DELETE FROM catatan WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isi_kosong_dan_terlalu_panjang_ditolak() {
        assert!(cek_isi("   ").is_err());
        assert!(cek_isi(&"a".repeat(ISI_MAKS + 1)).is_err());
        assert_eq!(cek_isi("  catatan  ").unwrap(), "catatan");
    }
}
