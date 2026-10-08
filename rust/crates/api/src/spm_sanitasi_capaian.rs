//! Capaian SPM sanitasi (`SpmSanitasiCapaianService` dan `SpmSanitasiController::capaian`, `mapStats`).
//!
//! Cakupan dihitung dari `jumlah_pemanfaat_kk` (x5 untuk jiwa) terhadap penduduk dan target KK desa
//! wilayah resmi. Pembulatan memakai `round(x, 2)` (`(x * 100).round() / 100`), bisa beda satu digit
//! pada nilai tepat setengah dibanding `round()` PHP.
//!
//! Urutan desa pada `capaian` memakai sort stabil dengan urutan `d.id` sebagai pembanding tetap.

use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    require_auth,
    spm_sanitasi::{
        bind, fetch_row, input, int_or, int_or_null, internal, php_cmp_str, php_int, truthy, Arg,
        Scope, JENIS, REAL_DESA,
    },
    validation::Errors,
    AppState,
};

pub const JIWA_PER_KK: i64 = 5;

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// `min(100, part / whole * 100)` dibulatkan dua digit. Bila `whole` 0, PHP memberi int `0`.
fn pct(part: i64, whole: i64) -> Value {
    if whole > 0 {
        json!(round2((part as f64 / whole as f64 * 100.0).min(100.0)))
    } else {
        json!(0)
    }
}

/// `desa.n` di bawah `Desa::realWilayah` sudah dipastikan lolos; bagian ini hanya pembantu bentuk JSON.
fn by_jenis_json(kk: i64, count: i64) -> Value {
    json!({
        "unit_count": count,
        "pemanfaat_kk": kk,
        "pemanfaat_jiwa": kk * JIWA_PER_KK,
    })
}

/// Ringkasan kapasitas (`SpmSanitasiCapaianService::summary`).
pub async fn summary(pool: &MySqlPool, scope: &Scope) -> Result<Map<String, Value>, ApiError> {
    let mut dargs: Vec<Arg> = Vec::new();
    let mut dsql = format!(
        "SELECT CAST(COUNT(*) AS SIGNED), CAST(COALESCE(SUM(d.jumlah_penduduk), 0) AS SIGNED), \
         CAST(COALESCE(SUM(d.target), 0) AS SIGNED) FROM tbl_desa d WHERE {REAL_DESA}"
    );
    if let Some(k) = scope.kecamatan {
        dsql.push_str(" AND d.kecamatan_id = ?");
        dargs.push(Arg::I(k));
    }
    let r = fetch_row(pool, &dsql, &dargs).await?;
    let total_desa: i64 = r.try_get(0).map_err(internal)?;
    let total_penduduk: i64 = r.try_get(1).map_err(internal)?;
    let target_kk: i64 = r.try_get(2).map_err(internal)?;

    // Infrastruktur dengan desa wilayah resmi, sesuai `infrastrukturBaseQuery`.
    let (w, wargs) = crate::spm_sanitasi::spm_filter(scope, true, true);
    let kk_sql = format!(
        "SELECT CAST(COALESCE(SUM(s.jumlah_pemanfaat_kk), 0) AS SIGNED) FROM tbl_spm_sanitasi s WHERE {w}"
    );
    let total_kk: i64 = fetch_row(pool, &kk_sql, &wargs)
        .await?
        .try_get(0)
        .map_err(internal)?;

    let jenis_sql = format!(
        "SELECT s.jenis, CAST(COALESCE(SUM(s.jumlah_pemanfaat_kk), 0) AS SIGNED), CAST(COUNT(*) AS SIGNED) \
         FROM tbl_spm_sanitasi s WHERE {w} GROUP BY s.jenis"
    );
    let mut by: HashMap<String, (i64, i64)> = HashMap::new();
    for row in bind(sqlx::query(&jenis_sql), &wargs)
        .fetch_all(pool)
        .await
        .map_err(internal)?
    {
        let jenis: String = row.try_get(0).map_err(internal)?;
        let kk: i64 = row.try_get(1).map_err(internal)?;
        let count: i64 = row.try_get(2).map_err(internal)?;
        by.insert(jenis, (kk, count));
    }

    let with_sql = format!("SELECT CAST(COUNT(DISTINCT s.desa_id) AS SIGNED) FROM tbl_spm_sanitasi s WHERE {w}");
    let desa_with: i64 = fetch_row(pool, &with_sql, &wargs)
        .await?
        .try_get(0)
        .map_err(internal)?;

    let total_jiwa = total_kk * JIWA_PER_KK;
    let mut by_jenis = Map::new();
    for j in JENIS {
        let (kk, count) = by.get(*j).copied().unwrap_or((0, 0));
        by_jenis.insert((*j).to_string(), by_jenis_json(kk, count));
    }

    let mut out = Map::new();
    out.insert("jiwa_per_kk".into(), json!(JIWA_PER_KK));
    out.insert("total_desa".into(), json!(total_desa));
    out.insert("desa_with_infrastruktur".into(), json!(desa_with));
    out.insert("desa_without_infrastruktur".into(), json!((total_desa - desa_with).max(0)));
    out.insert("total_penduduk".into(), json!(total_penduduk));
    out.insert("target_kk".into(), json!(target_kk));
    out.insert("total_pemanfaat_kk".into(), json!(total_kk));
    out.insert("total_pemanfaat_jiwa".into(), json!(total_jiwa));
    out.insert("gap_kk".into(), json!((target_kk - total_kk).max(0)));
    out.insert("gap_jiwa".into(), json!((total_penduduk - total_jiwa).max(0)));
    out.insert("coverage_percentage".into(), pct(total_jiwa, total_penduduk));
    out.insert("coverage_kk_percentage".into(), pct(total_kk, target_kk));
    out.insert("by_jenis".into(), Value::Object(by_jenis));
    Ok(out)
}

