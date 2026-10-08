//! Relasi tambahan untuk `PekerjaanResource`: hitungan (`withCount`), kontrak, dan `assignment_sources`.
//! Setara `PekerjaanController@index` (withCount + eager load) dan `PekerjaanResource::toArray`.

use chrono::NaiveDate;
use serde_json::{json, Value};
use sqlx::{MySqlPool, Row};
use std::collections::HashSet;

use crate::format::number_like_php;

/// Hitungan yang di Laravel berasal dari `withCount`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub penerima: i64,
    pub foto: i64,
    pub kontrak_pivot: i64,
    pub kontrak_legacy: i64,
    pub sipd_links: i64,
}

impl Counts {
    /// `has_kontrak`: pivot atau legacy terisi.
    pub fn has_kontrak(&self) -> bool {
        self.kontrak_pivot > 0 || self.kontrak_legacy > 0
    }

    /// `kontrak_count`: pivot diutamakan, legacy sebagai cadangan.
    pub fn kontrak_count(&self) -> i64 {
        if self.kontrak_pivot > 0 {
            self.kontrak_pivot
        } else {
            self.kontrak_legacy
        }
    }

    /// `foto_status` saat relasi `foto` dan `output` belum dimuat (mode non-summary).
    pub fn foto_status(&self) -> &'static str {
        if self.foto > 0 {
            "belum_selesai"
        } else {
            "belum_ada_foto"
        }
    }
}

async fn count(pool: &MySqlPool, sql: &str, id: u64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(sql)
        .bind(id)
        .fetch_one(pool)
        .await
}

pub async fn counts_for(pool: &MySqlPool, pekerjaan_id: u64) -> Result<Counts, sqlx::Error> {
    Ok(Counts {
        penerima: count(
            pool,
            "SELECT COUNT(*) FROM tbl_penerima WHERE pekerjaan_id = ?",
            pekerjaan_id,
        )
        .await?,
        foto: count(
            pool,
            "SELECT COUNT(*) FROM tbl_foto WHERE pekerjaan_id = ?",
            pekerjaan_id,
        )
        .await?,
        kontrak_pivot: count(
            pool,
            "SELECT COUNT(*) FROM kontrak_pekerjaan WHERE pekerjaan_id = ?",
            pekerjaan_id,
        )
        .await?,
        kontrak_legacy: count(
            pool,
            "SELECT COUNT(*) FROM tbl_kontrak WHERE id_pekerjaan = ?",
            pekerjaan_id,
        )
        .await?,
        sipd_links: count(
            pool,
            "SELECT COUNT(*) FROM tbl_sipd_pekerjaan_links WHERE pekerjaan_id = ?",
            pekerjaan_id,
        )
        .await?,
    })
}

/// Item `kontrak` di `PekerjaanResource`: kontrak pivot dengan penyedia dan addendum.
/// `registers` hanya dimuat saat `summary` aktif; di luar itu bentuk Laravel adalah `[]`.
pub async fn kontrak_items(
    pool: &MySqlPool,
    pekerjaan_id: u64,
    summary: bool,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT k.id, k.spk, k.tgl_spk, k.kode_paket, k.tgl_spmk, k.tgl_selesai, \
         CAST(k.nilai_kontrak AS DOUBLE) AS nilai_kontrak, k.id_penyedia \
         FROM tbl_kontrak k JOIN kontrak_pekerjaan kp ON kp.kontrak_id = k.id \
         WHERE kp.pekerjaan_id = ? ORDER BY k.id",
    )
    .bind(pekerjaan_id)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let kid: u64 = r.try_get("id")?;
        let penyedia_id: Option<u64> = r.try_get("id_penyedia")?;
        let penyedia = match penyedia_id {
            Some(pid) => sqlx::query("SELECT id, nama FROM tbl_penyedia WHERE id = ?")
                .bind(pid)
                .fetch_optional(pool)
                .await?
                .map(|p| -> Result<Value, sqlx::Error> {
                    Ok(json!({ "id": p.try_get::<u64, _>("id")?, "nama": p.try_get::<Option<String>, _>("nama")? }))
                })
                .transpose()?,
            None => None,
        };

        let add_rows = sqlx::query(
            "SELECT id, addendum_ke, nomor_addendum, tanggal_addendum, status FROM tbl_kontrak_addendums \
             WHERE kontrak_id = ? ORDER BY addendum_ke",
        )
        .bind(kid)
        .fetch_all(pool)
        .await?;
        let mut addendums = Vec::with_capacity(add_rows.len());
        for a in &add_rows {
            let tanggal: Option<NaiveDate> = a.try_get("tanggal_addendum")?;
            addendums.push(json!({
                "id": a.try_get::<u64, _>("id")?,
                "addendum_ke": a.try_get::<u32, _>("addendum_ke")?,
                "nomor_addendum": a.try_get::<Option<String>, _>("nomor_addendum")?,
                "tanggal_addendum": tanggal.map(|d| d.format("%Y-%m-%d").to_string()),
                "status": a.try_get::<Option<String>, _>("status")?,
            }));
        }

        let spk: Option<String> = r.try_get("spk")?;
        let tgl_spk: Option<NaiveDate> = r.try_get("tgl_spk")?;
        let tgl_spmk: Option<NaiveDate> = r.try_get("tgl_spmk")?;
        let tgl_selesai: Option<NaiveDate> = r.try_get("tgl_selesai")?;
        let nilai: Option<f64> = r.try_get("nilai_kontrak")?;
        let fmt = |d: Option<NaiveDate>| d.map(|d| d.format("%Y-%m-%d").to_string());
        out.push(json!({
            "id": kid,
            "spk": spk,
            "tgl_spk": fmt(tgl_spk),
            "kode_paket": r.try_get::<Option<String>, _>("kode_paket")?,
            "tgl_spmk": fmt(tgl_spmk),
            "tgl_selesai": fmt(tgl_selesai),
            "nilai_kontrak": nilai.map(number_like_php),
            "registers": if summary { Value::Null } else { json!([]) },
            "penyedia": penyedia.unwrap_or(Value::Null),
            "addendums": addendums,
        }));
    }
    Ok(out)
}

