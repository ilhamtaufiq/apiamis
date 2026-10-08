//! Relasi tambahan untuk `PekerjaanResource`: hitungan (`withCount`), kontrak, dan `assignment_sources`.
//! Setara `PekerjaanController@index` (withCount + eager load) dan `PekerjaanResource::toArray`.

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Value};
use sqlx::{MySqlPool, Row};
use std::collections::HashSet;

use crate::{
    format::{iso8601_utc, number_like_php},
    lookup::carbon_json,
};

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

/// Baris `tbl_output` (relasi `output`), dimuat saat `summary` pada daftar terpaginasi.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputRow {
    pub id: u64,
    pub pekerjaan_id: u64,
    pub komponen: String,
    pub satuan: String,
    /// Kolom `decimal(10,2)`: Eloquent mengembalikan string dengan dua desimal, mis. `"12.00"`.
    pub volume: String,
    pub penerima_is_optional: bool,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl OutputRow {
    /// `OutputResource`: `pekerjaan` tidak dimuat, jadi kuncinya dihilangkan.
    pub fn to_resource(&self) -> Value {
        json!({
            "id": self.id,
            "pekerjaan_id": self.pekerjaan_id,
            "komponen": self.komponen,
            "satuan": self.satuan,
            "volume": self.volume,
            "penerima_is_optional": self.penerima_is_optional,
            "created_at": iso8601_utc(self.created_at),
            "updated_at": iso8601_utc(self.updated_at),
        })
    }

    fn volume_f64(&self) -> f64 {
        self.volume.parse::<f64>().unwrap_or(0.0)
    }
}

/// Output milik satu pekerjaan, urut id.
pub async fn outputs_for(
    pool: &MySqlPool,
    pekerjaan_id: u64,
) -> Result<Vec<OutputRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, pekerjaan_id, komponen, satuan, CAST(volume AS CHAR) AS volume, penerima_is_optional, \
         created_at, updated_at FROM tbl_output WHERE pekerjaan_id = ? ORDER BY id",
    )
    .bind(pekerjaan_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(OutputRow {
                id: r.try_get("id")?,
                pekerjaan_id: r.try_get("pekerjaan_id")?,
                komponen: r.try_get("komponen")?,
                satuan: r.try_get("satuan")?,
                volume: r
                    .try_get::<Option<String>, _>("volume")?
                    .unwrap_or_default(),
                penerima_is_optional: r.try_get("penerima_is_optional")?,
                created_at: r.try_get("created_at")?,
                updated_at: r.try_get("updated_at")?,
            })
        })
        .collect()
}

/// Foto satu komponen (`komponen_id` = id output): jumlah foto dan penerima berbeda yang tidak kosong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FotoGroup {
    pub komponen_id: u64,
    pub count: i64,
    pub recipients: i64,
}

/// Foto pekerjaan dikelompokkan per komponen. `COUNT(DISTINCT NULLIF(..., 0))` setara `filter()` di PHP.
pub async fn foto_groups_for(
    pool: &MySqlPool,
    pekerjaan_id: u64,
) -> Result<Vec<FotoGroup>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT komponen_id, CAST(COUNT(*) AS SIGNED) AS n, \
         CAST(COUNT(DISTINCT NULLIF(penerima_id, 0)) AS SIGNED) AS recipients \
         FROM tbl_foto WHERE pekerjaan_id = ? GROUP BY komponen_id",
    )
    .bind(pekerjaan_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(FotoGroup {
                komponen_id: r.try_get("komponen_id")?,
                count: r.try_get("n")?,
                recipients: r.try_get("recipients")?,
            })
        })
        .collect()
}

/// Hasil `resolveFotoMetrics` cabang lengkap (relasi `output` dan `foto` dimuat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FotoMetrics {
    pub status: &'static str,
    pub required: i64,
}

/// Port cabang lengkap `Pekerjaan::resolveFotoMetrics`. `total` = jumlah seluruh foto pekerjaan.
pub fn foto_full(outputs: &[OutputRow], groups: &[FotoGroup], total: i64) -> FotoMetrics {
    if outputs.is_empty() {
        return FotoMetrics {
            status: if total > 0 {
                "belum_selesai"
            } else {
                "belum_ada_foto"
            },
            required: 0,
        };
    }
    let mut required = 0;
    let mut complete = true;
    for o in outputs {
        // max(1, (int) ceil(volume)) bila penerima wajib; 1 bila opsional.
        let units = if o.penerima_is_optional {
            1
        } else {
            (o.volume_f64().ceil() as i64).max(1)
        };
        let need = units * 5;
        required += need;
        let (photos, recipients) = groups
            .iter()
            .find(|g| g.komponen_id == o.id)
            .map_or((0, 0), |g| (g.count, g.recipients));
        if photos < need {
            complete = false;
        }
        if !o.penerima_is_optional && recipients < units {
            complete = false;
        }
    }
    let status = if total <= 0 {
        "belum_ada_foto"
    } else if !complete {
        "belum_selesai"
    } else {
        "selesai"
    };
    FotoMetrics { status, required }
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
/// `penyedia` dan `addendums` tidak dimuat pada `per_page=-1` (Laravel: `null` dan `[]`).
/// `registers` hanya dimuat saat `summary` aktif; di luar itu bentuk Laravel adalah `[]`.
pub async fn kontrak_items(
    pool: &MySqlPool,
    pekerjaan_id: u64,
    detail: bool,
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
        let penyedia = match penyedia_id.filter(|_| detail) {
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
        let add_rows = if detail { add_rows } else { Vec::new() };
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
            "registers": if summary { registers_for(pool, kid).await? } else { json!([]) },
            "penyedia": penyedia.unwrap_or(Value::Null),
            "addendums": addendums,
        }));
    }
    Ok(out)
}