/// Agregat per desa untuk tahun terpilih: jenis → (pemanfaat KK, jumlah unit).
type DesaAgg = HashMap<i64, HashMap<String, (i64, i64)>>;

async fn desa_aggregates(pool: &MySqlPool, tahun: Option<&str>) -> Result<DesaAgg, ApiError> {
    let mut sql = String::from(
        "SELECT CAST(s.desa_id AS SIGNED), s.jenis, CAST(COALESCE(SUM(s.jumlah_pemanfaat_kk), 0) AS SIGNED), \
         CAST(COUNT(*) AS SIGNED) FROM tbl_spm_sanitasi s WHERE s.desa_id IS NOT NULL",
    );
    let mut args: Vec<Arg> = Vec::new();
    if let Some(t) = tahun {
        sql.push_str(" AND s.tahun_konstruksi = ?");
        args.push(Arg::I(php_int(t)));
    }
    sql.push_str(" GROUP BY s.desa_id, s.jenis");
    let mut agg: DesaAgg = HashMap::new();
    for row in bind(sqlx::query(&sql), &args)
        .fetch_all(pool)
        .await
        .map_err(internal)?
    {
        let desa: i64 = row.try_get(0).map_err(internal)?;
        let jenis: String = row.try_get(1).map_err(internal)?;
        let kk: i64 = row.try_get(2).map_err(internal)?;
        let count: i64 = row.try_get(3).map_err(internal)?;
        agg.entry(desa).or_default().insert(jenis, (kk, count));
    }
    Ok(agg)
}

/// Total (KK, unit) untuk satu desa. `jenis` `None` berarti semua jenis; pencocokan tanpa memperhatikan huruf.
fn sum_for(agg: Option<&HashMap<String, (i64, i64)>>, jenis: Option<&str>) -> (i64, i64) {
    let Some(map) = agg else {
        return (0, 0);
    };
    map.iter()
        .filter(|(j, _)| jenis.map_or(true, |f| j.eq_ignore_ascii_case(f)))
        .fold((0, 0), |acc, (_, (kk, c))| (acc.0 + kk, acc.1 + c))
}

