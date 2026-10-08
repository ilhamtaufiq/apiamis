//! `/api/kontrak`: port `KontrakController` (daftar, store, show, update, destroy, dan daftar per
//! pekerjaan, kegiatan, dan penyedia) serta resource addendum yang dimuat di detail kontrak.
//!
//! Belum dipindah (tetap di Laravel): ekspor (`export`, `export-all-covers`, `export-cover`, `export-bap`,
//! `bap-context`, `import`, `import/template`, `export/excel`), dan rute `kontrak-addendums` (step 2).
//!
//! Pembatasan per pekerjaan (T36) belum diterapkan pada kontrak; ini mengikuti Laravel yang tidak membatasi
//! baca kontrak per pekerjaan (lihat T36).

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, QueryBuilder, Row};

use crate::{
    changes, format::iso8601_utc, format::number_like_php, foto, kegiatan, lookup::carbon_json, media,
    pagination, pekerjaan, pekerjaan_detail, pekerjaan_rel, penyedia, require_auth, AppState,
};

const MODEL_ADDENDUM: &str = "App\\Models\\KontrakAddendum";
const COLLECTION_ADDENDUM: &str = "kontrak/addendum";

/// Kolom tabel `tbl_kontrak` yang bisa diisi lewat store dan update, urut tetap untuk audit.
const COLUMNS: &[&str] = &[
    "id_kegiatan",
    "id_pekerjaan",
    "id_penyedia",
    "kode_rup",
    "kode_paket",
    "nomor_penawaran",
    "tanggal_penawaran",
    "nilai_kontrak",
    "tgl_sppbj",
    "tgl_spk",
    "tgl_spmk",
    "tgl_selesai",
    "sppbj",
    "spk",
    "spmk",
];

fn internal(e: impl std::fmt::Display) -> ApiError {
    media::internal(e)
}

