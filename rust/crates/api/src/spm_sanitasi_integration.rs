//! Tampilan integrasi SPM sanitasi per desa (`SpmSanitasiPekerjaanIntegrationService::paginateIntegration`,
//! `buildDesaIntegrationRow`, `summarizeRows`): `GET /api/spm-sanitasi/integration` dan
//! `GET /api/spm-sanitasi/integration/desa/{desaId}`.
//!
//! Aturan pemetaan jenis dan tipe output, daftar paket, dan sinkron ada di `spm_sanitasi_pekerjaan`.
//!
//! Perbedaan kecil:
//! - `per_page` di bawah 1 dibatasi ke 1. Laravel membagi dengan nol dan menghasilkan 500.
//! - Relasi `desa.kecamatan` yang hilang menghasilkan `null` di `id` dan `n_kec`. Laravel melempar
//!   error pada `$desa->kecamatan->id`.
//! - Urutan desa mengikuti `ORDER BY n_desa, id`. Laravel hanya `ORDER BY n_desa`.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlConnection, MySqlPool, Row};

use crate::{
    require_auth,
    spm_sanitasi::{self, desa_with_kecamatan, internal, Arg},
    spm_sanitasi_capaian::JIWA_PER_KK,
    spm_sanitasi_pekerjaan::{self as pkj, format_pekerjaan, load_pekerjaan, pekerjaan_ids, PkjFilter},
    validation::Errors,
    AppState,
};

const SYNC_STATUSES: &[&str] = &["matched", "partial", "no_infrastruktur", "no_pekerjaan"];

/// Pengguna yang melihat data (`byUserRole()`).
pub struct Viewer<'a> {
    pub user_id: u64,
    pub roles: &'a [(u64, String)],
}

/// Filter `paginateIntegration`.
#[derive(Default)]
pub struct IntegrationFilter {
    pub tahun: Option<String>,
    pub kecamatan_id: Option<i64>,
    pub desa_id: Option<i64>,
    pub search: Option<String>,
    pub sync_status: Option<String>,
    pub output_type: Option<String>,
    pub per_page: i64,
    pub page: i64,
}

/// `resolveSyncStatus`.
fn sync_status(infra: usize, pekerjaan: usize, linked: usize) -> &'static str {
    if infra == 0 && pekerjaan == 0 {
        return "no_data";
    }
    if infra == 0 {
        return "no_infrastruktur";
    }
    if pekerjaan == 0 {
        return "no_pekerjaan";
    }
    if linked > 0 {
        return "matched";
    }
    "partial"
}

/// Satu baris infrastruktur SPM untuk desa.
struct InfraRow {
    id: i64,
    jenis: String,
    nama: String,
    kk: i64,
    pembiayaan: f64,
    linked_count: i64,
}

