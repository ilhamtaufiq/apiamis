//! Progres pekerjaan: `GET /api/progress/pekerjaan/{id}` (laporan) dan `POST` (simpan).
//!
//! Mengikuti `ProgressController` dan model `Progress` (tabel `tbl_progress`; `Auditable` dan
//! `NotifiesAdminsOnChanges`). `report` membuat baris default bila belum ada (`firstOrCreate`),
//! jadi ikut mencatat audit dan notifikasi "created".

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::NaiveDate;
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row, Transaction};

use crate::{
    changes, format::number_like_php, foto, kontrak, lookup::carbon_json, require_auth, AppState,
};

const ITEM_KEYS: [&str; 7] = [
    "nama_item",
    "rincian_item",
    "satuan",
    "harga_satuan",
    "bobot",
    "target_volume",
    "weekly_data",
];

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn default_content() -> Value {
    json!({ "items": [], "week_count": 4 })
}

fn fmt_date(d: Option<NaiveDate>) -> Value {
    d.map_or(Value::Null, |d| json!(d.format("%Y-%m-%d").to_string()))
}

/// Baris `tbl_progress` untuk pekerjaan ini: (id, content).
async fn find_row(pool: &MySqlPool, pekerjaan_id: u64) -> Result<Option<(u64, Value)>, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED), CAST(content AS CHAR) FROM tbl_progress WHERE pekerjaan_id = ?",
    )
    .bind(pekerjaan_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    Ok(match row {
        None => None,
        Some(r) => {
            let id: i64 = r.try_get(0).map_err(internal)?;
            let content: Option<String> = r.try_get(1).map_err(internal)?;
            let value = content
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or(Value::Null);
            Some((id as u64, value))
        }
    })
}

async fn attributes<'e, E>(exec: E, id: u64) -> Result<Option<Map<String, Value>>, ApiError>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let row = sqlx::query("SELECT CAST(id AS SIGNED), CAST(pekerjaan_id AS SIGNED), CAST(content AS CHAR), created_at, updated_at FROM tbl_progress WHERE id = ?")
        .bind(id)
        .fetch_optional(exec)
        .await
        .map_err(internal)?;
    let Some(r) = row else {
        return Ok(None);
    };
    let content: Option<String> = r.try_get(2).map_err(internal)?;
    let created: Option<chrono::DateTime<chrono::Utc>> = r.try_get(3).map_err(internal)?;
    let updated: Option<chrono::DateTime<chrono::Utc>> = r.try_get(4).map_err(internal)?;
    let mut m = Map::new();
    m.insert(
        "id".into(),
        json!(r.try_get::<i64, _>(0).map_err(internal)?),
    );
    m.insert(
        "pekerjaan_id".into(),
        json!(r.try_get::<i64, _>(1).map_err(internal)?),
    );
    m.insert(
        "content".into(),
        content
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null),
    );
    m.insert("created_at".into(), carbon_json(created));
    m.insert("updated_at".into(), carbon_json(updated));
    Ok(Some(m))
}

/// `$validated` pada `store`: `items` (kunci yang divalidasi saja) dan `week_count`.
fn validate_store(body: &Value) -> Result<Value, ApiError> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let items = match obj.get("items") {
        None => {
            foto::add(&mut errs, "items", "The items field is required.".into());
            Vec::new()
        }
        Some(Value::Array(list)) => list.clone(),
        Some(_) => {
            foto::add(
                &mut errs,
                "items",
                "The items field must be an array.".into(),
            );
            Vec::new()
        }
    };
    let mut out_items = Vec::with_capacity(items.len());
    for (i, raw) in items.iter().enumerate() {
        let Some(item) = raw.as_object() else {
            foto::add(
                &mut errs,
                &format!("items.{i}"),
                format!("The items.{i} field must be an array."),
            );
            continue;
        };
        let text = |k: &str| {
            item.get(k)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        if text("nama_item").is_none() {
            foto::add(
                &mut errs,
                &format!("items.{i}.nama_item"),
                format!("The items.{i}.nama_item field is required."),
            );
        }
        if text("satuan").is_none() {
            foto::add(
                &mut errs,
                &format!("items.{i}.satuan"),
                format!("The items.{i}.satuan field is required."),
            );
        }
        for key in ["harga_satuan", "bobot", "target_volume"] {
            if let Some(v) = item.get(key).filter(|v| !v.is_null()) {
                let numeric =
                    v.is_number() || v.as_str().is_some_and(|s| s.trim().parse::<f64>().is_ok());
                if !numeric {
                    foto::add(
                        &mut errs,
                        &format!("items.{i}.{key}"),
                        format!("The items.{i}.{key} field must be a number."),
                    );
                }
            }
        }
        if let Some(v) = item.get("weekly_data").filter(|v| !v.is_null()) {
            if !v.is_array() && !v.is_object() {
                foto::add(
                    &mut errs,
                    &format!("items.{i}.weekly_data"),
                    format!("The items.{i}.weekly_data field must be an array."),
                );
            }
        }
        // Hanya kunci yang punya aturan validasi yang masuk ke `validated()`.
        let mut kept = Map::new();
        for k in ITEM_KEYS {
            if let Some(v) = item.get(k) {
                kept.insert(k.to_string(), v.clone());
            }
        }
        out_items.push(Value::Object(kept));
    }

    let week_count = match obj.get("week_count") {
        None => {
            foto::add(
                &mut errs,
                "week_count",
                "The week count field is required.".into(),
            );
            None
        }
        Some(v) => match v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        {
            Some(n) if n >= 1 => Some(n),
            Some(_) => {
                foto::add(
                    &mut errs,
                    "week_count",
                    "The week count field must be at least 1.".into(),
                );
                None
            }
            None => {
                foto::add(
                    &mut errs,
                    "week_count",
                    "The week count field must be an integer.".into(),
                );
                None
            }
        },
    };
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    Ok(json!({ "items": out_items, "week_count": week_count.unwrap_or(4) }))
}

