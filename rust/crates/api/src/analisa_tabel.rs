//! Analisis tabel terstruktur untuk Asisten Data (khusus admin).
//!
//! Model tidak menulis SQL. Model memilih tabel, fungsi agregasi, dan kolom dari katalog di bawah,
//! lalu server menyusun query dengan identifier dari katalog dan nilai tahun terikat. Kolom yang
//! berisi data pribadi (NIK, alamat, koordinat, nama penerima) tidak ada di katalog.

use axum::http::StatusCode;
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::media::internal;

/// Batas kelompok yang dikembalikan ke model.
pub const HASIL_MAKS: usize = 20;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tipe {
    Angka,
    Teks,
}

#[derive(Debug)]
pub struct Kolom {
    pub nama: &'static str,
    pub tipe: Tipe,
}

const fn teks(nama: &'static str) -> Kolom {
    Kolom { nama, tipe: Tipe::Teks }
}

const fn angka(nama: &'static str) -> Kolom {
    Kolom { nama, tipe: Tipe::Angka }
}

#[derive(Debug)]
pub struct Tabel {
    /// Nama yang dipakai model.
    pub kode: &'static str,
    /// Nama tabel di database.
    pub sql: &'static str,
    pub label: &'static str,
    /// Kolom tahun anggaran, bila ada. Diterapkan otomatis kecuali `semua: true`.
    pub kolom_tahun: Option<&'static str>,
    pub kolom: &'static [Kolom],
}

