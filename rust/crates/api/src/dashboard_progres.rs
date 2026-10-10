//! `GET /api/dashboard/progres-mvp`: ringkasan progres pekerjaan untuk dashboard MVP, dengan KPI
//! pengawas dan konsultan pengawas.
//!
//! Filter `tahun` dan `kecamatan` sama dengan `stats`. Cakupan data mengikuti role pemanggil
//! (`access::restriction`), jadi pengawas hanya melihat pekerjaan yang ditugaskan kepadanya.
//!
//! Progres = realisasi fisik TERBARU per pekerjaan (`tanggal` terbaru, lalu `id` terbesar), sama
//! dengan `stats`. Pekerjaan tanpa riwayat realisasi dihitung sebagai "belum ada progres" dan
//! tidak ikut rata-rata.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use auth::login as auth_login;
use axum::{
    extract::{RawQuery, State},
    http::HeaderMap,
    Json,
};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::Row;

use crate::{
    access,
    dashboard::{self, Bind, Cond},
    desa::internal,
    php, require_auth, AppState,
};

const ROLE_PENGAWAS: &str = "pengawas";
const ROLE_KONSULTAN: &str = "konsultan_pengawas";

/// Satu pekerjaan dalam cakupan.
#[derive(Debug, Clone, PartialEq)]
pub struct PekerjaanRow {
    pub id: i64,
    pub kecamatan_id: Option<i64>,
    pub pagu: f64,
    pub progres: Option<f64>,
}

/// Satu penugasan pengawas atau konsultan pengawas ke pekerjaan.
#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub pekerjaan_id: i64,
    pub user_id: i64,
    pub nama: String,
    pub role: String,
}

/// `GET /api/dashboard/progres-mvp`.
pub async fn progres_mvp(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Json<Value>, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth_login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let p = php::Params::parse(raw.as_deref());
    let scope = dashboard::stats_scope(&p);

    // Filter dasar (tahun, kecamatan, tanpa pekerjaan batal) lalu batas cakupan role.
    let mut cond = scope.pekerjaan(true);
    let access = access::restriction(user.user_id, &roles, "sp");
    if !access.sql.is_empty() {
        cond = cond.and(&Cond {
            sql: vec![format!(
                "tbl_pekerjaan.id IN (SELECT sp.id FROM tbl_pekerjaan sp WHERE 1=1{})",
                access.sql
            )],
            binds: access.binds.iter().map(|b| Bind::Int(*b as i64)).collect(),
        });
    }

    let pekerjaan = load_pekerjaan(&state.pool, &cond).await.map_err(internal)?;
    let assignments = load_assignments(&state.pool, &cond)
        .await
        .map_err(internal)?;
    let kecamatan = dashboard::kecamatan_map(&state.pool)
        .await
        .map_err(internal)?;

    Ok(Json(
        json!({ "data": summarize(&pekerjaan, &assignments, &kecamatan) }),
    ))
}

async fn load_pekerjaan(
    pool: &sqlx::MySqlPool,
    cond: &Cond,
) -> Result<Vec<PekerjaanRow>, sqlx::Error> {
    let sql = format!(
        "SELECT CAST(tbl_pekerjaan.id AS SIGNED) AS id, \
         CAST(tbl_pekerjaan.kecamatan_id AS SIGNED) AS kecamatan_id, \
         CAST(COALESCE(tbl_pekerjaan.pagu, 0) AS DOUBLE) AS pagu, \
         lp.persen AS progres \
         FROM tbl_pekerjaan \
         LEFT JOIN ( \
            SELECT x.pekerjaan_id, x.persen FROM ( \
                SELECT h.pekerjaan_id, CAST(h.persen AS DOUBLE) AS persen, \
                       ROW_NUMBER() OVER (PARTITION BY h.pekerjaan_id ORDER BY h.tanggal DESC, h.id DESC) AS rn \
                FROM pekerjaan_progress_estimasi_history h \
                WHERE h.tipe = 'realisasi' AND h.jenis = 'fisik' \
            ) x WHERE x.rn = 1 \
         ) lp ON lp.pekerjaan_id = tbl_pekerjaan.id \
         WHERE {}",
        cond.clause()
    );
    let mut q = sqlx::query(&sql);
    for b in &cond.binds {
        q = match b {
            Bind::Text(t) => q.bind(t.clone()),
            Bind::Int(i) => q.bind(*i),
        };
    }
    let rows = q.fetch_all(pool).await?;
    rows.iter()
        .map(|r| {
            Ok(PekerjaanRow {
                id: r.try_get("id")?,
                kecamatan_id: r.try_get("kecamatan_id")?,
                pagu: r.try_get("pagu")?,
                progres: r.try_get("progres")?,
            })
        })
        .collect()
}