/// `POST /api/progress/pekerjaan/{id}`: `updateOrCreate` dengan audit dan notifikasi.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pekerjaan_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let pekerjaan_id: u64 = pekerjaan_id.parse().map_err(|_| ApiError::not_found())?;
    let content = validate_store(&body)?;
    let url = format!(
        "{}/api/progress/pekerjaan/{pekerjaan_id}",
        state.app_url.trim_end_matches('/')
    );

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM tbl_progress WHERE pekerjaan_id = ? FOR UPDATE",
    )
    .bind(pekerjaan_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    match existing {
        None => {
            let id = insert_row(&mut tx, pekerjaan_id, &content).await?;
            log_created(&mut tx, &headers, user.user_id, id, pekerjaan_id, &url).await?;
        }
        Some(id) => {
            let id = id as u64;
            let before = attributes(&mut *tx, id)
                .await?
                .ok_or_else(|| internal("progress hilang"))?;
            sqlx::query("UPDATE tbl_progress SET content = ?, updated_at = NOW() WHERE id = ?")
                .bind(content.to_string())
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(internal)?;
            let after = attributes(&mut *tx, id)
                .await?
                .ok_or_else(|| internal("progress hilang"))?;
            if let Some((old, new)) = diff(&before, &after) {
                log_change_with(
                    &mut tx,
                    &headers,
                    user.user_id,
                    "updated",
                    id,
                    pekerjaan_id,
                    Some(old),
                    Some(new),
                    &url,
                )
                .await?;
            }
        }
    }
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({
        "success": true,
        "message": "Progress berhasil disimpan",
        "data": content,
    })))
}

