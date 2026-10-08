//! `GET /api/dashboard/analytics` (`AnalyticsController::stats`).
//!
//! Semantik yang ditiru:
//! - Basis data adalah SEMUA paket (`Pekerjaan::query()`), termasuk yang dibatalkan. Filter `tahun`
//!   dan `kecamatan_ids` memakai nilai mentah dari query (tanpa `(int)`), seperti Laravel.
//! - `trend`: kumulatif mingguan M1..Mn. `n` = `week_count` terbesar di `tbl_progress.content`,
//!   atau 12 bila tidak ada. Rata-rata dihitung per baris progres, bukan per paket.
//! - `regions`: semua kecamatan (tanpa filter), `value` 0 dan `hasProgress` false bila belum ada data.
//! - `categories`: jumlah paket per `sumber_dana`. Nama NULL tetap NULL (tidak diganti `N/A`).
//!
//! Perbedaan yang diketahui: Laravel tidak memakai `ORDER BY` pada `categories` dan `regions`
//! (regions mengikuti urutan `Kecamatan::get()`); di sini diurutkan per id dan per nama.

use axum::{
    extract::{RawQuery, State},
    http::HeaderMap,
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::Row;

use crate::{
    dashboard::{rows, Bind, Scope},
    desa::internal,
    php, require_auth, AppState,
};

/// Baris progres beserta `kecamatan_id` pekerjaan induknya.
struct ProgressRow {
    kecamatan_id: Option<i64>,
    content: Value,
}

/// `GET /api/dashboard/analytics`.
pub async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let p = php::Params::parse(raw.as_deref());

    // `$tahun` dan `$kecamatanIds` seperti Laravel: string mentah, bukan `(int)`.
    let tahun = p
        .get("tahun")
        .filter(|t| php::truthy(Some(t)))
        .map(|t| Bind::Text(t.to_string()));
    let kecamatan_values = crate::dashboard::kecamatan_raw(&p);
    let kecamatan = if kecamatan_values.is_empty() {
        None
    } else {
        Some(kecamatan_values.into_iter().map(Bind::Text).collect())
    };
    let scope = Scope {
        tahun,
        kecamatan,
        tag: None,
    };
    let data = build(&state.pool, &scope).await.map_err(internal)?;
    Ok(Json(json!({ "success": true, "data": data })))
}

async fn build(pool: &sqlx::MySqlPool, scope: &Scope) -> Result<Value, sqlx::Error> {
    let cond = scope.pekerjaan(false);
    let clause = cond.clause();

    // Semua baris progres milik paket yang lolos filter.
    let progress_rows = rows(
        pool,
        &format!(
            "SELECT CAST(tbl_pekerjaan.kecamatan_id AS SIGNED) AS kid, CAST(tbl_progress.content AS CHAR) AS content \
             FROM tbl_progress INNER JOIN tbl_pekerjaan ON tbl_pekerjaan.id = tbl_progress.pekerjaan_id \
             WHERE {clause} ORDER BY tbl_progress.id"
        ),
        &cond.binds,
    )
    .await?;
    let mut progress: Vec<ProgressRow> = Vec::with_capacity(progress_rows.len());
    for r in &progress_rows {
        let kecamatan_id: Option<i64> = r.try_get("kid")?;
        let content = r
            .try_get::<Option<String>, _>("content")?
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(Value::Null);
        progress.push(ProgressRow {
            kecamatan_id,
            content,
        });
    }

    let trend = trend(&progress);

    // Wilayah: semua kecamatan.
    let kecamatan_rows = rows(
        pool,
        "SELECT CAST(id AS SIGNED) AS id, n_kec FROM tbl_kecamatan ORDER BY id",
        &[],
    )
    .await?;
    let mut regions = Vec::with_capacity(kecamatan_rows.len());
    for r in &kecamatan_rows {
        let id: i64 = r.try_get("id")?;
        let name: String = r.try_get("n_kec")?;
        let mut total = 0.0f64;
        let mut count = 0i64;
        for p in progress.iter().filter(|p| p.kecamatan_id == Some(id)) {
            total += project_progress(&p.content);
            count += 1;
        }
        let value = if count > 0 {
            json!(php::round(total / count as f64, 2))
        } else {
            json!(0)
        };
        regions.push(json!({
            "name": name,
            "value": value,
            "hasProgress": count > 0,
        }));
    }

    let categories = rows(
        pool,
        &format!(
            "SELECT tbl_kegiatan.sumber_dana AS name, COUNT(*) AS value FROM tbl_pekerjaan \
             INNER JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
             WHERE {clause} GROUP BY tbl_kegiatan.sumber_dana ORDER BY tbl_kegiatan.sumber_dana"
        ),
        &cond.binds,
    )
    .await?
    .iter()
    .map(|r| {
        Ok(json!({
            "name": r.try_get::<Option<String>, _>("name")?.map_or(Value::Null, Value::String),
            "value": r.try_get::<i64, _>("value")?,
        }))
    })
    .collect::<Result<Vec<Value>, sqlx::Error>>()?;

    Ok(json!({
        "trend": trend,
        "regions": regions,
        "categories": categories,
    }))
}

/// `$content['week_count']` terbesar, minimal 0. Bila 0, dipakai 12 (fallback).
fn max_weeks(progress: &[ProgressRow]) -> i64 {
    let mut max = 0i64;
    for p in progress {
        if let Some(wc) = php::get(&p.content, "week_count") {
            max = max.max(php::intval(wc));
        }
    }
    if max == 0 {
        12
    } else {
        max
    }
}

