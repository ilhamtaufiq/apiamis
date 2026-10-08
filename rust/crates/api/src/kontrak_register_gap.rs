//! Celah register addendum: register dokumen bertipe ADD/Addendum yang belum punya addendum.
//!
//! Mengikuti `KontrakAddendumRegisterGapService::findGaps` dan `findGapsForKontrak`.
//! `notify-pengawas` tidak dipindah: ia mengirim email lewat SMTP dari `app_settings`, dan
//! Rust belum punya mailer. Rute itu tetap di Laravel.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDate;
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    format::number_like_php,
    kontrak::{self, KontrakRow},
    kontrak_addendum::{actor, authorize_admin, authorize_view_kontrak, find_kontrak},
    AppState,
};

/// Kode tipe dokumen yang dihitung sebagai addendum (sama dengan `ADDENDUM_TYPE_CODES`).
const TYPE_CODES: [&str; 2] = ["add", "addendum"];

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// `normalizeNomor`: trim, lalu spasi berurutan jadi satu spasi, lalu huruf besar.
fn normalize_nomor(raw: &str) -> String {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.to_uppercase()
}

fn fmt_date(d: Option<NaiveDate>) -> Value {
    d.map_or(Value::Null, |d| json!(d.format("%Y-%m-%d").to_string()))
}

/// Item celah (satu register) beserta data kontrak, pekerjaan, penyedia, dan pengawas.
async fn gap_items(pool: &MySqlPool) -> Result<Vec<(i64, Value)>, ApiError> {
    let type_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM tbl_document_types WHERE LOWER(TRIM(code)) IN ('add', 'addendum') ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    if type_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; type_ids.len()].join(", ");
    let sql = format!(
        "SELECT CAST(r.id AS SIGNED), r.nomor, r.tanggal, r.description, CAST(r.nilai AS DOUBLE), \
         CAST(r.kontrak_id AS SIGNED), t.code, t.name \
         FROM tbl_document_registers r LEFT JOIN tbl_document_types t ON t.id = r.type_id \
         WHERE r.type_id IN ({placeholders}) AND r.addendum_id IS NULL \
         ORDER BY r.tanggal DESC, r.id DESC"
    );
    let mut q = sqlx::query(&sql);
    for id in &type_ids {
        q = q.bind(id);
    }
    let registers = q.fetch_all(pool).await.map_err(internal)?;

    let mut out = Vec::new();
    for r in &registers {
        let register_id: i64 = r.try_get(0).map_err(internal)?;
        let nomor: String = r.try_get(1).map_err(internal)?;
        let tanggal: Option<NaiveDate> = r.try_get(2).map_err(internal)?;
        let description: Option<String> = r.try_get(3).map_err(internal)?;
        let nilai: Option<f64> = r.try_get(4).map_err(internal)?;
        let kontrak_id: i64 = r.try_get(5).map_err(internal)?;
        let type_code: Option<String> = r.try_get(6).map_err(internal)?;
        let type_name: Option<String> = r.try_get(7).map_err(internal)?;

        let Some(kontrak) = kontrak::find_row(pool, kontrak_id)
            .await
            .map_err(internal)?
        else {
            continue;
        };
        if normalize_nomor(&nomor).is_empty() {
            continue;
        }
        // Addendum sudah dibuat (apa pun statusnya): bukan celah.
        let addendum_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tbl_kontrak_addendums WHERE kontrak_id = ?")
                .bind(kontrak_id)
                .fetch_one(pool)
                .await
                .map_err(internal)?;
        if addendum_count > 0 {
            continue;
        }

        let pekerjaan = pekerjaan_with_pengawas(pool, &kontrak).await?;
        let pekerjaans = pekerjaan_list(pool, kontrak_id).await?;
        let penyedia = penyedia_summary(pool, kontrak.id_penyedia).await?;

        out.push((
            register_id,
            json!({
                "register_id": register_id,
                "nomor_register": nomor,
                "tanggal_register": fmt_date(tanggal),
                "type_code": type_code,
                "type_name": type_name,
                "description": description,
                "nilai": nilai.map_or(Value::Null, number_like_php),
                "kontrak_id": kontrak_id,
                "addendum_id": Value::Null,
                "addendum_count": addendum_count,
                "pekerjaan": pekerjaan.as_ref().map(|p| p.summary.clone()).unwrap_or(Value::Null),
                "pekerjaans": pekerjaans,
                "penyedia": penyedia,
                "pengawas": pekerjaan.as_ref().map(|p| p.pengawas.clone()).unwrap_or(Value::Null),
            }),
        ));
    }
    Ok(out)
}