/// `buildDesaIntegrationRow(desa, tahun, outputType)`. `desa` berisi relasi `kecamatan` seperti `toArray()`.
async fn desa_row(
    c: &mut MySqlConnection,
    viewer: &Viewer<'_>,
    desa: &Value,
    tahun: Option<&str>,
    output_type: Option<&str>,
) -> Result<Value, ApiError> {
    let desa_id = desa["id"].as_i64().unwrap_or_default();

    // Infrastruktur desa, dibatasi jenis bila output_type memetakan ke jenis SPM.
    let jenis = pkj::spm_jenis_list_for_output_type(output_type);
    let mut sql = String::from(
        "SELECT CAST(s.id AS SIGNED) AS id, s.jenis, s.nama_infrastruktur, \
         CAST(COALESCE(s.jumlah_pemanfaat_kk, 0) AS SIGNED) AS kk, \
         CAST(COALESCE(s.pembiayaan_total, 0) AS DOUBLE) AS pembiayaan, \
         (SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_spm_sanitasi_pekerjaan sp WHERE sp.spm_sanitasi_id = s.id) AS linked \
         FROM tbl_spm_sanitasi s WHERE s.desa_id = ?",
    );
    let mut args = vec![Arg::I(desa_id)];
    if !jenis.is_empty() {
        sql.push_str(&format!(" AND s.jenis IN ({})", vec!["?"; jenis.len()].join(",")));
        args.extend(jenis.iter().map(|j| Arg::S((*j).to_string())));
    }
    sql.push_str(" ORDER BY s.id");
    let mut infra: Vec<InfraRow> = Vec::new();
    for r in spm_sanitasi::bind(sqlx::query(&sql), &args)
        .fetch_all(&mut *c)
        .await
        .map_err(internal)?
    {
        infra.push(InfraRow {
            id: r.try_get("id").map_err(internal)?,
            jenis: r.try_get::<Option<String>, _>("jenis").map_err(internal)?.unwrap_or_default(),
            nama: r
                .try_get::<Option<String>, _>("nama_infrastruktur")
                .map_err(internal)?
                .unwrap_or_default(),
            kk: r.try_get("kk").map_err(internal)?,
            pembiayaan: r.try_get("pembiayaan").map_err(internal)?,
            linked_count: r.try_get("linked").map_err(internal)?,
        });
    }

    // sanitasiPekerjaanQuery(tahun, null, desa, null, outputType) dengan byUserRole.
    let filter = PkjFilter {
        tahun: tahun.map(str::to_string),
        desa_id: Some(desa_id),
        output_type: output_type.map(str::to_string),
        ..Default::default()
    };
    let ids = pekerjaan_ids(c, &filter, viewer.user_id, viewer.roles, false).await?;
    let pkjs = load_pekerjaan(c, &ids).await?;

    // aggregateDerived
    let mut unit = 0i64;
    let mut kk = 0i64;
    let mut jiwa = 0i64;
    let mut nilai = 0.0f64;
    let mut progress: Vec<f64> = Vec::new();
    for p in &pkjs {
        let d = pkj::derived(p);
        unit += d.unit;
        kk += d.kk;
        jiwa += d.jiwa;
        nilai += d.nilai_kontrak;
        progress.push(d.progress_total);
    }
    let progress_avg = if progress.is_empty() {
        0.0
    } else {
        let avg = progress.iter().sum::<f64>() / progress.len() as f64;
        (avg * 10.0).round() / 10.0
    };

    // aggregateManualForDesa
    let manual_kk: i64 = infra.iter().map(|r| r.kk).sum();
    let manual_nilai: f64 = infra.iter().map(|r| r.pembiayaan).sum();

    let linked_count = pkjs.iter().filter(|p| !p.spm_links.is_empty()).count();
    let formatted: Vec<Value> = pkjs.iter().map(|p| format_pekerjaan(p, None)).collect();

    let mut output_types: Vec<String> = Vec::new();
    for item in &formatted {
        if let Some(arr) = item["output_types"].as_array() {
            for t in arr.iter().filter_map(Value::as_str) {
                if !output_types.iter().any(|x| x == t) {
                    output_types.push(t.to_string());
                }
            }
        }
    }

    let infra_json: Vec<Value> = infra
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "jenis": r.jenis,
                "nama_infrastruktur": r.nama,
                "jumlah_pemanfaat_kk": r.kk,
                "linked_pekerjaan_count": r.linked_count,
            })
        })
        .collect();

    Ok(json!({
        "desa": {
            "id": desa["id"],
            "n_desa": desa["n_desa"],
            "jumlah_penduduk": desa["jumlah_penduduk"].as_i64().unwrap_or(0),
            "kecamatan": {
                "id": desa["kecamatan"]["id"],
                "n_kec": desa["kecamatan"]["n_kec"],
            },
        },
        "infrastruktur": infra_json,
        "infrastruktur_count": infra.len(),
        "pekerjaan_count": pkjs.len(),
        "linked_count": linked_count,
        "pekerjaan": formatted,
        "output_types": output_types,
        "output_type_filter": output_type,
        "derived": {
            "unit": unit,
            "mck_unit": unit,
            "kk": kk,
            "jiwa": jiwa,
            "nilai_kontrak": nilai,
            "progress_avg": progress_avg,
        },
        "manual": {
            "kk": manual_kk,
            "jiwa": manual_kk * JIWA_PER_KK,
            "nilai_kontrak": manual_nilai,
        },
        "sync_status": sync_status(infra.len(), pkjs.len(), linked_count),
    }))
}

