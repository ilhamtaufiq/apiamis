//! `PekerjaanDetailResource` untuk `GET /api/pekerjaan/{id}`.
//!
//! Berbeda dari daftar (`PekerjaanResource`): relasi dimuat penuh (foto, berkas, kontrak dengan penyedia,
//! output, penerima, tags, progres), tanpa metrik progres/estimasi, dan `assignment_sources` hanya
//! `manual` dan `role` (tanpa pengawas/pendamping), seperti Laravel.

use axum::http::HeaderMap;
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};
use std::collections::HashMap;

use crate::{
    berkas, desa::internal, format::iso8601_utc, format::number_like_php, foto, pekerjaan,
    pekerjaan_rel, penerima, penyedia, AppState,
};

/// Key yang sama di `PekerjaanDetailResource` dan daftar: diambil dari `to_resource`.
const SHARED_KEYS: &[&str] = &[
    "id",
    "kode_rekening",
    "nama_paket",
    "pagu",
    "is_konsultan",
    "status",
    "catatan",
    "has_kontrak",
    "kontrak_count",
    "foto_count",
    "foto_required_count",
    "foto_status",
    "kecamatan_id",
    "desa_id",
    "kegiatan_id",
    "pengawas_id",
    "pendamping_id",
];

/// Susun resource detail untuk satu pekerjaan yang sudah lolos pemeriksaan akses.
pub async fn build(
    state: &AppState,
    headers: &HeaderMap,
    row: &pekerjaan::PekerjaanRow,
    roles: &[(u64, String)],
    actor: u64,
    query: &HashMap<String, String>,
) -> Result<Value, ApiError> {
    let pool = &state.pool;
    let viewer = pekerjaan_rel::viewer(pool, actor, roles)
        .await
        .map_err(internal)?;
    // Mode summary memuat output dan foto, sehingga metrik foto memakai cabang lengkap.
    let rel = pekerjaan::load(
        pool,
        std::slice::from_ref(row),
        pekerjaan::Mode {
            summary: true,
            unbounded: false,
        },
        &viewer,
    )
    .await
    .map_err(internal)?;
    let base = pekerjaan::to_resource(row, &rel);

    let mut out = Map::new();
    for key in SHARED_KEYS {
        out.insert(
            (*key).into(),
            base.get(*key).cloned().unwrap_or(Value::Null),
        );
    }

    // `assignment_sources` tanpa pengawas dan pendamping.
    let mut without_nip = viewer.clone();
    without_nip.nip = None;
    let sources =
        pekerjaan_rel::assignment_sources(pool, &without_nip, row.id, row.kegiatan_id, None, None)
            .await
            .map_err(internal)?;
    out.insert("assignment_sources".into(), json!(sources));

    out.insert(
        "kecamatan".into(),
        row.kecamatan_id
            .and_then(|k| rel.kecamatan.get(&k).cloned())
            .unwrap_or(Value::Null),
    );
    out.insert(
        "desa".into(),
        row.desa_id
            .and_then(|d| rel.desa.get(&d).cloned())
            .unwrap_or(Value::Null),
    );
    out.insert(
        "kegiatan".into(),
        row.kegiatan_id
            .and_then(|k| rel.kegiatan.get(&k).cloned())
            .unwrap_or(Value::Null),
    );
    out.insert(
        "pengawas".into(),
        match row.pengawas_id {
            Some(id) => pekerjaan::pengawas_resource(pool, id)
                .await
                .map_err(internal)?
                .unwrap_or(Value::Null),
            None => Value::Null,
        },
    );
    out.insert(
        "pendamping".into(),
        match row.pendamping_id {
            Some(id) => pekerjaan::pengawas_resource(pool, id)
                .await
                .map_err(internal)?
                .unwrap_or(Value::Null),
            None => Value::Null,
        },
    );

    let mut fotos = Vec::new();
    for f in foto::rows_for_pekerjaan(pool, row.id as i64).await? {
        fotos.push(foto::nested_resource(pool, &state.app_url, &f).await?);
    }
    out.insert("foto".into(), Value::Array(fotos));

    let mut berkas_list = Vec::new();
    for b in berkas::rows_for_pekerjaan(pool, row.id as i64).await? {
        berkas_list.push(berkas::nested_resource(pool, &state.app_url, &b).await?);
    }
    out.insert("berkas".into(), Value::Array(berkas_list));

    out.insert(
        "kontrak".into(),
        Value::Array(kontrak(state, row.id).await?),
    );

    let mut outputs = Vec::new();
    for o in pekerjaan_rel::outputs_for(pool, row.id)
        .await
        .map_err(internal)?
    {
        outputs.push(o.to_resource());
    }
    out.insert("output".into(), Value::Array(outputs));

    let unmasked = penerima::unmasked_for(pool, headers, query).await?;
    let penerima_rows = penerima::rows_for_pekerjaan(pool, row.id as i64).await?;
    out.insert(
        "penerima".into(),
        Value::Array(penerima::nested(&penerima_rows, unmasked).await?),
    );

    out.insert(
        "tags".into(),
        Value::Array(rel.tags.get(&row.id).cloned().unwrap_or_default()),
    );
    out.insert("progress".into(), progress(pool, row.id).await?);

    out.insert("created_at".into(), iso8601_utc(row.created_at));
    out.insert("updated_at".into(), iso8601_utc(row.updated_at));
    Ok(Value::Object(out))
}