/// Pengguna yang melihat daftar: menentukan `assignment_sources`.
#[derive(Debug, Clone)]
pub struct Viewer {
    pub user_id: u64,
    pub is_admin: bool,
    pub nip: Option<String>,
    pub role_ids: Vec<u64>,
}

/// `assignment_sources` dari `PekerjaanResource`. Admin selalu `[]` karena Laravel tidak menghitungnya.
pub async fn assignment_sources(
    pool: &MySqlPool,
    viewer: &Viewer,
    pekerjaan_id: u64,
    kegiatan_id: Option<i64>,
    pengawas_nip: Option<&str>,
    pendamping_nip: Option<&str>,
) -> Result<Vec<&'static str>, sqlx::Error> {
    if viewer.is_admin {
        return Ok(Vec::new());
    }
    let mut sources = Vec::new();

    let manual: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_pekerjaan WHERE user_id = ? AND pekerjaan_id = ?",
    )
    .bind(viewer.user_id)
    .bind(pekerjaan_id)
    .fetch_one(pool)
    .await?;
    if manual > 0 {
        sources.push("manual");
    }

    if let Some(keg) = kegiatan_id {
        if !viewer.role_ids.is_empty() {
            let placeholders = vec!["?"; viewer.role_ids.len()].join(",");
            let sql = format!("SELECT COUNT(*) FROM kegiatan_role WHERE role_id IN ({placeholders}) AND kegiatan_id = ?");
            let mut q = sqlx::query_scalar::<_, i64>(&sql);
            for rid in &viewer.role_ids {
                q = q.bind(rid);
            }
            let n = q.bind(keg as u64).fetch_one(pool).await?;
            if n > 0 {
                sources.push("role");
            }
        }
    }

    if let Some(nip) = viewer.nip.as_deref().filter(|n| !n.is_empty() && *n != "0") {
        if pengawas_nip == Some(nip) {
            sources.push("pengawas");
        }
        if pendamping_nip == Some(nip) {
            sources.push("pendamping");
        }
    }
    Ok(sources)
}

/// Id role dari daftar `roles_of`, dipakai untuk `kegiatan_role`.
pub fn role_ids(roles: &[(u64, String)]) -> Vec<u64> {
    let set: HashSet<u64> = roles.iter().map(|(id, _)| *id).collect();
    let mut v: Vec<u64> = set.into_iter().collect();
    v.sort_unstable();
    v
}

/// Data user (nip) dan hak admin untuk `Viewer`.
pub async fn viewer(
    pool: &MySqlPool,
    user_id: u64,
    roles: &[(u64, String)],
) -> Result<Viewer, sqlx::Error> {
    let nip: Option<String> = sqlx::query_scalar("SELECT nip FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .flatten();
    Ok(Viewer {
        user_id,
        is_admin: roles.iter().any(|(_, n)| n == "admin"),
        nip,
        role_ids: role_ids(roles),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kontrak_flags_follow_pivot_then_legacy() {
        let c = Counts {
            kontrak_pivot: 0,
            kontrak_legacy: 2,
            ..Default::default()
        };
        assert!(c.has_kontrak());
        assert_eq!(c.kontrak_count(), 2);
        let none = Counts::default();
        assert!(!none.has_kontrak());
        assert_eq!(none.kontrak_count(), 0);
        let both = Counts {
            kontrak_pivot: 1,
            kontrak_legacy: 3,
            ..Default::default()
        };
        assert_eq!(both.kontrak_count(), 1, "pivot diutamakan");
    }

    #[test]
    fn foto_status_matches_non_summary_branch() {
        assert_eq!(Counts::default().foto_status(), "belum_ada_foto");
        assert_eq!(
            Counts {
                foto: 4,
                ..Default::default()
            }
            .foto_status(),
            "belum_selesai"
        );
    }
}