/// Tren kumulatif: untuk setiap minggu `w`, rata-rata berbobot seluruh baris progres.
fn trend(progress: &[ProgressRow]) -> Vec<Value> {
    let weeks = max_weeks(progress);
    let mut out = Vec::new();
    for w in 1..=weeks {
        let mut sum_renc = 0.0f64;
        let mut sum_real = 0.0f64;
        let mut count = 0i64;
        for p in progress {
            let (renc, real) = project_cumulative(&p.content, w);
            sum_renc += renc;
            sum_real += real;
            count += 1;
        }
        out.push(json!({
            "week": format!("M{w}"),
            "rencana": if count > 0 { json!(php::round(sum_renc / count as f64, 2)) } else { json!(0) },
            "realisasi": if count > 0 { json!(php::round(sum_real / count as f64, 2)) } else { json!(0) },
        }));
    }
    out
}

/// Item `content.items` sebagai daftar nilai (foreach PHP pada array atau objek).
fn items_of(content: &Value) -> Vec<&Value> {
    match php::get(content, "items") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(Value::Object(o)) => o.values().collect(),
        _ => Vec::new(),
    }
}

/// `(bobot, target_volume)` dari sebuah item. Item bukan array menghasilkan 0 dan 0.
fn bobot_and_target(item: &Value) -> (f64, f64) {
    let bobot = php::get(item, "bobot").map_or(0.0, crate::progress_metrics::php_float);
    let target = php::get(item, "target_volume").map_or(0.0, crate::progress_metrics::php_float);
    (bobot, target)
}

/// Rencana dan realisasi kumulatif sampai minggu `w` untuk satu baris progres (dalam persen bobot).
fn project_cumulative(content: &Value, w: i64) -> (f64, f64) {
    let mut renc_total = 0.0f64;
    let mut real_total = 0.0f64;
    for item in items_of(content) {
        let (bobot, target) = bobot_and_target(item);
        if target <= 0.0 {
            continue;
        }
        let weekly = php::get(item, "weekly_data");
        let mut item_renc = 0.0f64;
        let mut item_real = 0.0f64;
        for iw in 1..=w {
            if let Some(data) = weekly.and_then(|wd| php::get_int(wd, iw)) {
                item_renc += php::get(data, "rencana").map_or(0.0, crate::progress_metrics::php_float);
                item_real += php::get(data, "realisasi").map_or(0.0, crate::progress_metrics::php_float);
            }
        }
        renc_total += (item_renc / target) * bobot;
        real_total += (item_real / target) * bobot;
    }
    (renc_total, real_total)
}

/// Realisasi total satu baris progres untuk region (semua minggu, dalam persen bobot).
fn project_progress(content: &Value) -> f64 {
    let mut total = 0.0f64;
    for item in items_of(content) {
        let (bobot, target) = bobot_and_target(item);
        if target <= 0.0 {
            continue;
        }
        let mut total_real = 0.0f64;
        if let Some(weekly) = php::get(item, "weekly_data") {
            let values: Vec<&Value> = match weekly {
                Value::Array(a) => a.iter().collect(),
                Value::Object(o) => o.values().collect(),
                _ => Vec::new(),
            };
            for wd in values {
                total_real += php::get(wd, "realisasi").map_or(0.0, crate::progress_metrics::php_float);
            }
        }
        total += (total_real / target) * bobot;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(content: Value) -> ProgressRow {
        ProgressRow {
            kecamatan_id: Some(1),
            content,
        }
    }

    #[test]
    fn week_count_defaults_to_twelve() {
        assert_eq!(max_weeks(&[row(Value::Null)]), 12);
        assert_eq!(max_weeks(&[row(json!({"week_count": 4}))]), 4);
        assert_eq!(max_weeks(&[row(json!({"week_count": "6"}))]), 6);
    }

    #[test]
    fn cumulative_uses_weeks_up_to_w_and_bobot_over_target() {
        let content = json!({
            "items": [{
                "bobot": 50,
                "target_volume": 10,
                "weekly_data": {"1": {"rencana": 2, "realisasi": 1}, "2": {"rencana": 3, "realisasi": 4}}
            }]
        });
        let (r1, a1) = project_cumulative(&content, 1);
        assert!((r1 - 10.0).abs() < 1e-9, "2/10*50");
        assert!((a1 - 5.0).abs() < 1e-9, "1/10*50");
        let (r2, a2) = project_cumulative(&content, 2);
        assert!((r2 - 25.0).abs() < 1e-9);
        assert!((a2 - 25.0).abs() < 1e-9);
    }

    #[test]
    fn items_without_target_are_skipped() {
        let content = json!({"items": [{"bobot": 10, "target_volume": 0, "weekly_data": {"1": {"realisasi": 5}}}]});
        assert_eq!(project_cumulative(&content, 1), (0.0, 0.0));
        assert_eq!(project_progress(&content), 0.0);
    }

    #[test]
    fn region_progress_sums_all_weeks() {
        let content = json!({
            "items": [{
                "bobot": 20,
                "target_volume": 4,
                "weekly_data": [{"realisasi": 1}, {"realisasi": 3}]
            }]
        });
        assert!((project_progress(&content) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn trend_has_one_row_per_week_with_zero_when_no_progress() {
        let t = trend(&[]);
        assert_eq!(t.len(), 12);
        assert_eq!(t[0], json!({"week": "M1", "rencana": 0, "realisasi": 0}));
    }
}