async fn insert_row(
    tx: &mut Transaction<'_, MySql>,
    pekerjaan_id: u64,
    content: &Value,
) -> Result<u64, ApiError> {
    let res = sqlx::query("INSERT INTO tbl_progress (pekerjaan_id, content, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(pekerjaan_id)
        .bind(content.to_string())
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    Ok(res.last_insert_id())
}

fn diff(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Option<(Map<String, Value>, Map<String, Value>)> {
    let mut old = Map::new();
    let mut new = Map::new();
    for (k, v) in after {
        if k == "updated_at" {
            continue;
        }
        if before.get(k) != Some(v) {
            old.insert(k.clone(), before.get(k).cloned().unwrap_or(Value::Null));
            new.insert(k.clone(), v.clone());
        }
    }
    if new.is_empty() {
        return None;
    }
    old.insert(
        "updated_at".into(),
        before.get("updated_at").cloned().unwrap_or(Value::Null),
    );
    new.insert(
        "updated_at".into(),
        after.get("updated_at").cloned().unwrap_or(Value::Null),
    );
    Some((old, new))
}

/// Audit `created` dan notifikasi admin untuk baris progres yang baru dibuat.
async fn log_created(
    tx: &mut Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    id: u64,
    pekerjaan_id: u64,
    url: &str,
) -> Result<(), ApiError> {
    let new = attributes(&mut **tx, id).await?;
    changes::log(
        tx,
        headers,
        actor,
        &changes::PROGRESS,
        "created",
        id as i64,
        None,
        new,
        Some(pekerjaan_id as i64),
        url,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn log_change_with(
    tx: &mut Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    event: &str,
    id: u64,
    pekerjaan_id: u64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    url: &str,
) -> Result<(), ApiError> {
    changes::log(
        tx,
        headers,
        actor,
        &changes::PROGRESS,
        event,
        id as i64,
        old,
        new,
        Some(pekerjaan_id as i64),
        url,
    )
    .await
}

/// `GET /api/progress/pekerjaan/{id}`: laporan lengkap, dan membuat baris default bila belum ada.
pub async fn report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pekerjaan_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let pekerjaan_id: u64 = pekerjaan_id.parse().map_err(|_| ApiError::not_found())?;
    let pool = &state.pool;

    let pek = sqlx::query(
        "SELECT CAST(id AS SIGNED), nama_paket, pagu, CAST(kegiatan_id AS SIGNED), CAST(desa_id AS SIGNED), \
         CAST(kecamatan_id AS SIGNED), CAST(pengawas_id AS SIGNED) FROM tbl_pekerjaan WHERE id = ?",
    )
    .bind(pekerjaan_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?
    .ok_or_else(ApiError::not_found)?;
    let nama: Option<String> = pek.try_get(1).map_err(internal)?;
    let pagu: Option<f64> = pek.try_get(2).map_err(internal)?;
    let kegiatan_id: Option<i64> = pek.try_get(3).map_err(internal)?;
    let desa_id: Option<i64> = pek.try_get(4).map_err(internal)?;
    let kecamatan_id: Option<i64> = pek.try_get(5).map_err(internal)?;
    let pengawas_id: Option<i64> = pek.try_get(6).map_err(internal)?;

    // Kontrak: pivot dulu, lalu fallback `id_pekerjaan` (paket terbaru).
    let kontrak_id: Option<i64> = match sqlx::query_scalar::<_, i64>(
        "SELECT CAST(kp.kontrak_id AS SIGNED) FROM kontrak_pekerjaan kp WHERE kp.pekerjaan_id = ? ORDER BY kp.kontrak_id LIMIT 1",
    )
    .bind(pekerjaan_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?
    {
        Some(k) => Some(k),
        None => sqlx::query_scalar::<_, i64>("SELECT CAST(id AS SIGNED) FROM tbl_kontrak WHERE id_pekerjaan = ? ORDER BY id DESC LIMIT 1")
            .bind(pekerjaan_id)
            .fetch_optional(pool)
            .await
            .map_err(internal)?,
    };
    let kontrak = match kontrak_id {
        Some(k) => kontrak::find_row(pool, k).await.map_err(internal)?,
        None => None,
    };

    // Baris progres: dibuat bila belum ada (`firstOrCreate`).
    let default = default_content();
    let content = match find_row(pool, pekerjaan_id).await? {
        Some((_, found)) => found,
        None => {
            let mut tx = pool.begin().await.map_err(internal)?;
            let id = insert_row(&mut tx, pekerjaan_id, &default).await?;
            let url = format!(
                "{}/api/progress/pekerjaan/{pekerjaan_id}",
                state.app_url.trim_end_matches('/')
            );
            log_created(&mut tx, &headers, user.user_id, id, pekerjaan_id, &url).await?;
            tx.commit().await.map_err(internal)?;
            default.clone()
        }
    };
    let content = if content.is_null() {
        default.clone()
    } else {
        content
    };

    let items: Vec<Value> = content
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut total_bobot = 0.0f64;
    let mut total_real = 0.0f64;
    let mut total_weighted = 0.0f64;
    let mut max_minggu: i64 = 0;
    for item in &items {
        let bobot = php_float(item.get("bobot"));
        total_bobot += bobot;
        let mut item_real = 0.0f64;
        let mut item_max: i64 = 0;
        if let Some(weekly) = item.get("weekly_data").and_then(Value::as_object) {
            for (minggu, data) in weekly {
                let realisasi = data.get("realisasi");
                if !matches!(realisasi, Some(Value::Null)) {
                    item_real += php_float(realisasi);
                }
                let m: i64 = minggu.parse().unwrap_or(0);
                item_max = item_max.max(m);
            }
        }
        total_real += item_real;
        max_minggu = max_minggu.max(item_max);
        let target = php_float(item.get("target_volume"));
        let percent = if target > 0.0 {
            item_real / target * 100.0
        } else {
            0.0
        };
        total_weighted += percent * bobot / 100.0;
    }
    let week_count = content
        .get("week_count")
        .and_then(Value::as_i64)
        .unwrap_or(4);

    let (kegiatan_json, pengawas_json, desa_nama, kecamatan_nama) = (
        kegiatan_summary(pool, kegiatan_id).await?,
        pengawas_summary(pool, pengawas_id).await?,
        name_of(pool, "tbl_desa", "n_desa", desa_id).await?,
        name_of(pool, "tbl_kecamatan", "n_kec", kecamatan_id).await?,
    );
    let lokasi = format!(
        "{}, {}",
        desa_nama.clone().unwrap_or_default(),
        kecamatan_nama.clone().unwrap_or_default()
    );

    let kontrak_json = match &kontrak {
        None => Value::Null,
        Some(k) => {
            let latest = kontrak::latest_approved_row(pool, k.id).await?;
            let selesai = latest
                .as_ref()
                .and_then(|a| a.tgl_selesai_sesudah)
                .or(k.tgl_selesai);
            let nilai = latest
                .as_ref()
                .and_then(|a| a.nilai_kontrak_sesudah)
                .or(k.nilai_kontrak);
            let mulai = k.tgl_spmk.or(k.tgl_spk);
            json!({
                "tgl_spmk": fmt_date(mulai),
                "tgl_spk": fmt_date(k.tgl_spk),
                "tgl_selesai": fmt_date(selesai),
                "spk": k.spk,
                "spmk": k.spmk,
                "nilai_kontrak": nilai.map_or(Value::Null, number_like_php),
            })
        }
    };
    let penyedia_out = match &kontrak {
        Some(k) => match k.id_penyedia {
            Some(pid) => penyedia_summary(pool, pid).await?,
            None => Value::Null,
        },
        None => Value::Null,
    };

    // Total: PHP memulai dari int 0 bila tidak ada item; di sini ditulis sebagai angka.
    let totals = json!({
        "total_bobot": number_like_php(total_bobot),
        "total_accumulated_real": number_like_php(total_real),
        "total_weighted_progress": number_like_php(total_weighted),
    });

    Ok(Json(json!({
        "success": true,
        "data": {
            "pekerjaan": {
                "id": pekerjaan_id,
                "nama": nama,
                "pagu": pagu.map_or(Value::Null, number_like_php),
                "lokasi": lokasi,
                "desa_nama": desa_nama,
                "kecamatan_nama": kecamatan_nama,
            },
            "kegiatan": kegiatan_json,
            "kontrak": kontrak_json,
            "penyedia": penyedia_out,
            "pengawas": pengawas_json,
            "items": items,
            "totals": totals,
            "max_minggu": max_minggu.max(week_count),
        }
    })))
}

/// `(float)` PHP untuk nilai JSON: angka, string numerik, atau null (0).
fn php_float(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

async fn kegiatan_summary(pool: &MySqlPool, id: Option<i64>) -> Result<Value, ApiError> {
    let Some(id) = id else {
        return Ok(Value::Null);
    };
    let row = sqlx::query(
        "SELECT nama_kegiatan, nama_sub_kegiatan, sumber_dana, tahun_anggaran, nama_pptk, nip_pptk FROM tbl_kegiatan WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    Ok(match row {
        None => Value::Null,
        Some(r) => json!({
            "nama_kegiatan": r.try_get::<Option<String>, _>(0).map_err(internal)?,
            "nama_sub_kegiatan": r.try_get::<Option<String>, _>(1).map_err(internal)?,
            "sumber_dana": r.try_get::<Option<String>, _>(2).map_err(internal)?,
            "tahun_anggaran": r.try_get::<Option<String>, _>(3).map_err(internal)?,
            "nama_pptk": r.try_get::<Option<String>, _>(4).map_err(internal)?,
            "nip_pptk": r.try_get::<Option<String>, _>(5).map_err(internal)?,
        }),
    })
}

async fn pengawas_summary(pool: &MySqlPool, id: Option<i64>) -> Result<Value, ApiError> {
    let Some(id) = id else {
        return Ok(Value::Null);
    };
    let row = sqlx::query("SELECT nama, nip, jabatan FROM pengawas WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    Ok(match row {
        None => Value::Null,
        Some(r) => json!({
            "nama": r.try_get::<Option<String>, _>(0).map_err(internal)?,
            "nip": r.try_get::<Option<String>, _>(1).map_err(internal)?,
            "jabatan": r.try_get::<Option<String>, _>(2).map_err(internal)?,
        }),
    })
}

async fn penyedia_summary(pool: &MySqlPool, id: i64) -> Result<Value, ApiError> {
    let row = sqlx::query("SELECT nama, direktur FROM tbl_penyedia WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    Ok(match row {
        None => Value::Null,
        Some(r) => json!({
            "nama": r.try_get::<Option<String>, _>(0).map_err(internal)?,
            "direktur": r.try_get::<Option<String>, _>(1).map_err(internal)?,
        }),
    })
}

/// Nama dari tabel wilayah (`tbl_desa.n_desa`, `tbl_kecamatan.n_kec`); `None` bila relasi kosong.
async fn name_of(
    pool: &MySqlPool,
    table: &str,
    column: &str,
    id: Option<i64>,
) -> Result<Option<String>, ApiError> {
    let Some(id) = id else {
        return Ok(None);
    };
    let sql = format!("SELECT {column} FROM {table} WHERE id = ?");
    let v: Option<Option<String>> = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    Ok(v.flatten())
}