/// Baris desa wilayah resmi (`Desa::realWilayah`) dengan nama kecamatannya.
struct DesaRow {
    id: i64,
    n_desa: String,
    jumlah_penduduk: i64,
    target: i64,
    kecamatan_id: Option<i64>,
    n_kec: Option<String>,
}

async fn desa_rows(
    pool: &MySqlPool,
    kecamatan: Option<i64>,
    search: Option<&str>,
    order: &str,
) -> Result<Vec<DesaRow>, ApiError> {
    let mut sql = format!(
        "SELECT CAST(d.id AS SIGNED), d.n_desa, CAST(COALESCE(d.jumlah_penduduk, 0) AS SIGNED), \
         CAST(COALESCE(d.target, 0) AS SIGNED), CAST(d.kecamatan_id AS SIGNED), kc.n_kec \
         FROM tbl_desa d LEFT JOIN tbl_kecamatan kc ON kc.id = d.kecamatan_id WHERE {REAL_DESA}"
    );
    let mut args: Vec<Arg> = Vec::new();
    if let Some(k) = kecamatan {
        sql.push_str(" AND d.kecamatan_id = ?");
        args.push(Arg::I(k));
    }
    if let Some(s) = search {
        sql.push_str(
            " AND (d.n_desa LIKE ? OR EXISTS (SELECT 1 FROM tbl_kecamatan k \
             WHERE k.id = d.kecamatan_id AND k.n_kec LIKE ?))",
        );
        args.push(Arg::S(format!("%{s}%")));
        args.push(Arg::S(format!("%{s}%")));
    }
    sql.push_str(&format!(" ORDER BY {order}"));
    let rows = bind(sqlx::query(&sql), &args)
        .fetch_all(pool)
        .await
        .map_err(internal)?;
    rows.iter()
        .map(|r| {
            Ok(DesaRow {
                id: r.try_get(0)?,
                n_desa: r.try_get(1)?,
                jumlah_penduduk: r.try_get(2)?,
                target: r.try_get(3)?,
                kecamatan_id: r.try_get(4)?,
                n_kec: r.try_get(5)?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(internal)
}

/// Satu desa dalam daftar capaian (`mapDesaCapaian`).
struct Capaian {
    json: Value,
    coverage: f64,
    jumlah_penduduk: i64,
    pemanfaat_kk: i64,
    n_desa: String,
}

fn map_capaian(d: &DesaRow, agg: Option<&HashMap<String, (i64, i64)>>, jenis: Option<&str>) -> Capaian {
    let (kk, unit) = sum_for(agg, jenis);
    let jiwa = kk * JIWA_PER_KK;
    let cov_v = pct(jiwa, d.jumlah_penduduk);
    let coverage = cov_v.as_f64().unwrap_or(0.0);
    let per = |j: &str| sum_for(agg, Some(j)).0;
    let json = json!({
        "desa": {
            "id": d.id,
            "n_desa": d.n_desa,
            "jumlah_penduduk": d.jumlah_penduduk,
            "kecamatan": { "id": d.kecamatan_id, "n_kec": d.n_kec },
        },
        "unit_count": unit,
        "target_kk": d.target,
        "pemanfaat_kk": kk,
        "pemanfaat_jiwa": jiwa,
        "gap_kk": (d.target - kk).max(0),
        "gap_jiwa": (d.jumlah_penduduk - jiwa).max(0),
        "coverage_percentage": cov_v,
        "coverage_kk_percentage": pct(kk, d.target),
        "by_jenis": {
            "spaldt_kk": per("spaldt"),
            "spalds_kk": per("spalds"),
            "iplt_kk": per("iplt"),
            "mck_individu_kk": per("mck_individu"),
            "mck_komunal_kk": per("mck_komunal"),
        },
    });
    Capaian {
        json,
        coverage,
        jumlah_penduduk: d.jumlah_penduduk,
        pemanfaat_kk: kk,
        n_desa: d.n_desa.clone(),
    }
}

/// `GET /api/spm-sanitasi/capaian`: ringkasan dan daftar desa terurut, dengan paginator.
pub async fn capaian(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;

    let mut e = Errors::default();
    if let Some(j) = input(&q, "jenis") {
        if !JENIS.contains(&j.as_str()) {
            e.add("jenis", "The selected jenis is invalid.");
        }
    }
    let sort_in = input(&q, "sort");
    if let Some(s) = &sort_in {
        if !["coverage_percentage", "jumlah_penduduk", "pemanfaat_kk", "n_desa"].contains(&s.as_str()) {
            e.add("sort", "The selected sort is invalid.");
        }
    }
    let dir_in = input(&q, "direction");
    if let Some(d) = &dir_in {
        if d != "asc" && d != "desc" {
            e.add("direction", "The selected direction is invalid.");
        }
    }
    e.finish()?;

    let pool = &state.pool;
    let scope = Scope {
        kecamatan: int_or_null(&q, "kecamatan_id"),
        jenis: truthy(input(&q, "jenis")),
        tahun: truthy(input(&q, "tahun")),
    };
    let summary = summary(pool, &scope).await?;

    let search = input(&q, "search");
    let desa = desa_rows(pool, scope.kecamatan, search.as_deref(), "d.id").await?;
    let agg = desa_aggregates(pool, scope.tahun.as_deref()).await?;
    let mut items: Vec<Capaian> = desa
        .iter()
        .map(|d| map_capaian(d, agg.get(&d.id), scope.jenis.as_deref()))
        .collect();

    let sort = sort_in.unwrap_or_else(|| "coverage_percentage".into());
    let desc = dir_in.as_deref() == Some("desc");
    items.sort_by(|a, b| {
        let ord = match sort.as_str() {
            "jumlah_penduduk" => a.jumlah_penduduk.cmp(&b.jumlah_penduduk),
            "pemanfaat_kk" => a.pemanfaat_kk.cmp(&b.pemanfaat_kk),
            "n_desa" => php_cmp_str(&a.n_desa, &b.n_desa),
            _ => a.coverage.partial_cmp(&b.coverage).unwrap_or(std::cmp::Ordering::Equal),
        };
        if desc {
            ord.reverse()
        } else {
            ord
        }
    });

    let per_page = int_or(&q, "per_page", 15).max(1) as usize;
    let page = int_or(&q, "page", 1).max(1) as usize;
    let total = items.len();
    let data: Vec<Value> = items
        .into_iter()
        .skip((page - 1) * per_page)
        .take(per_page)
        .map(|c| c.json)
        .collect();
    let last_page = total.div_ceil(per_page).max(1);

    Ok(Json(json!({
        "success": true,
        "summary": summary,
        "data": data,
        "meta": {
            "current_page": page,
            "last_page": last_page,
            "per_page": per_page,
            "total": total,
        },
    }))
    .into_response())
}

/// Peta per desa (`mapStats`): urut nama desa, dengan jenis dan tahun opsional.
pub async fn map_stats(
    pool: &MySqlPool,
    jenis: Option<&str>,
    tahun: Option<&str>,
) -> Result<Vec<Value>, ApiError> {
    let desa = desa_rows(pool, None, None, "d.n_desa, d.id").await?;
    let agg = desa_aggregates(pool, tahun).await?;
    Ok(desa
        .iter()
        .map(|d| {
            let (kk, unit) = sum_for(agg.get(&d.id), jenis);
            json!({
                "desa_id": d.id,
                "desa": d.n_desa,
                "kecamatan": d.n_kec,
                "jumlah_penduduk": d.jumlah_penduduk,
                "target_kk": d.target,
                "unit_count": unit,
                "pemanfaat_kk": kk,
                "pemanfaat_jiwa": kk * JIWA_PER_KK,
            })
        })
        .collect())
}