/// `isChecklistComplete()`: ada item checklist dan semuanya tercentang.
pub(crate) async fn checklist_complete(pool: &MySqlPool, pekerjaan_id: u64) -> Result<bool, ApiError> {
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pekerjaan_checklist pc JOIN tbl_checklist_items ci ON ci.id = pc.checklist_item_id \
         WHERE pc.pekerjaan_id = ?",
    )
    .bind(pekerjaan_id)
    .fetch_one(pool)
    .await
    .map_err(internal)?;
    if total == 0 {
        return Ok(false);
    }
    let checked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pekerjaan_checklist pc JOIN tbl_checklist_items ci ON ci.id = pc.checklist_item_id \
         WHERE pc.pekerjaan_id = ? AND pc.is_checked = 1",
    )
    .bind(pekerjaan_id)
    .fetch_one(pool)
    .await
    .map_err(internal)?;
    Ok(checked == total)
}

/// `KontrakResource` dengan `penyedia` (relasi `kontrak.penyedia`). Tanpa `kegiatan` dan `pekerjaans`.
async fn kontrak(state: &AppState, pekerjaan_id: u64) -> Result<Vec<Value>, ApiError> {
    let pool = &state.pool;
    let rows = sqlx::query(
        "SELECT CAST(k.id AS SIGNED) AS id, k.kode_rup, k.kode_paket, k.nomor_penawaran, k.tanggal_penawaran, \
         CAST(k.nilai_kontrak AS DOUBLE) AS nilai_kontrak, k.tgl_sppbj, k.tgl_spk, k.tgl_spmk, k.tgl_selesai, \
         k.sppbj, k.spk, k.spmk, k.spse_sppbj_id, k.spse_spk_id, k.spse_rekanan_id, k.spse_pushed_at, \
         CAST(k.id_kegiatan AS SIGNED) AS id_kegiatan, CAST(k.id_penyedia AS SIGNED) AS id_penyedia, \
         k.created_at, k.updated_at \
         FROM tbl_kontrak k JOIN kontrak_pekerjaan kp ON kp.kontrak_id = k.id \
         WHERE kp.pekerjaan_id = ? ORDER BY k.id",
    )
    .bind(pekerjaan_id as i64)
    .fetch_all(pool)
    .await
    .map_err(internal)?;

    let fmt = |d: Option<NaiveDate>| d.map(|d| d.format("%Y-%m-%d").to_string());
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let kid: i64 = r.try_get("id").map_err(internal)?;
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT CAST(pekerjaan_id AS SIGNED) FROM kontrak_pekerjaan WHERE kontrak_id = ? ORDER BY pekerjaan_id",
        )
        .bind(kid)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
        // `$this->pekerjaans->every(...)`: koleksi kosong dianggap lengkap.
        let mut complete = true;
        for id in &ids {
            if !checklist_complete(pool, *id as u64).await? {
                complete = false;
                break;
            }
        }

        let id_penyedia: Option<i64> = r.try_get("id_penyedia").map_err(internal)?;
        let penyedia_json = match id_penyedia {
            Some(p) => match penyedia::find(pool, p as u64).await.map_err(internal)? {
                Some(row) => penyedia::with_dokumen(state, &row)
                    .await
                    .map_err(internal)?,
                None => Value::Null,
            },
            None => Value::Null,
        };

        let nilai: Option<f64> = r.try_get("nilai_kontrak").map_err(internal)?;
        let spse_pushed: Option<DateTime<Utc>> = r.try_get("spse_pushed_at").map_err(internal)?;
        let tanggal_penawaran: Option<NaiveDate> =
            r.try_get("tanggal_penawaran").map_err(internal)?;
        let tgl_sppbj: Option<NaiveDate> = r.try_get("tgl_sppbj").map_err(internal)?;
        let tgl_spk: Option<NaiveDate> = r.try_get("tgl_spk").map_err(internal)?;
        let tgl_spmk: Option<NaiveDate> = r.try_get("tgl_spmk").map_err(internal)?;
        let tgl_selesai: Option<NaiveDate> = r.try_get("tgl_selesai").map_err(internal)?;
        let created: Option<DateTime<Utc>> = r.try_get("created_at").map_err(internal)?;
        let updated: Option<DateTime<Utc>> = r.try_get("updated_at").map_err(internal)?;

        out.push(json!({
            "id": kid,
            "kode_rup": r.try_get::<Option<String>, _>("kode_rup").map_err(internal)?,
            "kode_paket": r.try_get::<Option<String>, _>("kode_paket").map_err(internal)?,
            "nomor_penawaran": r.try_get::<Option<String>, _>("nomor_penawaran").map_err(internal)?,
            "tanggal_penawaran": fmt(tanggal_penawaran),
            "nilai_kontrak": nilai.map(number_like_php),
            "tgl_sppbj": fmt(tgl_sppbj),
            "tgl_spk": fmt(tgl_spk),
            "tgl_spmk": fmt(tgl_spmk),
            "tgl_selesai": fmt(tgl_selesai),
            "sppbj": r.try_get::<Option<String>, _>("sppbj").map_err(internal)?,
            "spk": r.try_get::<Option<String>, _>("spk").map_err(internal)?,
            "spmk": r.try_get::<Option<String>, _>("spmk").map_err(internal)?,
            "spse_sppbj_id": r.try_get::<Option<String>, _>("spse_sppbj_id").map_err(internal)?,
            "spse_spk_id": r.try_get::<Option<String>, _>("spse_spk_id").map_err(internal)?,
            "spse_rekanan_id": r.try_get::<Option<String>, _>("spse_rekanan_id").map_err(internal)?,
            "spse_pushed_at": iso8601_utc(spse_pushed),
            "id_kegiatan": r.try_get::<Option<i64>, _>("id_kegiatan").map_err(internal)?,
            "pekerjaan_ids": ids,
            "id_penyedia": id_penyedia,
            "penyedia": penyedia_json,
            "is_checklist_complete": complete,
            "created_at": iso8601_utc(created),
            "updated_at": iso8601_utc(updated),
        }));
    }
    Ok(out)
}

/// `ProgressResource` (`content` berupa JSON) atau null bila belum ada progres.
async fn progress(pool: &MySqlPool, pekerjaan_id: u64) -> Result<Value, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, \
         CAST(content AS CHAR) AS content, created_at, updated_at FROM tbl_progress \
         WHERE pekerjaan_id = ? ORDER BY id LIMIT 1",
    )
    .bind(pekerjaan_id as i64)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let Some(r) = row else {
        return Ok(Value::Null);
    };
    let content: Option<String> = r.try_get("content").map_err(internal)?;
    let created: Option<DateTime<Utc>> = r.try_get("created_at").map_err(internal)?;
    let updated: Option<DateTime<Utc>> = r.try_get("updated_at").map_err(internal)?;
    Ok(json!({
        "id": r.try_get::<i64, _>("id").map_err(internal)?,
        "pekerjaan_id": r.try_get::<i64, _>("pekerjaan_id").map_err(internal)?,
        "content": content.and_then(|c| serde_json::from_str::<Value>(&c).ok()).unwrap_or(Value::Null),
        "created_at": iso8601_utc(created),
        "updated_at": iso8601_utc(updated),
    }))
}