/// `summarizeRows`.
fn summarize(rows: &[Value]) -> Value {
    let mut s = json!({
        "total_desa": rows.len(),
        "matched_count": 0,
        "partial_count": 0,
        "no_infrastruktur_count": 0,
        "no_pekerjaan_count": 0,
        "total_infrastruktur": 0,
        "total_pekerjaan": 0,
        "total_linked": 0,
    });
    let mut tot = [0i64; 3];
    let mut status_counts: HashMap<&str, i64> = HashMap::new();
    for row in rows {
        tot[0] += row["infrastruktur_count"].as_i64().unwrap_or(0);
        tot[1] += row["pekerjaan_count"].as_i64().unwrap_or(0);
        tot[2] += row["linked_count"].as_i64().unwrap_or(0);
        if let Some(st) = row["sync_status"].as_str() {
            *status_counts.entry(st).or_insert(0) += 1;
        }
    }
    let count = |k: &str| status_counts.get(k).copied().unwrap_or(0);
    s["matched_count"] = json!(count("matched"));
    s["partial_count"] = json!(count("partial"));
    s["no_infrastruktur_count"] = json!(count("no_infrastruktur"));
    s["no_pekerjaan_count"] = json!(count("no_pekerjaan"));
    s["total_infrastruktur"] = json!(tot[0]);
    s["total_pekerjaan"] = json!(tot[1]);
    s["total_linked"] = json!(tot[2]);
    s
}

/// `paginateIntegration`: semua desa difilter, dihitung, lalu dipaginasi di memori.
async fn paginate(
    pool: &MySqlPool,
    viewer: &Viewer<'_>,
    f: &IntegrationFilter,
) -> Result<(Vec<Value>, Value, Value), ApiError> {
    let mut sql = String::from("SELECT CAST(d.id AS SIGNED) FROM tbl_desa d WHERE 1 = 1");
    let mut args: Vec<Arg> = Vec::new();
    if let Some(k) = f.kecamatan_id {
        sql.push_str(" AND d.kecamatan_id = ?");
        args.push(Arg::I(k));
    }
    if let Some(d) = f.desa_id {
        sql.push_str(" AND d.id = ?");
        args.push(Arg::I(d));
    }
    if let Some(s) = &f.search {
        sql.push_str(
            " AND (d.n_desa LIKE ? OR EXISTS (SELECT 1 FROM tbl_kecamatan kc WHERE kc.id = d.kecamatan_id AND kc.n_kec LIKE ?))",
        );
        let like = format!("%{s}%");
        args.push(Arg::S(like.clone()));
        args.push(Arg::S(like));
    }
    sql.push_str(" ORDER BY d.n_desa, d.id");
    let desa_ids: Vec<i64> = spm_sanitasi::bind(sqlx::query(&sql), &args)
        .fetch_all(pool)
        .await
        .map_err(internal)?
        .iter()
        .map(|r| r.try_get::<i64, _>(0).map_err(internal))
        .collect::<Result<_, _>>()?;

    let mut rows: Vec<Value> = Vec::new();
    for id in desa_ids {
        let desa = desa_with_kecamatan(pool, id).await?;
        if desa.is_null() {
            continue;
        }
        let mut c = pool.acquire().await.map_err(internal)?;
        let row = desa_row(&mut c, viewer, &desa, f.tahun.as_deref(), f.output_type.as_deref()).await?;
        rows.push(row);
    }

    if let Some(st) = &f.sync_status {
        rows.retain(|r| r["sync_status"].as_str() == Some(st.as_str()));
    }
    rows.retain(|r| {
        r["infrastruktur_count"].as_i64().unwrap_or(0) > 0 || r["pekerjaan_count"].as_i64().unwrap_or(0) > 0
    });

    let per_page = f.per_page.max(1);
    let total = rows.len() as i64;
    let last_page = ((total + per_page - 1) / per_page).max(1);
    let page = f.page.max(1).min(last_page);
    let offset = ((page - 1) * per_page) as usize;
    let page_rows: Vec<Value> = rows
        .iter()
        .skip(offset)
        .take(per_page as usize)
        .cloned()
        .collect();

    let meta = json!({
        "current_page": page,
        "last_page": last_page,
        "per_page": per_page,
        "total": total,
    });
    Ok((page_rows, meta, summarize(&rows)))
}