/// `registers` dengan `type` (`kontrak.registers.type`), urut id seperti relasi tanpa `orderBy`.
async fn registers_for(pool: &MySqlPool, kontrak_id: u64) -> Result<Value, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT r.id, r.type_id, r.nomor, r.tanggal, CAST(r.nilai AS DOUBLE) AS nilai, \
         t.id AS type_row_id, t.code AS type_code, t.name AS type_name \
         FROM tbl_document_registers r LEFT JOIN tbl_document_types t ON t.id = r.type_id \
         WHERE r.kontrak_id = ? ORDER BY r.id",
    )
    .bind(kontrak_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let tanggal: Option<NaiveDate> = r.try_get("tanggal")?;
        let type_id: Option<u64> = r.try_get("type_row_id")?;
        let nilai: Option<f64> = r.try_get("nilai")?;
        out.push(json!({
            "id": r.try_get::<u64, _>("id")?,
            "type_id": r.try_get::<u64, _>("type_id")?,
            "nomor": r.try_get::<String, _>("nomor")?,
            // Carbon (cast date): JSON memakai format timestamp tengah malam UTC.
            "tanggal": carbon_json(tanggal.map(|d| d.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc())),
            "nilai": nilai.map_or(Value::Null, number_like_php),
            "type": match type_id {
                Some(tid) => json!({
                    "id": tid,
                    "code": r.try_get::<String, _>("type_code")?,
                    "name": r.try_get::<String, _>("type_name")?,
                }),
                None => Value::Null,
            },
        }));
    }
    Ok(Value::Array(out))
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

#[cfg(test)]
mod foto_tests {
    use super::*;

    fn output(id: u64, volume: &str, optional: bool) -> OutputRow {
        OutputRow {
            id,
            pekerjaan_id: 1,
            komponen: "k".into(),
            satuan: "unit".into(),
            volume: volume.into(),
            penerima_is_optional: optional,
            created_at: None,
            updated_at: None,
        }
    }

    fn group(komponen_id: u64, count: i64, recipients: i64) -> FotoGroup {
        FotoGroup {
            komponen_id,
            count,
            recipients,
        }
    }

    #[test]
    fn no_outputs_follows_foto_total() {
        assert_eq!(
            foto_full(&[], &[], 0),
            FotoMetrics {
                status: "belum_ada_foto",
                required: 0
            }
        );
        assert_eq!(
            foto_full(&[], &[], 3),
            FotoMetrics {
                status: "belum_selesai",
                required: 0
            }
        );
    }

    #[test]
    fn required_uses_ceiling_of_volume_times_five() {
        let outs = [output(1, "2.10", false), output(2, "0.00", false)];
        let r = foto_full(&outs, &[], 0);
        // ceil(2.10)=3 -> 15 ; max(1, 0) -> 5
        assert_eq!(r.required, 20);
        assert_eq!(r.status, "belum_ada_foto");
    }

    #[test]
    fn optional_recipient_needs_one_unit_and_no_recipient() {
        let outs = [output(1, "9.00", true)];
        // 1 unit = 5 foto; penerima tidak wajib
        let done = foto_full(&outs, &[group(1, 5, 0)], 5);
        assert_eq!(
            done,
            FotoMetrics {
                status: "selesai",
                required: 5
            }
        );
        let short = foto_full(&outs, &[group(1, 4, 0)], 4);
        assert_eq!(short.status, "belum_selesai");
    }

    #[test]
    fn mandatory_recipient_must_be_distinct_and_non_zero() {
        let outs = [output(1, "2.00", false)];
        // 10 foto cukup, tetapi penerima berbeda hanya 1 (dan 0 diabaikan)
        let one = foto_full(&outs, &[group(1, 10, 1)], 10);
        assert_eq!(one.status, "belum_selesai");
        let two = foto_full(&outs, &[group(1, 10, 2)], 10);
        assert_eq!(
            two,
            FotoMetrics {
                status: "selesai",
                required: 10
            }
        );
    }

    #[test]
    fn every_output_must_be_complete() {
        let outs = [output(1, "1.00", true), output(2, "1.00", true)];
        let r = foto_full(&outs, &[group(1, 5, 0)], 5);
        assert_eq!(r.status, "belum_selesai");
        assert_eq!(r.required, 10);
    }

    #[test]
    fn output_resource_keeps_decimal_string_and_drops_pekerjaan() {
        let v = output(7, "12.50", true).to_resource();
        assert_eq!(v["volume"], "12.50");
        assert_eq!(v["penerima_is_optional"], true);
        assert!(v.get("pekerjaan").is_none());
    }
}
