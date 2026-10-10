//! `GET /api/desa/{id}/profile`, setara `DesaController@profile` di Laravel.
//!
//! Isi respons: `desa` (DesaResource dengan kecamatan), `ringkasan`, `pekerjaan` (PekerjaanResource),
//! `spm_sanitasi`, dan `unit_spam` (model mentah, tanpa resource).
//!
//! Perbedaan dari daftar pekerjaan (`pekerjaan::to_resource`), sesuai Laravel:
//! - Relasi yang dimuat hanya `kecamatan` dan `kegiatan`. Key relasi lain (`desa`, `pengawas`,
//!   `pendamping`, `tags`, `kontrak`, `output`, `draft`) tidak ada di respons, seperti `whenLoaded`.
//! - Hitungan dan progres yang tidak dimuat di Laravel memakai nilai default: `foto_count` dan
//!   `foto_required_count` null, `foto_status` `belum_ada_foto`, `kontrak_count` 0, `progress_total` 0,
//!   `penerima_count` null, `sipd_links_count` 0.
//! - Tidak memakai scope `byUserRole()`, seperti `Pekerjaan::where('desa_id', ...)` di Laravel.
//!
//! Catatan lain:
//! - Baris diurutkan `id`. Laravel tanpa `orderBy`.
//! - `pagu` dibaca sebagai teks (`CAST AS CHAR`) seperti PDO di PHP. Kolom FLOAT dibaca sebagai f64
//!   biner menghasilkan 1234.56005859375, sedangkan PHP menerima "1234.56".
//! - Urutan key JSON mengikuti `serde_json` tanpa `preserve_order`, yaitu alfabetis, bukan urutan Laravel.

use std::collections::HashMap;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    access, desa,
    format::{iso8601_utc, number_like_php},
    pekerjaan, pekerjaan_rel,
    raw_model::{raw_model, Cast},
    require_auth, spam_units, AppState,
};

/// `SpmSanitasi::$casts`.
const SPM_CASTS: &[(&str, Cast)] = &[
    ("latitude", Cast::Float),
    ("longitude", Cast::Float),
    ("jumlah_pemanfaat_kk", Cast::Int),
    ("jumlah_pemanfaat_jiwa", Cast::Int),
    ("tahun_konstruksi", Cast::Int),
    ("pembiayaan_apbn", Cast::Float),
    ("pembiayaan_apbd", Cast::Float),
    ("pembiayaan_dak", Cast::Float),
    ("pembiayaan_hibah", Cast::Float),
    ("pembiayaan_csr", Cast::Float),
    ("pembiayaan_lain", Cast::Float),
    ("pembiayaan_total", Cast::Float),
    ("kapasitas_desain", Cast::Float),
    ("kapasitas_terpakai", Cast::Float),
    ("kapasitas_tidak_terpakai", Cast::Float),
    ("biaya_operasional", Cast::Float),
    ("truk_tinja_unit", Cast::Int),
    ("kapasitas_truk_m3", Cast::Float),
    ("jumlah_ritasi", Cast::Int),
    ("jarak_maksimal_pelayanan_km", Cast::Float),
    ("alokasi_biaya_operasional", Cast::Float),
    ("pemanfaat_dari_integrasi", Cast::Bool),
    ("pembiayaan_dari_integrasi", Cast::Bool),
];

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// `round($x, 2)` PHP: pembulatan setengah menjauhi nol.
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Model mentah `SpmSanitasi` / `UnitSpam` milik desa, urut `id`.
async fn raw_rows(
    pool: &MySqlPool,
    table: &str,
    desa_id: u64,
    casts: &[(&str, Cast)],
) -> Result<Vec<Value>, ApiError> {
    let sql = format!("SELECT * FROM {table} WHERE desa_id = ? ORDER BY id");
    let rows = sqlx::query(&sql)
        .bind(desa_id)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    rows.iter().map(|r| raw_model(r, casts)).collect()
}

/// `pagu` sebagai teks MySQL, seperti PDO di PHP (lihat catatan di atas modul).
async fn pagu_text(pool: &MySqlPool, desa_id: u64) -> Result<HashMap<u64, f64>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, CAST(pagu AS CHAR) AS pagu FROM tbl_pekerjaan WHERE desa_id = ?",
    )
    .bind(desa_id)
    .fetch_all(pool)
    .await?;
    let mut out = HashMap::new();
    for r in &rows {
        let id: u64 = r.try_get("id")?;
        let pagu: Option<String> = r.try_get("pagu")?;
        if let Some(v) = pagu.and_then(|s| s.trim().parse::<f64>().ok()) {
            out.insert(id, v);
        }
    }
    Ok(out)
}