/// Penugasan per paket dari fitur `user-pekerjaan` (tabel `user_pekerjaan`). Peran ditentukan per user:
/// `konsultan_pengawas` bila user punya role itu, selain itu `pengawas` (sama dengan aturan
/// `grantPengawasRoleIfEligible` saat penugasan dibuat).
pub(crate) async fn load_assignments(
    pool: &sqlx::MySqlPool,
    cond: &Cond,
) -> Result<Vec<Assignment>, sqlx::Error> {
    let sql = format!(
        "SELECT CAST(up.pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(u.id AS SIGNED) AS user_id, \
         u.name AS nama, \
         CASE WHEN EXISTS (SELECT 1 FROM model_has_roles m JOIN roles r ON r.id = m.role_id \
              WHERE m.model_type = 'App\\\\Models\\\\User' AND m.model_id = u.id AND r.name = '{ROLE_KONSULTAN}') \
         THEN '{ROLE_KONSULTAN}' ELSE '{ROLE_PENGAWAS}' END AS role \
         FROM user_pekerjaan up \
         JOIN users u ON u.id = up.user_id \
         WHERE up.pekerjaan_id IN (SELECT tbl_pekerjaan.id FROM tbl_pekerjaan WHERE {})",
        cond.clause()
    );
    let mut q = sqlx::query(&sql);
    for b in &cond.binds {
        q = match b {
            Bind::Text(t) => q.bind(t.clone()),
            Bind::Int(i) => q.bind(*i),
        };
    }
    let rows = q.fetch_all(pool).await?;
    rows.iter()
        .map(|r| {
            Ok(Assignment {
                pekerjaan_id: r.try_get("pekerjaan_id")?,
                user_id: r.try_get("user_id")?,
                nama: r.try_get::<Option<String>, _>("nama")?.unwrap_or_default(),
                role: r.try_get("role")?,
            })
        })
        .collect()
}