/// Filter `integration` dari query string, dengan nilai seperti `$request->filled()` dan `integer()`.
fn filter_from_query(q: &HashMap<String, String>) -> IntegrationFilter {
    IntegrationFilter {
        tahun: spm_sanitasi::input(q, "tahun"),
        kecamatan_id: spm_sanitasi::int_or_null(q, "kecamatan_id"),
        desa_id: spm_sanitasi::int_or_null(q, "desa_id"),
        search: spm_sanitasi::input(q, "search"),
        sync_status: spm_sanitasi::input(q, "sync_status"),
        output_type: spm_sanitasi::input(q, "output_type"),
        per_page: spm_sanitasi::int_or(q, "per_page", 15),
        page: spm_sanitasi::int_or(q, "page", 1),
    }
}

/// `in:` untuk `output_type` (`OUTPUT_TYPES`) dan `sync_status` (`SYNC_STATUSES`).
fn validate_query(q: &HashMap<String, String>, check_sync: bool) -> Result<(), ApiError> {
    let mut e = Errors::default();
    if check_sync {
        if let Some(v) = spm_sanitasi::input(q, "sync_status") {
            if !SYNC_STATUSES.contains(&v.as_str()) {
                e.add("sync_status", "The selected sync status is invalid.");
            }
        }
    }
    if let Some(v) = spm_sanitasi::input(q, "output_type") {
        if !pkj::OUTPUT_TYPES.contains(&v.as_str()) {
            e.add("output_type", "The selected output type is invalid.");
        }
    }
    e.finish()
}

/// `GET /api/spm-sanitasi/integration`.
pub async fn integration(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    validate_query(&q, true)?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let viewer = Viewer {
        user_id: user.user_id,
        roles: &roles,
    };
    let (data, meta, summary) = paginate(&state.pool, &viewer, &filter_from_query(&q)).await?;
    Ok(Json(json!({
        "success": true,
        "data": data,
        "meta": meta,
        "summary": summary,
    }))
    .into_response())
}

/// `GET /api/spm-sanitasi/integration/desa/{desaId}`: satu baris integrasi desa. Desa tak ada berarti 404.
pub async fn integration_by_desa(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(desa_id): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    validate_query(&q, false)?;
    let desa_id: i64 = desa_id.parse().map_err(|_| ApiError::not_found())?;
    let desa = desa_with_kecamatan(&state.pool, desa_id).await?;
    if desa.is_null() {
        return Err(ApiError::not_found());
    }
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let viewer = Viewer {
        user_id: user.user_id,
        roles: &roles,
    };
    let tahun = spm_sanitasi::input(&q, "tahun");
    let output_type = spm_sanitasi::input(&q, "output_type");
    let mut c = state.pool.acquire().await.map_err(internal)?;
    let data = desa_row(&mut c, &viewer, &desa, tahun.as_deref(), output_type.as_deref()).await?;
    Ok((
        StatusCode::OK,
        Json(json!({ "success": true, "data": data })),
    )
        .into_response())
}