/// Satu item `PekerjaanResource` dengan `load(['kecamatan', 'kegiatan'])`.
fn pekerjaan_item(p: &pekerjaan::PekerjaanRow, rel: &pekerjaan::Loaded) -> Value {
    json!({
        "id": p.id,
        "kode_rekening": p.kode_rekening,
        "nama_paket": p.nama_paket,
        "pagu": p.pagu.map(number_like_php),
        "is_konsultan": p.is_konsultan,
        // `$this->status ?: 'active'`: string kosong dan "0" dianggap falsy.
        "status": p.status.clone().filter(|s| !s.is_empty() && s != "0").unwrap_or_else(|| "active".to_string()),
        "catatan": p.catatan,
        // Hitungan yang tidak dimuat di profil: nilai default Laravel.
        "has_kontrak": false,
        "kontrak_count": 0,
        "progress_total": 0,
        "deviasi": 0,
        "progress_estimasi_fisik": null,
        "progress_estimasi_keuangan": null,
        "progress_estimasi_keuangan_nilai": null,
        "deviasi_estimasi_fisik": null,
        "deviasi_estimasi_keuangan": null,
        "foto_count": null,
        "foto_required_count": null,
        "foto_status": "belum_ada_foto",
        "kecamatan_id": p.kecamatan_id,
        "desa_id": p.desa_id,
        "kegiatan_id": p.kegiatan_id,
        "pengawas_id": p.pengawas_id,
        "pendamping_id": p.pendamping_id,
        "assignment_sources": rel.sources.get(&p.id).cloned().unwrap_or_default(),
        "kecamatan": p.kecamatan_id.and_then(|id| rel.kecamatan.get(&id).cloned()),
        "kegiatan": p.kegiatan_id.and_then(|id| rel.kegiatan.get(&id).cloned()),
        "penerima_count": null,
        "sipd_links_count": 0,
        "created_at": iso8601_utc(p.created_at),
        "updated_at": iso8601_utc(p.updated_at),
    })
}

/// `GET /api/desa/{id}/profile`.
pub async fn profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // auth:sanctum lebih dulu, lalu binding model (404), seperti `show`.
    let user = require_auth(&state, &headers).await?;
    let desa_id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let desa_row = desa::find(&state.pool, desa_id)
        .await
        .map_err(desa::internal)?
        .ok_or_else(ApiError::not_found)?;

    // `Pekerjaan::where('desa_id', ...)->get()`: tanpa scope byUserRole.
    let filter = pekerjaan::PekerjaanFilter {
        desa_id: Some(desa_id.to_string()),
        sort_by: "id".to_string(),
        sort_desc: false,
        ..Default::default()
    };
    let (mut pekerjaan_rows, _) = pekerjaan::list(
        &state.pool,
        &filter,
        &access::Restriction::none(),
        None,
        None,
    )
    .await
    .map_err(desa::internal)?;
    let pagu_teks = pagu_text(&state.pool, desa_id).await.map_err(desa::internal)?;
    for p in &mut pekerjaan_rows {
        if let Some(v) = pagu_teks.get(&p.id) {
            p.pagu = Some(*v);
        }
    }

    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(desa::internal)?;
    let viewer = pekerjaan_rel::viewer(&state.pool, user.user_id, &roles)
        .await
        .map_err(desa::internal)?;
    // Mode `unbounded` tanpa `summary`: hanya kecamatan, kegiatan, pengawas, dan sumber penugasan.
    let rel = pekerjaan::load(
        &state.pool,
        &pekerjaan_rows,
        pekerjaan::Mode {
            summary: false,
            unbounded: true,
        },
        &viewer,
    )
    .await
    .map_err(desa::internal)?;
    let pekerjaan_json: Vec<Value> = pekerjaan_rows
        .iter()
        .map(|p| pekerjaan_item(p, &rel))
        .collect();

    let spm = raw_rows(&state.pool, "tbl_spm_sanitasi", desa_id, SPM_CASTS).await?;
    let unit = raw_rows(&state.pool, "tbl_unit_spam", desa_id, spam_units::UNIT_CASTS).await?;
    let usulan_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tbl_usulan_kegiatan WHERE desa_id = ?")
            .bind(desa_id)
            .fetch_one(&state.pool)
            .await
            .map_err(desa::internal)?;

    // Ringkasan (`DesaController::profile`).
    let kepadatan = match desa_row.luas {
        Some(luas) if luas > 0.0 => {
            let penduduk = desa_row.jumlah_penduduk.unwrap_or(0) as f64;
            number_like_php(round2(penduduk / luas))
        }
        _ => Value::Null,
    };
    let total_pagu: f64 = pekerjaan_rows.iter().filter_map(|p| p.pagu).sum();
    // `Collection::where` di PHP: `==` pada nilai di memori, case-sensitive.
    let status_count = |status: &str| {
        pekerjaan_rows
            .iter()
            .filter(|p| p.status.as_deref() == Some(status))
            .count()
    };
    let sum_int = |rows: &[Value], key: &str| -> i64 {
        rows.iter()
            .map(|r| r[key].as_i64().unwrap_or(0))
            .sum()
    };
    let simspam = unit.iter().filter(|u| u["is_simspam"] == json!(true)).count();
    let berfungsi = spm
        .iter()
        .filter(|s| s["status_keberfungsian"].as_str() == Some("Berfungsi"))
        .count();

    Ok(Json(json!({
        "data": {
            "desa": desa::to_resource(&desa_row),
            "ringkasan": {
                "kepadatan_penduduk": kepadatan,
                "total_pekerjaan": pekerjaan_rows.len(),
                "pekerjaan_aktif": status_count("active"),
                "pekerjaan_selesai": status_count("completed"),
                "total_pagu": number_like_php(round2(total_pagu)),
                "total_unit_spam": unit.len(),
                "unit_spam_simspam": simspam,
                "total_infrastruktur_sanitasi": spm.len(),
                "infrastruktur_berfungsi": berfungsi,
                "total_pemanfaat_kk": sum_int(&spm, "jumlah_pemanfaat_kk"),
                "total_pemanfaat_jiwa": sum_int(&spm, "jumlah_pemanfaat_jiwa"),
                "total_usulan_kegiatan": usulan_count,
            },
            "pekerjaan": pekerjaan_json,
            "spm_sanitasi": spm,
            "unit_spam": unit,
        }
    })))
}