fn avg(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn round2(v: Option<f64>) -> Value {
    match v {
        Some(x) => json!((x * 100.0).round() / 100.0),
        None => Value::Null,
    }
}

/// Hitung KPI dari baris mentah. Fungsi murni supaya mudah diuji.
pub fn summarize(
    pekerjaan: &[PekerjaanRow],
    assignments: &[Assignment],
    kecamatan: &HashMap<i64, String>,
) -> Value {
    let progres_all: Vec<f64> = pekerjaan.iter().filter_map(|p| p.progres).collect();
    let total_pagu: f64 = pekerjaan.iter().map(|p| p.pagu).sum();

    // Per kecamatan: jumlah pekerjaan dan rata-rata progres.
    let mut per_kec: BTreeMap<String, (i64, Vec<f64>)> = BTreeMap::new();
    for p in pekerjaan {
        let name = p
            .kecamatan_id
            .and_then(|id| kecamatan.get(&id).cloned())
            .unwrap_or_else(|| "Tanpa kecamatan".to_string());
        let e = per_kec.entry(name).or_insert((0, Vec::new()));
        e.0 += 1;
        if let Some(v) = p.progres {
            e.1.push(v);
        }
    }
    let per_kecamatan: Vec<Value> = per_kec
        .into_iter()
        .map(|(nama, (jumlah, prog))| json!({ "nama": nama, "jumlah": jumlah, "rata_progres": round2(avg(&prog)) }))
        .collect();

    let progres_by_id: HashMap<i64, Option<f64>> =
        pekerjaan.iter().map(|p| (p.id, p.progres)).collect();
    let total = pekerjaan.len() as i64;

    let mut pengawas = serde_json::Map::new();
    for role in [ROLE_PENGAWAS, ROLE_KONSULTAN] {
        let assigned: BTreeSet<i64> = assignments
            .iter()
            .filter(|a| a.role == role)
            .map(|a| a.pekerjaan_id)
            .filter(|id| progres_by_id.contains_key(id))
            .collect();
        let prog: Vec<f64> = assigned
            .iter()
            .filter_map(|id| progres_by_id.get(id).copied().flatten())
            .collect();
        let aktif: BTreeSet<i64> = assignments
            .iter()
            .filter(|a| a.role == role)
            .map(|a| a.user_id)
            .collect();
        pengawas.insert(
            role.to_string(),
            json!({
                "aktif": aktif.len(),
                "pekerjaan_diawasi": assigned.len(),
                "belum_diawasi": total - assigned.len() as i64,
                "rata_progres": round2(avg(&prog)),
            }),
        );
    }

    // Per orang: jumlah pekerjaan dan rata-rata progres, diurutkan dari jumlah terbanyak.
    let mut per_orang: BTreeMap<(i64, String), (String, BTreeSet<i64>)> = BTreeMap::new();
    for a in assignments {
        if !progres_by_id.contains_key(&a.pekerjaan_id) {
            continue;
        }
        let e = per_orang
            .entry((a.user_id, a.role.clone()))
            .or_insert((a.nama.clone(), BTreeSet::new()));
        e.1.insert(a.pekerjaan_id);
    }
    let mut orang: Vec<Value> = per_orang
        .into_iter()
        .map(|((user_id, role), (nama, ids))| {
            let prog: Vec<f64> = ids
                .iter()
                .filter_map(|id| progres_by_id.get(id).copied().flatten())
                .collect();
            json!({
                "user_id": user_id,
                "nama": nama,
                "role": role,
                "jumlah_pekerjaan": ids.len(),
                "rata_progres": round2(avg(&prog)),
            })
        })
        .collect();
    orang.sort_by(|a, b| {
        b["jumlah_pekerjaan"]
            .as_u64()
            .cmp(&a["jumlah_pekerjaan"].as_u64())
    });

    json!({
        "kpi": {
            "total_pekerjaan": total,
            "total_pagu": total_pagu,
            "rata_progres": round2(avg(&progres_all)),
            "belum_progres": total - progres_all.len() as i64,
        },
        "pengawas": Value::Object(pengawas),
        "per_kecamatan": per_kecamatan,
        "per_pengawas": orang,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: i64, kec: Option<i64>, pagu: f64, progres: Option<f64>) -> PekerjaanRow {
        PekerjaanRow {
            id,
            kecamatan_id: kec,
            pagu,
            progres,
        }
    }

    fn a(pid: i64, uid: i64, nama: &str, role: &str) -> Assignment {
        Assignment {
            pekerjaan_id: pid,
            user_id: uid,
            nama: nama.into(),
            role: role.into(),
        }
    }

    #[test]
    fn computes_kpi_and_pengawas_coverage() {
        let pekerjaan = vec![
            p(1, Some(10), 100.0, Some(40.0)),
            p(2, Some(10), 50.0, Some(60.0)),
            p(3, Some(20), 25.0, None),
            p(4, None, 25.0, Some(80.0)),
        ];
        let assignments = vec![
            a(1, 7, "Budi", ROLE_PENGAWAS),
            a(2, 7, "Budi", ROLE_PENGAWAS),
            a(4, 8, "Sari", ROLE_KONSULTAN),
        ];
        let kec = HashMap::from([(10, "Cianjur".to_string()), (20, "Pacet".to_string())]);
        let v = summarize(&pekerjaan, &assignments, &kec);

        assert_eq!(v["kpi"]["total_pekerjaan"], 4);
        assert_eq!(v["kpi"]["total_pagu"], 200.0);
        assert_eq!(v["kpi"]["rata_progres"], 60.0); // (40+60+80)/3
        assert_eq!(v["kpi"]["belum_progres"], 1);

        assert_eq!(v["pengawas"][ROLE_PENGAWAS]["aktif"], 1);
        assert_eq!(v["pengawas"][ROLE_PENGAWAS]["pekerjaan_diawasi"], 2);
        assert_eq!(v["pengawas"][ROLE_PENGAWAS]["belum_diawasi"], 2);
        assert_eq!(v["pengawas"][ROLE_PENGAWAS]["rata_progres"], 50.0);
        assert_eq!(v["pengawas"][ROLE_KONSULTAN]["pekerjaan_diawasi"], 1);
        assert_eq!(v["pengawas"][ROLE_KONSULTAN]["rata_progres"], 80.0);

        let kec_cianjur = v["per_kecamatan"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["nama"] == "Cianjur")
            .unwrap();
        assert_eq!(kec_cianjur["jumlah"], 2);
        assert_eq!(kec_cianjur["rata_progres"], 50.0);
        assert!(v["per_kecamatan"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| k["nama"] == "Tanpa kecamatan"));

        assert_eq!(v["per_pengawas"][0]["nama"], "Budi");
        assert_eq!(v["per_pengawas"][0]["jumlah_pekerjaan"], 2);
    }

    #[test]
    fn empty_scope_returns_nulls_not_panics() {
        let v = summarize(&[], &[], &HashMap::new());
        assert_eq!(v["kpi"]["total_pekerjaan"], 0);
        assert!(v["kpi"]["rata_progres"].is_null());
        assert_eq!(v["pengawas"][ROLE_PENGAWAS]["aktif"], 0);
        assert!(v["pengawas"][ROLE_PENGAWAS]["rata_progres"].is_null());
    }

    #[test]
    fn assignment_outside_scope_is_ignored() {
        let pekerjaan = vec![p(1, None, 10.0, Some(10.0))];
        let assignments = vec![a(99, 5, "Luar", ROLE_PENGAWAS)];
        let v = summarize(&pekerjaan, &assignments, &HashMap::new());
        assert_eq!(v["pengawas"][ROLE_PENGAWAS]["pekerjaan_diawasi"], 0);
        assert_eq!(v["per_pengawas"].as_array().unwrap().len(), 0);
    }
}