pub static KATALOG: &[Tabel] = &[
    Tabel {
        kode: "spm_sanitasi",
        sql: "tbl_spm_sanitasi",
        label: "Sarana sanitasi: MCK, IPAL, SPALD-S, SPALD-T",
        kolom_tahun: None,
        kolom: &[
            teks("jenis"),
            teks("status_keberfungsian"),
            teks("kualitas_keberfungsian"),
            teks("jenis_pengelola"),
            angka("jumlah_pemanfaat_kk"),
            angka("jumlah_pemanfaat_jiwa"),
            angka("pembiayaan_total"),
            angka("biaya_operasional"),
        ],
    },
    Tabel {
        kode: "unit_spam",
        sql: "tbl_unit_spam",
        label: "Unit SPAM perdesaan",
        kolom_tahun: None,
        kolom: &[teks("sistem_layanan"), teks("program"), teks("sumber_dana"), teks("tahun_pembangunan")],
    },
    Tabel {
        kode: "budget_spam",
        sql: "tbl_spam_budgets",
        label: "Paket budget SPAM",
        kolom_tahun: Some("tahun"),
        kolom: &[teks("tahun"), teks("sumber_dana"), angka("nilai_kontrak")],
    },
    Tabel {
        kode: "capaian_spam",
        sql: "tbl_spam_achievements",
        label: "Capaian SPAM per tahun",
        kolom_tahun: Some("tahun"),
        kolom: &[
            teks("tahun"),
            teks("sumber"),
            angka("jumlah_sr"),
            angka("jumlah_kk"),
            angka("jumlah_jiwa"),
            angka("jumlah_bjp_kk"),
            angka("jumlah_bjp_jiwa"),
        ],
    },
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Agregasi {
    Count,
    Sum,
    Avg,
}

/// Permintaan analisis yang sudah dicocokkan dengan katalog.
#[derive(Debug)]
pub struct Rencana {
    pub tabel: &'static Tabel,
    pub agregasi: Agregasi,
    pub kolom: Option<&'static Kolom>,
    pub group_by: Option<&'static Kolom>,
    pub semua_tahun: bool,
}

fn cari_kolom(tabel: &'static Tabel, nama: &str) -> Option<&'static Kolom> {
    tabel.kolom.iter().find(|k| k.nama == nama)
}

/// Mencocokkan argumen model dengan katalog. Galat berupa teks yang bisa dibaca model.
pub fn rencana(args: &Value) -> Result<Rencana, String> {
    let kode = args.get("tabel").and_then(Value::as_str).unwrap_or("");
    let tabel = KATALOG
        .iter()
        .find(|t| t.kode == kode)
        .ok_or_else(|| format!("Tabel '{kode}' tidak ada. Panggil katalog_data untuk daftar tabel."))?;

    let agregasi = match args.get("agregasi").and_then(Value::as_str).unwrap_or("count") {
        "count" => Agregasi::Count,
        "sum" => Agregasi::Sum,
        "avg" => Agregasi::Avg,
        lain => return Err(format!("Agregasi '{lain}' tidak dikenal. Pilih count, sum, atau avg.")),
    };

    let kolom = match args.get("kolom").and_then(Value::as_str) {
        None => None,
        Some(nama) => {
            let k = cari_kolom(tabel, nama).ok_or_else(|| format!("Kolom '{nama}' tidak ada di tabel {kode}."))?;
            if k.tipe != Tipe::Angka {
                return Err(format!("Kolom '{nama}' bukan angka, tidak bisa di-{}.", if agregasi == Agregasi::Avg { "rata-rata" } else { "jumlahkan" }));
            }
            Some(k)
        }
    };
    if agregasi != Agregasi::Count && kolom.is_none() {
        return Err("Untuk sum atau avg, isi 'kolom' dengan kolom angka.".into());
    }

    let group_by = match args.get("group_by").and_then(Value::as_str) {
        None => None,
        Some(nama) => {
            let k = cari_kolom(tabel, nama).ok_or_else(|| format!("Kolom '{nama}' tidak ada di tabel {kode}."))?;
            if k.tipe != Tipe::Teks {
                return Err(format!("Kolom '{nama}' bukan teks, tidak bisa dijadikan kelompok."));
            }
            Some(k)
        }
    };

    Ok(Rencana {
        tabel,
        agregasi,
        kolom,
        group_by,
        semua_tahun: args.get("semua").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// Menyusun SQL dari rencana. Identifier hanya berasal dari katalog; nilai tahun diikat sebagai parameter.
/// Mengembalikan SQL dan nilai tahun yang harus diikat, bila ada.
pub fn susun(r: &Rencana, tahun: &str) -> (String, Option<String>) {
    let agregat = match (r.agregasi, r.kolom) {
        (Agregasi::Sum, Some(k)) => format!("CAST(COALESCE(SUM(t.{}), 0) AS DOUBLE)", k.nama),
        (Agregasi::Avg, Some(k)) => format!("CAST(COALESCE(AVG(t.{}), 0) AS DOUBLE)", k.nama),
        _ => "CAST(COUNT(*) AS DOUBLE)".to_string(),
    };
    let mut where_sql = String::from(" WHERE 1=1");
    let mut bind = None;
    if let (Some(kolom_tahun), false) = (r.tabel.kolom_tahun, r.semua_tahun) {
        where_sql.push_str(&format!(" AND t.{kolom_tahun} = ?"));
        bind = Some(tahun.to_string());
    }
    let sql = match r.group_by {
        Some(g) => format!(
            "SELECT COALESCE(CAST(t.{} AS CHAR), '(kosong)') AS grup, {agregat} AS nilai FROM {} t{where_sql} \
             GROUP BY grup ORDER BY nilai DESC LIMIT {HASIL_MAKS}",
            g.nama, r.tabel.sql
        ),
        None => format!("SELECT NULL AS grup, {agregat} AS nilai FROM {} t{where_sql}", r.tabel.sql),
    };
    (sql, bind)
}

/// Tool `katalog_data`: tabel dan kolom yang bisa dianalisis.
pub fn katalog() -> Value {
    let tabel: Vec<Value> = KATALOG
        .iter()
        .map(|t| {
            let kolom: Vec<Value> = t
                .kolom
                .iter()
                .map(|k| {
                    json!({
                        "nama": k.nama,
                        "tipe": if k.tipe == Tipe::Angka { "angka" } else { "teks" },
                    })
                })
                .collect();
            json!({ "tabel": t.kode, "label": t.label, "adaTahun": t.kolom_tahun.is_some(), "kolom": kolom })
        })
        .collect();
    json!({ "tabel": tabel })
}

/// Tool `analisa_tabel`. Mengembalikan data dan jumlah kelompok.
pub async fn jalankan(pool: &MySqlPool, args: &Value, tahun: &str) -> Result<(Value, Option<i64>), ApiError> {
    let r = rencana(args).map_err(|pesan| ApiError::new(StatusCode::BAD_REQUEST, pesan))?;
    let (sql, bind) = susun(&r, tahun);
    let mut q = sqlx::query(&sql);
    if let Some(b) = &bind {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await.map_err(internal)?;
    let mut hasil = Vec::with_capacity(rows.len());
    for row in &rows {
        hasil.push(json!({
            "grup": row.try_get::<String, _>("grup").map_err(internal)?,
            "nilai": row.try_get::<f64, _>("nilai").map_err(internal)?,
        }));
    }
    let jumlah = hasil.len() as i64;
    Ok((
        json!({
            "tabel": r.tabel.label,
            "tahun": if r.tabel.kolom_tahun.is_some() && !r.semua_tahun { Some(tahun) } else { None },
            "hasil": hasil,
            "terpotong": jumlah as usize >= HASIL_MAKS,
        }),
        Some(jumlah),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabel_tidak_dikenal_ditolak() {
        let e = rencana(&json!({ "tabel": "tbl_user" })).unwrap_err();
        assert!(e.contains("tidak ada"));
    }

    #[test]
    fn sum_butuh_kolom_angka() {
        assert!(rencana(&json!({ "tabel": "spm_sanitasi", "agregasi": "sum" })).is_err());
        assert!(rencana(&json!({ "tabel": "spm_sanitasi", "agregasi": "sum", "kolom": "jenis" })).is_err());
        assert!(rencana(&json!({ "tabel": "spm_sanitasi", "agregasi": "sum", "kolom": "pembiayaan_total" })).is_ok());
    }

    #[test]
    fn kolom_pribadi_tidak_bisa_dipilih() {
        assert!(rencana(&json!({ "tabel": "spm_sanitasi", "group_by": "alamat_lengkap" })).is_err());
        assert!(rencana(&json!({ "tabel": "spm_sanitasi", "kolom": "latitude", "agregasi": "sum" })).is_err());
    }

    #[test]
    fn susun_group_by_dan_tahun_terikat() {
        let r = rencana(&json!({ "tabel": "budget_spam", "agregasi": "sum", "kolom": "nilai_kontrak", "group_by": "sumber_dana" })).unwrap();
        let (sql, bind) = susun(&r, "2026");
        assert!(sql.contains("GROUP BY grup"));
        assert!(sql.contains("t.tahun = ?"));
        assert!(!sql.contains("2026"));
        assert_eq!(bind.as_deref(), Some("2026"));
    }

    #[test]
    fn semua_menonaktifkan_filter_tahun() {
        let r = rencana(&json!({ "tabel": "capaian_spam", "semua": true })).unwrap();
        let (sql, bind) = susun(&r, "2026");
        assert!(!sql.contains("t.tahun = ?"));
        assert!(bind.is_none());
    }

    #[test]
    fn tabel_tanpa_tahun_tidak_difilter() {
        let r = rencana(&json!({ "tabel": "spm_sanitasi", "group_by": "jenis" })).unwrap();
        let (sql, bind) = susun(&r, "2026");
        assert!(!sql.contains("= ?"));
        assert!(bind.is_none());
    }
}