struct PekerjaanInfo {
    summary: Value,
    pengawas: Value,
}

/// `kontrak.pekerjaan` (id_pekerjaan) dan `pekerjaan.pengawas` (tabel `pengawas`).
async fn pekerjaan_with_pengawas(
    pool: &MySqlPool,
    kontrak: &KontrakRow,
) -> Result<Option<PekerjaanInfo>, ApiError> {
    let Some(pid) = kontrak.id_pekerjaan else {
        return Ok(None);
    };
    let row = sqlx::query(
        "SELECT CAST(p.id AS SIGNED), p.nama_paket, p.kode_rekening, CAST(p.pengawas_id AS SIGNED) \
         FROM tbl_pekerjaan p WHERE p.id = ?",
    )
    .bind(pid)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(r) = row else {
        return Ok(None);
    };
    let id: i64 = r.try_get(0).map_err(internal)?;
    let nama: Option<String> = r.try_get(1).map_err(internal)?;
    let kode: Option<String> = r.try_get(2).map_err(internal)?;
    let pengawas_id: Option<i64> = r.try_get(3).map_err(internal)?;
    let pengawas = match pengawas_id {
        None => Value::Null,
        Some(pw) => {
            let prow = sqlx::query("SELECT CAST(id AS SIGNED), nama FROM pengawas WHERE id = ?")
                .bind(pw)
                .fetch_optional(pool)
                .await
                .map_err(internal)?;
            match prow {
                Some(p) => json!({
                    "id": p.try_get::<i64, _>(0).map_err(internal)?,
                    "nama": p.try_get::<Option<String>, _>(1).map_err(internal)?,
                }),
                None => Value::Null,
            }
        }
    };
    Ok(Some(PekerjaanInfo {
        summary: json!({ "id": id, "nama_paket": nama, "kode_rekening": kode }),
        pengawas,
    }))
}

/// `kontrak.pekerjaans` (pivot), urut id.
async fn pekerjaan_list(pool: &MySqlPool, kontrak_id: i64) -> Result<Vec<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(p.id AS SIGNED), p.nama_paket, p.kode_rekening FROM kontrak_pekerjaan kp \
         JOIN tbl_pekerjaan p ON p.id = kp.pekerjaan_id WHERE kp.kontrak_id = ? ORDER BY p.id",
    )
    .bind(kontrak_id)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    rows.iter()
        .map(|r| {
            Ok(json!({
                "id": r.try_get::<i64, _>(0).map_err(internal)?,
                "nama_paket": r.try_get::<Option<String>, _>(1).map_err(internal)?,
                "kode_rekening": r.try_get::<Option<String>, _>(2).map_err(internal)?,
            }))
        })
        .collect()
}

async fn penyedia_summary(pool: &MySqlPool, id: Option<i64>) -> Result<Value, ApiError> {
    let Some(id) = id else {
        return Ok(Value::Null);
    };
    let row = sqlx::query("SELECT CAST(id AS SIGNED), nama FROM tbl_penyedia WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    Ok(match row {
        Some(r) => json!({
            "id": r.try_get::<i64, _>(0).map_err(internal)?,
            "nama": r.try_get::<Option<String>, _>(1).map_err(internal)?,
        }),
        None => Value::Null,
    })
}

fn payload(items: Vec<Value>) -> Value {
    json!({
        "total": items.len(),
        "items": items,
        "type_codes": TYPE_CODES,
    })
}

/// `GET /api/kontrak-addendums/register-gaps`: admin.
pub async fn register_gaps(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    authorize_admin(&a)?;
    let items = gap_items(&state.pool)
        .await?
        .into_iter()
        .map(|(_, v)| v)
        .collect();
    Ok(Json(payload(items)).into_response())
}

/// `GET /api/kontrak/{id}/addendum-register-gaps`: celah untuk satu kontrak, sesuai akses kontrak.
pub async fn register_gaps_for_kontrak(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let k = find_kontrak(&state.pool, id).await?;
    authorize_view_kontrak(&state, &a, &k).await?;
    let items: Vec<Value> = gap_items(&state.pool)
        .await?
        .into_iter()
        .filter(|(_, v)| v["kontrak_id"] == json!(id))
        .map(|(_, v)| v)
        .collect();
    Ok(Json(payload(items)).into_response())
}