// ---------------------------------------------------------------------------
// Baris dan nilai
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct KontrakRow {
    pub id: i64,
    pub id_kegiatan: Option<i64>,
    pub id_pekerjaan: Option<i64>,
    pub id_penyedia: Option<i64>,
    pub kode_rup: Option<String>,
    pub kode_paket: Option<String>,
    pub nomor_penawaran: Option<String>,
    pub tanggal_penawaran: Option<NaiveDate>,
    pub nilai_kontrak: Option<f64>,
    pub tgl_sppbj: Option<NaiveDate>,
    pub tgl_spk: Option<NaiveDate>,
    pub tgl_spmk: Option<NaiveDate>,
    pub tgl_selesai: Option<NaiveDate>,
    pub sppbj: Option<String>,
    pub spk: Option<String>,
    pub spmk: Option<String>,
    pub spse_sppbj_id: Option<String>,
    pub spse_spk_id: Option<String>,
    pub spse_rekanan_id: Option<String>,
    pub spse_pushed_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

const SELECT_KONTRAK: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(id_kegiatan AS SIGNED) AS id_kegiatan, \
     CAST(id_pekerjaan AS SIGNED) AS id_pekerjaan, CAST(id_penyedia AS SIGNED) AS id_penyedia, kode_rup, kode_paket, \
     nomor_penawaran, tanggal_penawaran, CAST(nilai_kontrak AS DOUBLE) AS nilai_kontrak, tgl_sppbj, tgl_spk, tgl_spmk, \
     tgl_selesai, sppbj, spk, spmk, spse_sppbj_id, spse_spk_id, spse_rekanan_id, spse_pushed_at, created_at, updated_at \
     FROM tbl_kontrak";

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<KontrakRow, sqlx::Error> {
    Ok(KontrakRow {
        id: r.try_get("id")?,
        id_kegiatan: r.try_get("id_kegiatan")?,
        id_pekerjaan: r.try_get("id_pekerjaan")?,
        id_penyedia: r.try_get("id_penyedia")?,
        kode_rup: r.try_get("kode_rup")?,
        kode_paket: r.try_get("kode_paket")?,
        nomor_penawaran: r.try_get("nomor_penawaran")?,
        tanggal_penawaran: r.try_get("tanggal_penawaran")?,
        nilai_kontrak: r.try_get("nilai_kontrak")?,
        tgl_sppbj: r.try_get("tgl_sppbj")?,
        tgl_spk: r.try_get("tgl_spk")?,
        tgl_spmk: r.try_get("tgl_spmk")?,
        tgl_selesai: r.try_get("tgl_selesai")?,
        sppbj: r.try_get("sppbj")?,
        spk: r.try_get("spk")?,
        spmk: r.try_get("spmk")?,
        spse_sppbj_id: r.try_get("spse_sppbj_id")?,
        spse_spk_id: r.try_get("spse_spk_id")?,
        spse_rekanan_id: r.try_get("spse_rekanan_id")?,
        spse_pushed_at: r.try_get("spse_pushed_at")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn find_row<'e, E>(exec: E, id: i64) -> Result<Option<KontrakRow>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = MySql>,
{
    let sql = format!("{SELECT_KONTRAK} WHERE id = ?");
    let row = sqlx::query(&sql).bind(id).fetch_optional(exec).await?;
    row.as_ref().map(map_row).transpose()
}

fn fmt_date(d: Option<NaiveDate>) -> Option<String> {
    d.map(|d| d.format("%Y-%m-%d").to_string())
}

fn col_json(row: &KontrakRow, col: &str) -> Value {
    match col {
        "id_kegiatan" => json!(row.id_kegiatan),
        "id_pekerjaan" => json!(row.id_pekerjaan),
        "id_penyedia" => json!(row.id_penyedia),
        "kode_rup" => json!(row.kode_rup),
        "kode_paket" => json!(row.kode_paket),
        "nomor_penawaran" => json!(row.nomor_penawaran),
        "tanggal_penawaran" => json!(fmt_date(row.tanggal_penawaran)),
        "nilai_kontrak" => json!(row.nilai_kontrak),
        "tgl_sppbj" => json!(fmt_date(row.tgl_sppbj)),
        "tgl_spk" => json!(fmt_date(row.tgl_spk)),
        "tgl_spmk" => json!(fmt_date(row.tgl_spmk)),
        "tgl_selesai" => json!(fmt_date(row.tgl_selesai)),
        "sppbj" => json!(row.sppbj),
        "spk" => json!(row.spk),
        "spmk" => json!(row.spmk),
        _ => Value::Null,
    }
}

/// `getAttributes()` untuk audit `created` dan `deleted`.
fn attributes(row: &KontrakRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    for col in COLUMNS {
        m.insert((*col).into(), col_json(row, col));
    }
    m.insert("created_at".into(), carbon_json(row.created_at));
    m.insert("updated_at".into(), carbon_json(row.updated_at));
    m
}

// ---------------------------------------------------------------------------
// Relasi: pekerjaan, kegiatan, penyedia, addendum
// ---------------------------------------------------------------------------

/// ID pekerjaan pada pivot `kontrak_pekerjaan`, urut id pekerjaan.
async fn pekerjaan_ids(pool: &MySqlPool, kontrak_id: i64) -> Result<Vec<i64>, ApiError> {
    sqlx::query_scalar("SELECT CAST(pekerjaan_id AS SIGNED) FROM kontrak_pekerjaan WHERE kontrak_id = ? ORDER BY pekerjaan_id")
        .bind(kontrak_id)
        .fetch_all(pool)
        .await
        .map_err(internal)
}

/// `PekerjaanResource` untuk daftar pekerjaan kontrak (relasi dasar dimuat).
async fn pekerjaan_resources(state: &AppState, actor: u64, ids: &[i64]) -> Result<Vec<Value>, ApiError> {
    let mut rows = Vec::new();
    for id in ids {
        if let Some(p) = pekerjaan::find(&state.pool, *id as u64).await.map_err(internal)? {
            rows.push(p);
        }
    }
    let roles = auth::login::roles_of(&state.pool, actor).await.map_err(internal)?;
    let viewer = pekerjaan_rel::viewer(&state.pool, actor, &roles).await.map_err(internal)?;
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
    Ok(rows.iter().map(|p| pekerjaan::to_resource(p, &rel)).collect())
}

/// `KegiatanResource`, atau null bila kegiatan tidak ada.
async fn kegiatan_json(pool: &MySqlPool, id: Option<i64>) -> Result<Value, ApiError> {
    let Some(id) = id else {
        return Ok(Value::Null);
    };
    Ok(match kegiatan::find(pool, id as u64).await.map_err(internal)? {
        Some(k) => kegiatan::to_resource(&k),
        None => Value::Null,
    })
}

/// `PenyediaResource` (dengan dokumen), atau null.
async fn penyedia_json(state: &AppState, id: Option<i64>) -> Result<Value, ApiError> {
    let Some(id) = id else {
        return Ok(Value::Null);
    };
    match penyedia::find(&state.pool, id as u64).await.map_err(internal)? {
        Some(p) => penyedia::with_dokumen(state, &p).await.map_err(internal),
        None => Ok(Value::Null),
    }
}

/// `Kontrak::normalizePaketNameForCompare` dan `sanitizeSpsePaketName`: decode entitas HTML, buang tag,
/// rapikan spasi. Decode entitas dibatasi pada yang umum (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;`, `&nbsp;`, numerik).
fn sanitize_spse(value: &str) -> String {
    let decoded = decode_entities(value);
    let mut without_tags = String::with_capacity(decoded.len());
    let mut in_tag = false;
    for c in decoded.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => without_tags.push(c),
            _ => {}
        }
    }
    without_tags.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entities(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let Some(end) = tail.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn normalize_name(value: &str) -> String {
    sanitize_spse(value)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// `Kontrak::spseNamaPaketIfDifferent`: nama paket dari staging SPSE bila berbeda dari semua paket kontrak.
async fn spse_nama_paket(pool: &MySqlPool, row: &KontrakRow, pekerjaan_names: &[String]) -> Result<Option<String>, ApiError> {
    let kode = row.kode_paket.as_deref().unwrap_or("").trim().to_string();
    if kode.is_empty() {
        return Ok(None);
    }
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT nama_paket FROM tbl_procurement_staging_paket WHERE kode_paket = ? ORDER BY fetched_at DESC, id DESC LIMIT 1",
    )
    .bind(&kode)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    let spse = sanitize_spse(&raw.unwrap_or_default());
    if spse.is_empty() {
        return Ok(None);
    }
    let normalized = normalize_name(&spse);
    if pekerjaan_names.iter().any(|n| normalize_name(n) == normalized) {
        return Ok(None);
    }
    Ok(Some(spse))
}

/// Ringkasan addendum yang disetujui terbaru (`latestApprovedAddendum`).
async fn latest_approved_row(pool: &MySqlPool, kontrak_id: i64) -> Result<Option<AddendumRow>, ApiError> {
    let sql = format!(
        "{SELECT_ADDENDUM} WHERE kontrak_id = ? AND status = 'disetujui' ORDER BY addendum_ke DESC, id DESC LIMIT 1"
    );
    let row = sqlx::query(&sql)
        .bind(kontrak_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    row.as_ref().map(map_addendum).transpose().map_err(internal)
}

// ---------------------------------------------------------------------------
// Addendum (resource yang dimuat di detail kontrak)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct AddendumRow {
    pub id: i64,
    pub kontrak_id: i64,
    pub addendum_ke: i64,
    pub nomor_addendum: Option<String>,
    pub attachment_nomors: Option<String>,
    pub tanggal_addendum: Option<NaiveDate>,
    pub jenis_addendum: Option<String>,
    pub alasan: Option<String>,
    pub deskripsi_perubahan: Option<String>,
    pub nilai_kontrak_sebelum: Option<f64>,
    pub nilai_kontrak_sesudah: Option<f64>,
    pub tgl_selesai_sebelum: Option<NaiveDate>,
    pub tgl_selesai_sesudah: Option<NaiveDate>,
    pub status: String,
    pub kelengkapan_override: bool,
    pub created_by: Option<i64>,
    pub approved_by: Option<i64>,
    pub approved_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

const SELECT_ADDENDUM: &str = "SELECT CAST(id AS SIGNED) AS id, CAST(kontrak_id AS SIGNED) AS kontrak_id, \
     CAST(addendum_ke AS SIGNED) AS addendum_ke, nomor_addendum, CAST(attachment_nomors AS CHAR) AS attachment_nomors, \
     tanggal_addendum, jenis_addendum, alasan, deskripsi_perubahan, CAST(nilai_kontrak_sebelum AS DOUBLE) AS nilai_kontrak_sebelum, \
     CAST(nilai_kontrak_sesudah AS DOUBLE) AS nilai_kontrak_sesudah, tgl_selesai_sebelum, tgl_selesai_sesudah, status, \
     kelengkapan_override, CAST(created_by AS SIGNED) AS created_by, CAST(approved_by AS SIGNED) AS approved_by, \
     approved_at, created_at, updated_at FROM tbl_kontrak_addendums";

fn map_addendum(r: &sqlx::mysql::MySqlRow) -> Result<AddendumRow, sqlx::Error> {
    Ok(AddendumRow {
        id: r.try_get("id")?,
        kontrak_id: r.try_get("kontrak_id")?,
        addendum_ke: r.try_get("addendum_ke")?,
        nomor_addendum: r.try_get("nomor_addendum")?,
        attachment_nomors: r.try_get("attachment_nomors")?,
        tanggal_addendum: r.try_get("tanggal_addendum")?,
        jenis_addendum: r.try_get("jenis_addendum")?,
        alasan: r.try_get("alasan")?,
        deskripsi_perubahan: r.try_get("deskripsi_perubahan")?,
        nilai_kontrak_sebelum: r.try_get("nilai_kontrak_sebelum")?,
        nilai_kontrak_sesudah: r.try_get("nilai_kontrak_sesudah")?,
        tgl_selesai_sebelum: r.try_get("tgl_selesai_sebelum")?,
        tgl_selesai_sesudah: r.try_get("tgl_selesai_sesudah")?,
        status: r.try_get("status")?,
        kelengkapan_override: r.try_get("kelengkapan_override")?,
        created_by: r.try_get("created_by")?,
        approved_by: r.try_get("approved_by")?,
        approved_at: r.try_get("approved_at")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

/// `KontrakAddendumResource` tanpa `creator`, `approver`, dan `kontrak` (tidak dimuat di detail).
/// `items` hanya ada bila `with_items` (relasi `addendums.items` dimuat).
async fn addendum_json(state: &AppState, row: &AddendumRow, with_items: bool) -> Result<Value, ApiError> {
    let pool = &state.pool;
    let attachment_nomors: Value = row
        .attachment_nomors
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or(Value::Null);
    let mut out = Map::new();
    out.insert("id".into(), json!(row.id));
    out.insert("kontrak_id".into(), json!(row.kontrak_id));
    out.insert("addendum_ke".into(), json!(row.addendum_ke));
    out.insert("nomor_addendum".into(), json!(row.nomor_addendum));
    out.insert("attachment_nomors".into(), attachment_nomors);
    out.insert("tanggal_addendum".into(), json!(fmt_date(row.tanggal_addendum)));
    out.insert("jenis_addendum".into(), json!(row.jenis_addendum));
    out.insert("alasan".into(), json!(row.alasan));
    out.insert("deskripsi_perubahan".into(), json!(row.deskripsi_perubahan));
    out.insert("nilai_kontrak_sebelum".into(), row.nilai_kontrak_sebelum.map_or(Value::Null, number_like_php));
    out.insert("nilai_kontrak_sesudah".into(), row.nilai_kontrak_sesudah.map_or(Value::Null, number_like_php));
    out.insert("tgl_selesai_sebelum".into(), json!(fmt_date(row.tgl_selesai_sebelum)));
    out.insert("tgl_selesai_sesudah".into(), json!(fmt_date(row.tgl_selesai_sesudah)));
    out.insert("status".into(), json!(row.status));
    out.insert("kelengkapan_override".into(), json!(row.kelengkapan_override));
    out.insert("created_by".into(), json!(row.created_by));
    out.insert("approved_by".into(), json!(row.approved_by));
    out.insert("approved_at".into(), iso8601_utc(row.approved_at));
    out.insert("can_submit".into(), json!(row.status == "draft" || row.status == "ditolak"));
    out.insert("can_edit".into(), json!(row.status != "disetujui"));
    if with_items {
        let items = sqlx::query(
            "SELECT CAST(id AS SIGNED) AS id, nama_item, spesifikasi_sebelum, spesifikasi_sesudah, \
             CAST(volume_sebelum AS CHAR) AS volume_sebelum, CAST(volume_sesudah AS CHAR) AS volume_sesudah, \
             CAST(harga_sebelum AS CHAR) AS harga_sebelum, CAST(harga_sesudah AS CHAR) AS harga_sesudah, \
             CAST(subtotal_sebelum AS CHAR) AS subtotal_sebelum, CAST(subtotal_sesudah AS CHAR) AS subtotal_sesudah \
             FROM tbl_kontrak_addendum_items WHERE addendum_id = ? ORDER BY id",
        )
        .bind(row.id)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
        let mut list = Vec::with_capacity(items.len());
        for r in &items {
            list.push(json!({
                "id": r.try_get::<i64, _>("id").map_err(internal)?,
                "nama_item": r.try_get::<Option<String>, _>("nama_item").map_err(internal)?,
                "spesifikasi_sebelum": r.try_get::<Option<String>, _>("spesifikasi_sebelum").map_err(internal)?,
                "spesifikasi_sesudah": r.try_get::<Option<String>, _>("spesifikasi_sesudah").map_err(internal)?,
                "volume_sebelum": r.try_get::<Option<String>, _>("volume_sebelum").map_err(internal)?,
                "volume_sesudah": r.try_get::<Option<String>, _>("volume_sesudah").map_err(internal)?,
                "harga_sebelum": r.try_get::<Option<String>, _>("harga_sebelum").map_err(internal)?,
                "harga_sesudah": r.try_get::<Option<String>, _>("harga_sesudah").map_err(internal)?,
                "subtotal_sebelum": r.try_get::<Option<String>, _>("subtotal_sebelum").map_err(internal)?,
                "subtotal_sesudah": r.try_get::<Option<String>, _>("subtotal_sesudah").map_err(internal)?,
            }));
        }
        out.insert("items".into(), Value::Array(list));
    }
    out.insert("attachments".into(), Value::Array(attachments(state, row.id).await?));
    out.insert("created_at".into(), iso8601_utc(row.created_at));
    out.insert("updated_at".into(), iso8601_utc(row.updated_at));
    Ok(Value::Object(out))
}

/// Lampiran addendum (`getMedia('kontrak/addendum')`) dengan custom property `type`, `label`, `nomor`, `tanggal`.
async fn attachments(state: &AppState, addendum_id: i64) -> Result<Vec<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(id AS UNSIGNED) AS id, file_name, mime_type, CAST(size AS UNSIGNED) AS size, \
         CAST(custom_properties AS CHAR) AS custom_properties FROM media \
         WHERE model_type = ? AND model_id = ? AND collection_name = ? ORDER BY order_column, id",
    )
    .bind(MODEL_ADDENDUM)
    .bind(addendum_id as u64)
    .bind(COLLECTION_ADDENDUM)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let id: u64 = r.try_get("id").map_err(internal)?;
        let file_name: String = r.try_get("file_name").map_err(internal)?;
        let props: Value = r
            .try_get::<Option<String>, _>("custom_properties")
            .map_err(internal)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        out.push(json!({
            "id": id,
            "name": file_name,
            "url": format!("{}/storage/{id}/{file_name}", state.app_url.trim_end_matches('/')),
            "type": r.try_get::<Option<String>, _>("mime_type").map_err(internal)?,
            "document_type": props.get("type").cloned().unwrap_or(Value::Null),
            "label": props.get("label").cloned().unwrap_or(Value::Null),
            "nomor": props.get("nomor").cloned().unwrap_or(Value::Null),
            "tanggal": props.get("tanggal").cloned().unwrap_or(Value::Null),
            "size": r.try_get::<u64, _>("size").map_err(internal)?,
        }));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Resource kontrak
// ---------------------------------------------------------------------------

/// Bentuk resource kontrak.
///
/// `detail = false`: `KontrakResource` (daftar). `detail = true`: `KontrakDetailResource`.
/// `show_addendums`: relasi `latestApprovedAddendum` dan `addendums` dimuat (hanya pada `show`).
async fn resource(
    state: &AppState,
    actor: u64,
    row: &KontrakRow,
    detail: bool,
    show_addendums: bool,
) -> Result<Value, ApiError> {
    let pool = &state.pool;
    let ids = pekerjaan_ids(pool, row.id).await?;
    let pekerjaans = pekerjaan_resources(state, actor, &ids).await?;
    let mut pekerjaan_names = Vec::new();
    for id in &ids {
        if let Some(p) = pekerjaan::find(pool, *id as u64).await.map_err(internal)? {
            pekerjaan_names.push(p.nama_paket.unwrap_or_default());
        }
    }
    let mut complete = true;
    for id in &ids {
        if !pekerjaan_detail::checklist_complete(pool, *id as u64).await? {
            complete = false;
            break;
        }
    }

    let mut out = Map::new();
    out.insert("id".into(), json!(row.id));
    out.insert("kode_rup".into(), json!(row.kode_rup));
    out.insert("kode_paket".into(), json!(row.kode_paket));
    if detail {
        out.insert(
            "spse_nama_paket".into(),
            json!(spse_nama_paket(pool, row, &pekerjaan_names).await?),
        );
    }
    out.insert("nomor_penawaran".into(), json!(row.nomor_penawaran));
    out.insert("tanggal_penawaran".into(), json!(fmt_date(row.tanggal_penawaran)));
    out.insert("nilai_kontrak".into(), row.nilai_kontrak.map_or(Value::Null, number_like_php));
    out.insert("tgl_sppbj".into(), json!(fmt_date(row.tgl_sppbj)));
    out.insert("tgl_spk".into(), json!(fmt_date(row.tgl_spk)));
    out.insert("tgl_spmk".into(), json!(fmt_date(row.tgl_spmk)));
    out.insert("tgl_selesai".into(), json!(fmt_date(row.tgl_selesai)));
    out.insert("sppbj".into(), json!(row.sppbj));
    out.insert("spk".into(), json!(row.spk));
    out.insert("spmk".into(), json!(row.spmk));
    out.insert("spse_sppbj_id".into(), json!(row.spse_sppbj_id));
    out.insert("spse_spk_id".into(), json!(row.spse_spk_id));
    out.insert("spse_rekanan_id".into(), json!(row.spse_rekanan_id));
    out.insert("spse_pushed_at".into(), iso8601_utc(row.spse_pushed_at));
    out.insert("id_kegiatan".into(), json!(row.id_kegiatan));
    out.insert("pekerjaan_ids".into(), json!(ids));
    out.insert("id_penyedia".into(), json!(row.id_penyedia));

    let latest = latest_approved_row(pool, row.id).await?;
    if detail {
        // `nilaiKontrakBerjalan` dan `tglSelesaiBerjalan`: addendum disetujui terbaru, lalu nilai utama.
        let berjalan = latest.as_ref().and_then(|a| a.nilai_kontrak_sesudah).or(row.nilai_kontrak);
        out.insert("nilai_kontrak_berjalan".into(), berjalan.map_or(Value::Null, number_like_php));
        let selesai = latest
            .as_ref()
            .and_then(|a| a.tgl_selesai_sesudah)
            .or(row.tgl_selesai);
        out.insert("tgl_selesai_berjalan".into(), json!(fmt_date(selesai)));
    }

    // Relasi yang dimuat pada daftar, store, dan update: kegiatan, pekerjaans, penyedia.
    out.insert("kegiatan".into(), kegiatan_json(pool, row.id_kegiatan).await?);
    out.insert("pekerjaans".into(), Value::Array(pekerjaans));
    out.insert("penyedia".into(), penyedia_json(state, row.id_penyedia).await?);
    out.insert("is_checklist_complete".into(), json!(complete));

    if detail && show_addendums {
        let latest_json = match &latest {
            Some(a) => addendum_json(state, a, false).await?,
            None => Value::Null,
        };
        out.insert("latest_approved_addendum".into(), latest_json);

        let mut addendums = Vec::new();
        let sql = format!("{SELECT_ADDENDUM} WHERE kontrak_id = ? ORDER BY addendum_ke, id");
        for r in sqlx::query(&sql).bind(row.id).fetch_all(pool).await.map_err(internal)? {
            let a = map_addendum(&r).map_err(internal)?;
            addendums.push(addendum_json(state, &a, true).await?);
        }
        out.insert("addendums".into(), Value::Array(addendums.clone()));

        // `contract_versions`: kontrak utama, lalu setiap addendum.
        let mut versions = vec![json!({
            "type": "utama",
            "label": "Kontrak Utama",
            "nomor": row.spk.clone().filter(|s| !s.is_empty()).or(row.kode_paket.clone()),
            "tanggal": fmt_date(row.tgl_spk),
            "nilai_kontrak": row.nilai_kontrak.map_or(Value::Null, number_like_php),
            "tgl_selesai": fmt_date(row.tgl_selesai),
            "status": "utama",
        })];
        let sql = format!("{SELECT_ADDENDUM} WHERE kontrak_id = ? ORDER BY addendum_ke, id");
        for r in sqlx::query(&sql).bind(row.id).fetch_all(pool).await.map_err(internal)? {
            let a = map_addendum(&r).map_err(internal)?;
            versions.push(json!({
                "type": "addendum",
                "id": a.id,
                "label": format!("Addendum ke-{}", a.addendum_ke),
                "addendum_ke": a.addendum_ke,
                "nomor": a.nomor_addendum,
                "tanggal": fmt_date(a.tanggal_addendum),
                "nilai_kontrak": a.nilai_kontrak_sesudah.map_or(Value::Null, number_like_php),
                "tgl_selesai": fmt_date(a.tgl_selesai_sesudah),
                "status": a.status,
            }));
        }
        out.insert("contract_versions".into(), Value::Array(versions));
    }

    out.insert("created_at".into(), iso8601_utc(row.created_at));
    out.insert("updated_at".into(), iso8601_utc(row.updated_at));
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------------------
// Validasi input
// ---------------------------------------------------------------------------

/// `Some(None)` = null, `Some(Some(v))` = nilai, `None` = tidak dikirim.
type Field<T> = Option<Option<T>>;

#[derive(Debug, Default)]
struct Input {
    id_kegiatan: Field<i64>,
    id_pekerjaan: Field<i64>,
    id_penyedia: Field<i64>,
    pekerjaan_ids: Option<Vec<i64>>,
    kode_rup: Field<String>,
    kode_paket: Field<String>,
    nomor_penawaran: Field<String>,
    tanggal_penawaran: Field<NaiveDate>,
    nilai_kontrak: Field<f64>,
    tgl_sppbj: Field<NaiveDate>,
    tgl_spk: Field<NaiveDate>,
    tgl_spmk: Field<NaiveDate>,
    tgl_selesai: Field<NaiveDate>,
    sppbj: Field<String>,
    spk: Field<String>,
    spmk: Field<String>,
}

fn int_of(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Aturan `date` untuk `Y-m-d` (format lain di luar ini ditolak).
fn date_of(v: &Value) -> Option<NaiveDate> {
    v.as_str().and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
}

fn parse_input(body: &Value, store: bool) -> Result<Input, ApiError> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut input = Input::default();

    let int_field = |key: &str, errs: &mut BTreeMap<String, Vec<String>>| -> Field<i64> {
        match obj.get(key) {
            None => None,
            Some(Value::Null) => Some(None),
            Some(v) => match int_of(v) {
                Some(n) => Some(Some(n)),
                None => {
                    foto::add(errs, key, format!("The {} field must be an integer.", key.replace('_', " ")));
                    None
                }
            },
        }
    };
    let str_field = |key: &str, max: usize, errs: &mut BTreeMap<String, Vec<String>>| -> Field<String> {
        match obj.get(key) {
            None => None,
            Some(Value::Null) => Some(None),
            Some(Value::String(s)) if s.chars().count() <= max => Some(Some(s.clone())),
            Some(Value::String(_)) => {
                foto::add(errs, key, format!("The {} field must not be greater than {max} characters.", key.replace('_', " ")));
                None
            }
            Some(_) => {
                foto::add(errs, key, format!("The {} field must be a string.", key.replace('_', " ")));
                None
            }
        }
    };
    let date_field = |key: &str, errs: &mut BTreeMap<String, Vec<String>>| -> Field<NaiveDate> {
        match obj.get(key) {
            None => None,
            Some(Value::Null) => Some(None),
            Some(v) => match date_of(v) {
                Some(d) => Some(Some(d)),
                None => {
                    foto::add(errs, key, format!("The {} field must be a valid date.", key.replace('_', " ")));
                    None
                }
            },
        }
    };

    if store {
        match obj.get("id_penyedia") {
            None | Some(Value::Null) => {
                foto::add(&mut errs, "id_penyedia", "The id penyedia field is required.".into());
            }
            Some(_) => input.id_penyedia = int_field("id_penyedia", &mut errs),
        }
    } else {
        input.id_penyedia = int_field("id_penyedia", &mut errs);
    }
    input.id_kegiatan = int_field("id_kegiatan", &mut errs);
    input.id_pekerjaan = int_field("id_pekerjaan", &mut errs);
    match obj.get("pekerjaan_ids") {
        None => {}
        Some(Value::Null) => input.pekerjaan_ids = None,
        Some(Value::Array(items)) => {
            let mut ids = Vec::new();
            for (i, v) in items.iter().enumerate() {
                match int_of(v) {
                    Some(n) => ids.push(n),
                    None => foto::add(&mut errs, &format!("pekerjaan_ids.{i}"), format!("The pekerjaan_ids.{i} field must be an integer.")),
                }
            }
            input.pekerjaan_ids = Some(ids);
        }
        Some(_) => foto::add(&mut errs, "pekerjaan_ids", "The pekerjaan ids field must be an array.".into()),
    }
    input.kode_rup = str_field("kode_rup", 50, &mut errs);
    input.kode_paket = str_field("kode_paket", 50, &mut errs);
    input.nomor_penawaran = str_field("nomor_penawaran", 50, &mut errs);
    input.tanggal_penawaran = date_field("tanggal_penawaran", &mut errs);
    input.nilai_kontrak = match obj.get("nilai_kontrak") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(v) => match v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())) {
            Some(n) if n >= 0.0 && n.is_finite() => Some(Some(n)),
            Some(_) => {
                foto::add(&mut errs, "nilai_kontrak", "The nilai kontrak field must be at least 0.".into());
                None
            }
            None => {
                foto::add(&mut errs, "nilai_kontrak", "The nilai kontrak field must be a number.".into());
                None
            }
        },
    };
    input.tgl_sppbj = date_field("tgl_sppbj", &mut errs);
    input.tgl_spk = date_field("tgl_spk", &mut errs);
    input.tgl_spmk = date_field("tgl_spmk", &mut errs);
    input.tgl_selesai = date_field("tgl_selesai", &mut errs);
    input.sppbj = str_field("sppbj", 50, &mut errs);
    input.spk = str_field("spk", 50, &mut errs);
    input.spmk = str_field("spmk", 50, &mut errs);

    if errs.is_empty() {
        Ok(input)
    } else {
        Err(ApiError::validation("The given data was invalid.", errs))
    }
}

/// Pemeriksaan `exists` untuk id yang dikirim (kegiatan, penyedia, pekerjaan).
async fn check_exists(pool: &MySqlPool, table: &str, id: i64, key: &str) -> Result<(), ApiError> {
    let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE id = ?"))
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    if n > 0 {
        return Ok(());
    }
    let mut errs = BTreeMap::new();
    foto::add(&mut errs, key, format!("The selected {} is invalid.", key.replace('_', " ")));
    Err(ApiError::validation("The given data was invalid.", errs))
}

async fn validate_exists(state: &AppState, input: &Input) -> Result<(), ApiError> {
    let pool = &state.pool;
    if let Some(Some(v)) = input.id_kegiatan {
        check_exists(pool, "tbl_kegiatan", v, "id_kegiatan").await?;
    }
    if let Some(Some(v)) = input.id_penyedia {
        check_exists(pool, "tbl_penyedia", v, "id_penyedia").await?;
    }
    if let Some(Some(v)) = input.id_pekerjaan {
        check_exists(pool, "tbl_pekerjaan", v, "id_pekerjaan").await?;
    }
    for id in input.pekerjaan_ids.iter().flatten() {
        check_exists(pool, "tbl_pekerjaan", *id, "pekerjaan_ids").await?;
    }
    Ok(())
}

/// `sync()` pada pivot `kontrak_pekerjaan`: hapus yang tidak ada, tambahkan yang baru.
async fn sync_pekerjaan(tx: &mut sqlx::Transaction<'_, MySql>, kontrak_id: i64, ids: &[i64]) -> Result<(), ApiError> {
    let current: Vec<i64> = sqlx::query_scalar("SELECT CAST(pekerjaan_id AS SIGNED) FROM kontrak_pekerjaan WHERE kontrak_id = ?")
        .bind(kontrak_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(internal)?;
    for id in current.iter().filter(|c| !ids.contains(c)) {
        sqlx::query("DELETE FROM kontrak_pekerjaan WHERE kontrak_id = ? AND pekerjaan_id = ?")
            .bind(kontrak_id)
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(internal)?;
    }
    let mut seen = std::collections::BTreeSet::new();
    for id in ids.iter().filter(|id| seen.insert(**id)) {
        if current.contains(id) {
            continue;
        }
        sqlx::query("INSERT INTO kontrak_pekerjaan (kontrak_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
            .bind(kontrak_id)
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(internal)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Filter daftar kontrak: `tahun`, `search` (kode RUP, nomor penawaran, kode paket, nama paket, nama penyedia).
fn list_clauses(query: &HashMap<String, String>) -> (String, Vec<String>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if query.get("tahun").is_some_and(|v| !v.is_empty() && v != "0") {
        clauses.push("k.id_kegiatan IN (SELECT kg.id FROM tbl_kegiatan kg WHERE kg.tahun_anggaran = ?)".into());
        binds.push(query["tahun"].clone());
    }
    if let Some(term) = query.get("search").filter(|v| !v.is_empty()) {
        let like = format!("%{term}%");
        clauses.push(
            "(k.kode_rup LIKE ? OR k.nomor_penawaran LIKE ? OR k.kode_paket LIKE ? \
             OR k.id IN (SELECT kp.kontrak_id FROM kontrak_pekerjaan kp JOIN tbl_pekerjaan p ON p.id = kp.pekerjaan_id WHERE p.nama_paket LIKE ?) \
             OR k.id_penyedia IN (SELECT py.id FROM tbl_penyedia py WHERE py.nama LIKE ?))"
                .into(),
        );
        binds.extend([like.clone(), like.clone(), like.clone(), like.clone(), like]);
    }
    let sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    (sql, binds)
}

async fn paged_list(
    state: &AppState,
    actor: u64,
    where_sql: &str,
    binds: &[String],
    order: &str,
    base: &str,
    page: u64,
) -> Result<Response, ApiError> {
    let per_page = 20u64;
    let count_sql = format!("SELECT COUNT(*) FROM tbl_kontrak k{where_sql}");
    let total: i64 = {
        let mut q = sqlx::query_scalar::<_, i64>(&count_sql);
        for b in binds {
            q = q.bind(b);
        }
        q.fetch_one(&state.pool).await.map_err(internal)?
    };
    let sql = format!("SELECT CAST(k.id AS SIGNED) AS id FROM tbl_kontrak k{where_sql} {order} LIMIT ? OFFSET ?");
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    for b in binds {
        q = q.bind(b);
    }
    let ids: Vec<i64> = q
        .bind(per_page)
        .bind((page - 1) * per_page)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let mut data = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(row) = find_row(&state.pool, id).await.map_err(internal)? {
            data.push(resource(state, actor, &row, false, false).await?);
        }
    }
    Ok(Json(pagination::paginate_with_query(
        data,
        total as u64,
        pagination::PageParams { page, per_page },
        base,
        "",
    ))
    .into_response())
}

fn page_of(query: &HashMap<String, String>) -> u64 {
    query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1)
}

/// `GET /api/kontrak`: paginasi 20, diurutkan `tgl_spk` lalu `created_at` menurun.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let (where_sql, binds) = list_clauses(&query);
    let base = format!("{}/api/kontrak", state.app_url.trim_end_matches('/'));
    paged_list(
        &state,
        user.user_id,
        &where_sql,
        &binds,
        "ORDER BY k.tgl_spk DESC, k.created_at DESC, k.id DESC",
        &base,
        page_of(&query),
    )
    .await
}

/// `GET /api/kontrak/pekerjaan/{id}`, `/kegiatan/{id}`, dan `/penyedia/{id}`: paginasi 20.
/// Laravel tidak memberi urutan pada rute ini; di sini diurutkan `id` naik.
async fn by_relation(
    state: &AppState,
    headers: &HeaderMap,
    query: &HashMap<String, String>,
    where_sql: String,
    param: String,
    base: String,
) -> Result<Response, ApiError> {
    let user = require_auth(state, headers).await?;
    let (extra_sql, extra_binds) = list_clauses(query);
    let mut where_all = where_sql;
    let mut binds = vec![param];
    if !extra_sql.is_empty() {
        where_all = format!("{where_all} AND{}", &extra_sql[" WHERE".len()..]);
        binds.extend(extra_binds);
    }
    paged_list(state, user.user_id, &format!(" WHERE {where_all}"), &binds, "ORDER BY k.id", &base, page_of(query)).await
}

/// `GET /api/kontrak/pekerjaan/{pekerjaan_id}`.
pub async fn by_pekerjaan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let base = format!("{}/api/kontrak/pekerjaan/{id}", state.app_url.trim_end_matches('/'));
    by_relation(
        &state,
        &headers,
        &query,
        "k.id IN (SELECT kp.kontrak_id FROM kontrak_pekerjaan kp WHERE kp.pekerjaan_id = ?)".into(),
        id,
        base,
    )
    .await
}

/// `GET /api/kontrak/kegiatan/{kegiatan_id}`.
pub async fn by_kegiatan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let base = format!("{}/api/kontrak/kegiatan/{id}", state.app_url.trim_end_matches('/'));
    by_relation(&state, &headers, &query, "k.id_kegiatan = ?".into(), id, base).await
}

/// `GET /api/kontrak/penyedia/{penyedia_id}`.
pub async fn by_penyedia(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let base = format!("{}/api/kontrak/penyedia/{id}", state.app_url.trim_end_matches('/'));
    by_relation(&state, &headers, &query, "k.id_penyedia = ?".into(), id, base).await
}

/// `POST /api/kontrak`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input = parse_input(&body, true)?;
    validate_exists(&state, &input).await?;

    let mut ids = input.pekerjaan_ids.clone().unwrap_or_default();
    if ids.is_empty() {
        if let Some(Some(p)) = input.id_pekerjaan {
            ids = vec![p];
        }
    }
    if ids.is_empty() {
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "message": "Minimal satu pekerjaan harus dipilih" })),
        )
            .into_response());
    }
    let id_pekerjaan_value = match input.id_pekerjaan.flatten() {
        Some(p) => Some(p),
        None => ids.first().copied(),
    };
    // Pemilik paket diperiksa seperti pekerjaan lain: kontrak baru hanya untuk paket yang bisa diakses.
    let roles = auth::login::roles_of(&state.pool, user.user_id).await.map_err(internal)?;
    for p in &ids {
        foto::ensure_access(&state, user.user_id, &roles, Some(*p)).await?;
    }

    let url = format!("{}/api/kontrak", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO tbl_kontrak (id_kegiatan, id_pekerjaan, id_penyedia, kode_rup, kode_paket, nomor_penawaran, tanggal_penawaran, \
         nilai_kontrak, tgl_sppbj, tgl_spk, tgl_spmk, tgl_selesai, sppbj, spk, spmk, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(input.id_kegiatan.flatten())
    .bind(id_pekerjaan_value)
    .bind(input.id_penyedia.flatten())
    .bind(input.kode_rup.clone().flatten())
    .bind(input.kode_paket.clone().flatten())
    .bind(input.nomor_penawaran.clone().flatten())
    .bind(input.tanggal_penawaran.flatten())
    .bind(input.nilai_kontrak.flatten())
    .bind(input.tgl_sppbj.flatten())
    .bind(input.tgl_spk.flatten())
    .bind(input.tgl_spmk.flatten())
    .bind(input.tgl_selesai.flatten())
    .bind(input.sppbj.clone().flatten())
    .bind(input.spk.clone().flatten())
    .bind(input.spmk.clone().flatten())
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let id = res.last_insert_id() as i64;
    sync_pekerjaan(&mut tx, id, &ids).await?;
    let row = find_row(&mut *tx, id).await.map_err(internal)?.ok_or_else(|| internal("kontrak baru tidak terbaca"))?;
    changes::log(
        &mut tx,
        &headers,
        user.user_id,
        &changes::KONTRAK,
        "created",
        id,
        None,
        Some(attributes(&row)),
        row.id_pekerjaan,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    let row = find_row(&state.pool, id).await.map_err(internal)?.ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": resource(&state, user.user_id, &row, true, false).await? })).into_response())
}

/// `GET /api/kontrak/{id}`: detail lengkap dengan addendum.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_row(&state.pool, id).await.map_err(internal)?.ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": resource(&state, user.user_id, &row, true, true).await? })).into_response())
}

/// `PUT` dan `PATCH /api/kontrak/{id}`. Field yang tidak dikirim tidak diubah.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = find_row(&state.pool, id).await.map_err(internal)?.ok_or_else(ApiError::not_found)?;
    let input = parse_input(&body, false)?;
    validate_exists(&state, &input).await?;

    let mut next = current.clone();
    if let Some(v) = input.id_kegiatan {
        next.id_kegiatan = v;
    }
    if let Some(v) = input.id_pekerjaan {
        next.id_pekerjaan = v;
    }
    if let Some(v) = input.id_penyedia {
        next.id_penyedia = v;
    }
    if let Some(v) = input.kode_rup.clone() {
        next.kode_rup = v;
    }
    if let Some(v) = input.kode_paket.clone() {
        next.kode_paket = v;
    }
    if let Some(v) = input.nomor_penawaran.clone() {
        next.nomor_penawaran = v;
    }
    if let Some(v) = input.tanggal_penawaran {
        next.tanggal_penawaran = v;
    }
    if let Some(v) = input.nilai_kontrak {
        next.nilai_kontrak = v;
    }
    if let Some(v) = input.tgl_sppbj {
        next.tgl_sppbj = v;
    }
    if let Some(v) = input.tgl_spk {
        next.tgl_spk = v;
    }
    if let Some(v) = input.tgl_spmk {
        next.tgl_spmk = v;
    }
    if let Some(v) = input.tgl_selesai {
        next.tgl_selesai = v;
    }
    if let Some(v) = input.sppbj.clone() {
        next.sppbj = v;
    }
    if let Some(v) = input.spk.clone() {
        next.spk = v;
    }
    if let Some(v) = input.spmk.clone() {
        next.spmk = v;
    }

    // Akses ke paket yang dikirim (sama dengan T31): paket baru harus bisa diakses user.
    let roles = auth::login::roles_of(&state.pool, user.user_id).await.map_err(internal)?;
    let new_ids = input.pekerjaan_ids.clone();
    for p in new_ids.iter().flatten() {
        foto::ensure_access(&state, user.user_id, &roles, Some(*p)).await?;
    }

    let changed: Vec<&str> = COLUMNS
        .iter()
        .copied()
        .filter(|c| col_json(&current, c) != col_json(&next, c))
        .collect();
    let url = format!("{}/api/kontrak/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    if !changed.is_empty() {
        let mut qb = QueryBuilder::<MySql>::new("UPDATE tbl_kontrak SET ");
        for (i, col) in changed.iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            qb.push(*col).push(" = ");
            match *col {
                "id_kegiatan" => qb.push_bind(next.id_kegiatan),
                "id_pekerjaan" => qb.push_bind(next.id_pekerjaan),
                "id_penyedia" => qb.push_bind(next.id_penyedia),
                "kode_rup" => qb.push_bind(next.kode_rup.clone()),
                "kode_paket" => qb.push_bind(next.kode_paket.clone()),
                "nomor_penawaran" => qb.push_bind(next.nomor_penawaran.clone()),
                "tanggal_penawaran" => qb.push_bind(next.tanggal_penawaran),
                "nilai_kontrak" => qb.push_bind(next.nilai_kontrak),
                "tgl_sppbj" => qb.push_bind(next.tgl_sppbj),
                "tgl_spk" => qb.push_bind(next.tgl_spk),
                "tgl_spmk" => qb.push_bind(next.tgl_spmk),
                "tgl_selesai" => qb.push_bind(next.tgl_selesai),
                "sppbj" => qb.push_bind(next.sppbj.clone()),
                "spk" => qb.push_bind(next.spk.clone()),
                _ => qb.push_bind(next.spmk.clone()),
            };
        }
        qb.push(", updated_at = NOW() WHERE id = ").push_bind(id);
        qb.build().execute(&mut *tx).await.map_err(internal)?;
    }
    if let Some(ids) = &new_ids {
        if !ids.is_empty() {
            sync_pekerjaan(&mut tx, id, ids).await?;
        }
    }
    let after = find_row(&mut *tx, id).await.map_err(internal)?.ok_or_else(|| internal("kontrak hilang"))?;
    if !changed.is_empty() {
        let mut old = Map::new();
        let mut new = Map::new();
        for col in &changed {
            old.insert((*col).into(), col_json(&current, col));
            new.insert((*col).into(), col_json(&after, col));
        }
        old.insert("updated_at".into(), carbon_json(current.updated_at));
        new.insert("updated_at".into(), carbon_json(after.updated_at));
        changes::log(
            &mut tx,
            &headers,
            user.user_id,
            &changes::KONTRAK,
            "updated",
            id,
            Some(old),
            Some(new),
            after.id_pekerjaan,
            &url,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;

    Ok(Json(json!({ "data": resource(&state, user.user_id, &after, true, false).await? })).into_response())
}

/// `DELETE /api/kontrak/{id}`.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_row(&state.pool, id).await.map_err(internal)?.ok_or_else(ApiError::not_found)?;
    let url = format!("{}/api/kontrak/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_kontrak WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    changes::log(
        &mut tx,
        &headers,
        user.user_id,
        &changes::KONTRAK,
        "deleted",
        id,
        Some(attributes(&row)),
        None,
        row.id_pekerjaan,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    Ok((StatusCode::OK, Json(json!({ "message": "Kontrak deleted successfully" }))).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_tags_entities_and_spaces() {
        assert_eq!(sanitize_spse("  <b>Rehab&nbsp;Jalan</b>   &amp; Drainase "), "Rehab Jalan & Drainase");
        assert_eq!(sanitize_spse("A&#39;B &#x41;"), "A'B A");
    }

    #[test]
    fn normalized_names_ignore_punctuation_and_case() {
        assert_eq!(normalize_name("Rehab Jalan, Desa-1"), normalize_name("rehab jalan desa 1"));
        assert_ne!(normalize_name("Rehab Jalan"), normalize_name("Bangun Jalan"));
    }

    #[test]
    fn create_requires_penyedia_and_accepts_date_strings() {
        assert!(parse_input(&json!({}), true).is_err());
        let ok = parse_input(&json!({"id_penyedia": 3, "tgl_spk": "2025-03-04", "nilai_kontrak": "1000.50"}), true).unwrap();
        assert_eq!(ok.id_penyedia, Some(Some(3)));
        assert_eq!(ok.tgl_spk, Some(Some(NaiveDate::from_ymd_opt(2025, 3, 4).unwrap())));
        assert_eq!(ok.nilai_kontrak, Some(Some(1000.5)));
        assert!(parse_input(&json!({"id_penyedia": 3, "tgl_spk": "04-03-2025"}), true).is_err());
    }
}
