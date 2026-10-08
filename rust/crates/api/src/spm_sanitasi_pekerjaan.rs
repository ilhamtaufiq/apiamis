//! Tautan SPM sanitasi ke paket pekerjaan (`SpmSanitasiPekerjaanIntegrationService`) dan rute yang
//! memakainya: `GET /api/spm-sanitasi/{id}` (show), `GET /api/spm-sanitasi/mck-pekerjaan`, serta
//! `POST /api/spm-sanitasi/{id}/pekerjaan` dan `DELETE /api/spm-sanitasi/{id}/pekerjaan/{pekerjaanId}`.
//!
//! Isi:
//! - Klasifikasi komponen output (`classifySanitasiKomponen`) dan pemetaan jenis SPM.
//! - Daftar paket yang memenuhi `sanitasiPekerjaanQuery` (scope output, `byUserRole()` lewat
//!   `access::restriction`, filter tahun/kecamatan/desa/cari).
//! - Model pekerjaan seperti `toArray()` untuk show dan respon attach/detach. Ini bukan
//!   `PekerjaanDetailResource`, jadi `pekerjaan_detail` tidak dipakai.
//! - Sinkron master SPM dari paket tertaut (`syncInfrastrukturFromLinkedPekerjaan`). Perubahan baris SPM
//!   dicatat audit dan notifikasi admin, seperti `Auditable` dan `NotifiesAdminsOnChanges`.
//!
//! Perbedaan kecil:
//! - Attach dan detach berjalan dalam satu transaksi. Laravel tidak membungkusnya, jadi kegagalan di
//!   tengah bisa meninggalkan pivot tanpa sinkron.
//! - `linked_spm_ids` dan relasi `pekerjaan` diurutkan menurut id, karena Laravel tidak menentukan urutan.
//! - Klasifikasi memakai regex Rust. Kelas `\b` dan `\s` di sini Unicode, PHP `/u` tanpa UCP hanya ASCII.
//!   Untuk nama komponen berbahasa Indonesia hasilnya sama.

use std::{collections::HashMap, sync::OnceLock};

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySql, MySqlConnection, MySqlPool, Row, Transaction};

use crate::{
    access,
    changes::{self, Target},
    format::iso8601_utc,
    lookup::carbon_json,
    require_auth,
    spam_integration::is_countable_satuan,
    spm_sanitasi::{self, find, internal, resource_of, Arg},
    spm_sanitasi_capaian::JIWA_PER_KK,
    validation::Errors,
    AppState,
};

/// Jenis output yang diterima `mck_type` / `output_type` (`in:` di validasi).
pub const OUTPUT_TYPES: &[&str] = &[
    "mck",
    "mck_individu",
    "mck_komunal",
    "tangki_septik",
    "tangki_septik_individu",
    "tangki_septik_komunal",
    "ipal",
];

const SPM_TARGET: Target = Target {
    model_type: "App\\Models\\SpmSanitasi",
    label: "SpmSanitasi",
    tab: "",
};

// ---------------------------------------------------------------------------
// Klasifikasi komponen
// ---------------------------------------------------------------------------

static RE_IPAL: OnceLock<Regex> = OnceLock::new();
static RE_SPALD_T: OnceLock<Regex> = OnceLock::new();
static RE_SPALD_S: OnceLock<Regex> = OnceLock::new();
static RE_SPALD: OnceLock<Regex> = OnceLock::new();

fn rx(cell: &'static OnceLock<Regex>, pattern: &'static str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("pola regex tetap"))
}

/// `classifySanitasiKomponen`: `ipal`, `tangki_septik[_individu|_komunal]`, `mck[_individu|_komunal]`, atau `None`.
pub fn classify_sanitasi_komponen(komponen: &str) -> Option<&'static str> {
    // mb_strtolower(trim()), lalu `_ - / \` menjadi spasi dan spasi berurutan dipadatkan.
    let lowered = komponen.trim().to_lowercase();
    let replaced: String = lowered
        .chars()
        .map(|c| if matches!(c, '_' | '-' | '/' | '\\') { ' ' } else { c })
        .collect();
    let mut normalized = String::with_capacity(replaced.len());
    let mut in_space = false;
    for c in replaced.chars() {
        if c.is_whitespace() {
            if !in_space {
                normalized.push(' ');
            }
            in_space = true;
        } else {
            normalized.push(c);
            in_space = false;
        }
    }
    let compact: String = normalized.chars().filter(|c| *c != ' ').collect();

    // 1) IPAL / IPLT / SPALDT terpusat. Harus dicek sebelum SPALDS.
    let is_terpusat = rx(&RE_IPAL, r"\b(ipal|iplt)").is_match(&normalized)
        || compact.contains("spaldt")
        || rx(&RE_SPALD_T, r"\bspald\s*t\b").is_match(&normalized);
    if is_terpusat {
        return Some("ipal");
    }

    // 2) SPALDS / tangki septik setempat.
    let is_setempat = (normalized.contains("tangki") && normalized.contains("septik"))
        || normalized.contains("septic tank")
        || compact.contains("septictank")
        || compact.contains("spalds")
        || rx(&RE_SPALD_S, r"\bspald\s*s\b").is_match(&normalized)
        || rx(&RE_SPALD, r"\bspald\b").is_match(&normalized);
    if is_setempat {
        if normalized.contains("komunal") {
            return Some("tangki_septik_komunal");
        }
        if normalized.contains("individu") || normalized.contains("indvidu") {
            return Some("tangki_septik_individu");
        }
        return Some("tangki_septik");
    }

    // 3) MCK / jamban / toilet.
    let is_mck = normalized.contains("mck")
        || normalized.contains("jamban")
        || normalized.contains("wc ")
        || normalized.starts_with("wc")
        || normalized.contains(" toilet")
        || normalized.starts_with("toilet");
    if is_mck {
        if normalized.contains("komunal") {
            return Some("mck_komunal");
        }
        if normalized.contains("individu") || normalized.contains("indvidu") {
            return Some("mck_individu");
        }
        return Some("mck");
    }

    None
}

pub fn is_sanitasi_komponen(komponen: &str) -> bool {
    classify_sanitasi_komponen(komponen).is_some()
}

/// `spmJenisListForOutputType`: jenis SPM yang cocok untuk satu tipe output.
pub fn spm_jenis_list_for_output_type(output_type: Option<&str>) -> &'static [&'static str] {
    match output_type {
        Some("mck_individu") => &["mck_individu"],
        Some("mck_komunal") => &["mck_komunal"],
        Some("mck") => &["mck_individu", "mck_komunal"],
        Some("tangki_septik_individu" | "tangki_septik_komunal" | "tangki_septik") => &["spalds"],
        Some("ipal") => &["spaldt", "iplt"],
        _ => &[],
    }
}

/// `outputTypesForSpmJenis`: tipe output yang cocok untuk satu jenis SPM.
pub fn output_types_for_spm_jenis(spm_jenis: &str) -> &'static [&'static str] {
    match spm_jenis {
        "spaldt" | "iplt" => &["ipal"],
        "spalds" => &["tangki_septik_individu", "tangki_septik_komunal", "tangki_septik"],
        "mck_individu" => &["mck_individu", "mck"],
        "mck_komunal" => &["mck_komunal", "mck"],
        _ => &[],
    }
}

/// `outputMatchesSpmJenis`: IPLT juga menerima tipe yang cocok dengan SPALDT.
pub fn output_matches_spm_jenis(output_type: Option<&str>, spm_jenis: &str) -> bool {
    let direct = |t: &str| output_types_for_spm_jenis(spm_jenis).contains(&t);
    match output_type {
        None => false,
        Some(t) => {
            direct(t)
                || (spm_jenis == "iplt" && output_types_for_spm_jenis("spaldt").contains(&t))
        }
    }
}

/// Klausa scope komponen sanitasi (`applySanitasiOutputScope`) dengan alias `o` pada `tbl_output`.
const SANITASI_SCOPE_SQL: &str = "(LOWER(o.komponen) LIKE '%mck%' \
    OR LOWER(o.komponen) LIKE '%jamban%' \
    OR LOWER(o.komponen) LIKE '%toilet%' \
    OR LOWER(o.komponen) LIKE '%wc%' \
    OR ((LOWER(o.komponen) LIKE '%tangki%' AND LOWER(o.komponen) LIKE '%septik%') \
        OR LOWER(o.komponen) LIKE '%septic%' \
        OR LOWER(o.komponen) LIKE '%spalds%' \
        OR LOWER(o.komponen) LIKE '%spald-s%' \
        OR LOWER(o.komponen) LIKE '%spald s%') \
    OR LOWER(o.komponen) LIKE '%ipal%' \
    OR LOWER(o.komponen) LIKE '%iplt%' \
    OR LOWER(o.komponen) LIKE '%spaldt%' \
    OR LOWER(o.komponen) LIKE '%spald-t%' \
    OR LOWER(o.komponen) LIKE '%spald t%')";

/// `applyOutputTypeSqlFilter` dengan alias `o`. Tipe tak dikenal menghasilkan `1 = 0`.
fn output_type_sql(output_type: &str) -> &'static str {
    match output_type {
        "mck_individu" => "(LOWER(o.komponen) LIKE '%mck individu%' \
            OR LOWER(o.komponen) LIKE '%mck indvidu%' \
            OR LOWER(o.komponen) LIKE '%jamban individu%' \
            OR LOWER(o.komponen) LIKE '%toilet individu%')",
        "mck_komunal" => "(LOWER(o.komponen) LIKE '%mck komunal%' \
            OR LOWER(o.komponen) LIKE '%jamban komunal%' \
            OR LOWER(o.komponen) LIKE '%toilet komunal%')",
        "mck" => "((LOWER(o.komponen) LIKE '%mck%' \
            OR LOWER(o.komponen) LIKE '%jamban%' \
            OR LOWER(o.komponen) LIKE '%toilet%' \
            OR LOWER(o.komponen) LIKE '%wc%') \
            AND LOWER(o.komponen) NOT LIKE '%individu%' \
            AND LOWER(o.komponen) NOT LIKE '%indvidu%' \
            AND LOWER(o.komponen) NOT LIKE '%komunal%')",
        "tangki_septik_individu" => "(((LOWER(o.komponen) LIKE '%tangki%' AND LOWER(o.komponen) LIKE '%septik%') \
            OR LOWER(o.komponen) LIKE '%septic%' \
            OR LOWER(o.komponen) LIKE '%spalds%') \
            AND (LOWER(o.komponen) LIKE '%individu%' OR LOWER(o.komponen) LIKE '%indvidu%'))",
        "tangki_septik_komunal" => "(((LOWER(o.komponen) LIKE '%tangki%' AND LOWER(o.komponen) LIKE '%septik%') \
            OR LOWER(o.komponen) LIKE '%septic%' \
            OR LOWER(o.komponen) LIKE '%spalds%') \
            AND LOWER(o.komponen) LIKE '%komunal%')",
        "tangki_septik" => "((LOWER(o.komponen) LIKE '%tangki%' AND LOWER(o.komponen) LIKE '%septik%') \
            OR LOWER(o.komponen) LIKE '%septic%' \
            OR LOWER(o.komponen) LIKE '%spalds%' \
            OR LOWER(o.komponen) LIKE '%spald-s%' \
            OR LOWER(o.komponen) LIKE '%spald s%')",
        "ipal" => "(LOWER(o.komponen) LIKE '%ipal%' \
            OR LOWER(o.komponen) LIKE '%iplt%' \
            OR LOWER(o.komponen) LIKE '%spaldt%' \
            OR LOWER(o.komponen) LIKE '%spald-t%' \
            OR LOWER(o.komponen) LIKE '%spald t%')",
        _ => "1 = 0",
    }
}

// ---------------------------------------------------------------------------
// Pembacaan kolom sebagai model (`toArray`)
// ---------------------------------------------------------------------------

/// Tipe kolom untuk pembacaan model: `(CAST)` sesuai tipe, dan cast Eloquent yang berlaku.
#[derive(Clone, Copy)]
enum C {
    /// `int`/`bigint` (dan bool `tinyint(1)` di bawah `B`).
    I,
    /// `tinyint(1)` sebagai bool.
    B,
    /// `double`/`float` sebagai angka.
    F,
    /// Teks biasa.
    S,
    /// `decimal(p,2)` dengan cast `decimal:2`: teks dua desimal.
    D2,
    /// Cast `array` dari JSON teks.
    J,
    /// `timestamp` sebagai Carbon (`Y-m-d\TH:i:s.u\Z`).
    T,
    /// Cast `datetime` sebagai ISO (`+00:00`), seperti `KontrakResource`.
    Iso,
    /// Cast `date`: `Y-m-d`.
    Date,
}

type Cols = &'static [(&'static str, C)];

const PEKERJAAN_COLS: Cols = &[
    ("id", C::I),
    ("kode_rekening", C::S),
    ("nama_paket", C::S),
    ("kecamatan_id", C::I),
    ("desa_id", C::I),
    ("kegiatan_id", C::I),
    ("pagu", C::F),
    ("is_konsultan", C::B),
    ("status", C::S),
    ("catatan", C::S),
    ("created_at", C::T),
    ("updated_at", C::T),
    ("pengawas_id", C::I),
    ("pendamping_id", C::I),
];

const KEGIATAN_COLS: Cols = &[
    ("id", C::I),
    ("nama_program", C::S),
    ("sub_bidang", C::S),
    ("nama_kegiatan", C::S),
    ("nama_sub_kegiatan", C::S),
    ("tahun_anggaran", C::S),
    ("sumber_dana", C::S),
    ("pagu", C::D2),
    ("kode_rekening", C::J),
    ("nama_pptk", C::S),
    ("nip_pptk", C::S),
    ("sipd_id_sub_bl", C::I),
    ("kode_sub_giat", C::S),
    ("created_at", C::T),
    ("updated_at", C::T),
];

const OUTPUT_COLS: Cols = &[
    ("id", C::I),
    ("pekerjaan_id", C::I),
    ("komponen", C::S),
    ("satuan", C::S),
    ("volume", C::D2),
    ("penerima_is_optional", C::B),
    ("created_at", C::T),
    ("updated_at", C::T),
];

const DESA_COLS: Cols = &[
    ("id", C::I),
    ("n_desa", C::S),
    ("luas", C::F),
    ("jumlah_penduduk", C::I),
    ("jumlah_kk", C::I),
    ("target", C::I),
    ("bjp_master", C::I),
    ("kecamatan_id", C::I),
    ("created_at", C::T),
    ("updated_at", C::T),
];

const KONTRAK_COLS: Cols = &[
    ("id", C::I),
    ("id_kegiatan", C::I),
    ("id_pekerjaan", C::I),
    ("id_penyedia", C::I),
    ("kode_rup", C::S),
    ("kode_paket", C::S),
    ("nomor_penawaran", C::S),
    ("tanggal_penawaran", C::Date),
    ("nilai_kontrak", C::F),
    ("tgl_sppbj", C::Date),
    ("tgl_spk", C::Date),
    ("tgl_spmk", C::Date),
    ("tgl_selesai", C::Date),
    ("sppbj", C::S),
    ("spk", C::S),
    ("spmk", C::S),
    ("spse_sppbj_id", C::S),
    ("spse_spk_id", C::S),
    ("spse_rekanan_id", C::S),
    ("spse_pushed_at", C::Iso),
    ("spse_push_log", C::J),
    ("created_at", C::T),
    ("updated_at", C::T),
];

/// Daftar kolom `SELECT` dengan `CAST` sesuai tipe, untuk tabel tanpa alias.
fn select_list(cols: Cols) -> String {
    cols.iter()
        .map(|(n, k)| match k {
            C::I | C::B => format!("CAST({n} AS SIGNED) AS {n}"),
            C::F => format!("CAST({n} AS DOUBLE) AS {n}"),
            C::D2 | C::J => format!("CAST({n} AS CHAR) AS {n}"),
            C::Date => format!("DATE_FORMAT({n}, '%Y-%m-%d') AS {n}"),
            C::S | C::T | C::Iso => (*n).to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Baca satu baris menjadi atribut model, urut sesuai `cols`.
fn read_cols(row: &MySqlRow, cols: Cols) -> Result<Map<String, Value>, ApiError> {
    let mut m = Map::new();
    for (n, k) in cols {
        let v = match k {
            C::I => json!(row.try_get::<Option<i64>, _>(*n).map_err(internal)?),
            C::B => json!(row
                .try_get::<Option<i64>, _>(*n)
                .map_err(internal)?
                .map(|x| x != 0)),
            C::F => json!(row.try_get::<Option<f64>, _>(*n).map_err(internal)?),
            C::S | C::D2 | C::Date => {
                json!(row.try_get::<Option<String>, _>(*n).map_err(internal)?)
            }
            C::J => match row.try_get::<Option<String>, _>(*n).map_err(internal)? {
                Some(t) => serde_json::from_str::<Value>(&t).unwrap_or(Value::Null),
                None => Value::Null,
            },
            C::T => carbon_json(row.try_get::<Option<DateTime<Utc>>, _>(*n).map_err(internal)?),
            C::Iso => iso8601_utc(row.try_get::<Option<DateTime<Utc>>, _>(*n).map_err(internal)?),
        };
        m.insert((*n).to_string(), v);
    }
    Ok(m)
}

/// Eksekusi SQL dengan argumen posisional.
async fn rows(c: &mut MySqlConnection, sql: &str, args: &[Arg]) -> Result<Vec<MySqlRow>, ApiError> {
    spm_sanitasi::bind(sqlx::query(sql), args)
        .fetch_all(c)
        .await
        .map_err(internal)
}

/// `?,?,?` untuk `IN (...)`. Pemanggil memastikan daftar tidak kosong.
fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

fn id_args(ids: &[i64]) -> Vec<Arg> {
    ids.iter().map(|i| Arg::I(*i)).collect()
}

/// Nilai `integer` dari JSON seperti validasi `exists:` (angka atau teks angka).
fn json_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// `derivedMetricsForPekerjaan` dan `calculateProgressTotal` memakai `(float)` PHP.
fn php_float(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => {
            let t = s.trim_start();
            let end = t
                .char_indices()
                .take_while(|(i, c)| {
                    c.is_ascii_digit()
                        || *c == '.'
                        || ((*c == '-' || *c == '+') && *i == 0)
                        || ((*c == 'e' || *c == 'E') && *i > 0)
                })
                .map(|(i, c)| i + c.len_utf8())
                .last()
                .unwrap_or(0);
            t[..end].parse::<f64>().unwrap_or(0.0)
        }
        Value::Bool(b) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Pekerjaan dengan relasi perhitungan
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct OutRow {
    pub id: i64,
    pub komponen: String,
    pub satuan: String,
    pub volume: f64,
}

/// Pekerjaan beserta relasi yang dipakai daftar, integrasi, dan sinkron.
#[derive(Clone, Debug)]
pub struct Pkj {
    pub id: i64,
    pub nama_paket: Option<String>,
    pub pagu: f64,
    pub desa_id: Option<i64>,
    pub kecamatan_id: Option<i64>,
    /// `kegiatan?->tahun_anggaran`.
    pub tahun_anggaran: Option<String>,
    /// `desa?->id` dan `desa?->n_desa`. `None` bila relasi tidak ada.
    pub desa: Option<(i64, Option<String>)>,
    pub kecamatan: Option<(i64, Option<String>)>,
    /// Semua output (urut id), untuk klasifikasi dan fallback output.
    pub outputs: Vec<OutRow>,
    pub penerima_count: i64,
    pub penerima_jiwa: i64,
    /// `SUM(kontrak.nilai_kontrak)`.
    pub kontrak_sum: f64,
    pub progress: Option<Value>,
    /// Id SPM yang tertaut (urut id). `spmSanitasi`.
    pub spm_links: Vec<i64>,
}

/// Muat pekerjaan beserta relasi. Hasil urut sesuai `ids`, dan pekerjaan yang tidak ada dilewati.
pub async fn load_pekerjaan(c: &mut MySqlConnection, ids: &[i64]) -> Result<Vec<Pkj>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let ph = placeholders(ids.len());
    let args = id_args(ids);
    let mut map: HashMap<i64, Pkj> = HashMap::new();

    let base = format!(
        "SELECT CAST(p.id AS SIGNED) AS id, p.nama_paket, CAST(COALESCE(p.pagu, 0) AS DOUBLE) AS pagu, \
         CAST(p.desa_id AS SIGNED) AS desa_id, CAST(p.kecamatan_id AS SIGNED) AS kecamatan_id, \
         k.tahun_anggaran, CAST(d.id AS SIGNED) AS desa_row, d.n_desa AS desa_nama, \
         CAST(kc.id AS SIGNED) AS kec_row, kc.n_kec AS kec_nama \
         FROM tbl_pekerjaan p \
         LEFT JOIN tbl_kegiatan k ON k.id = p.kegiatan_id \
         LEFT JOIN tbl_desa d ON d.id = p.desa_id \
         LEFT JOIN tbl_kecamatan kc ON kc.id = p.kecamatan_id \
         WHERE p.id IN ({ph}) ORDER BY p.id"
    );
    for r in rows(c, &base, &args).await? {
        let id: i64 = r.try_get("id").map_err(internal)?;
        let desa_row: Option<i64> = r.try_get("desa_row").map_err(internal)?;
        let kec_row: Option<i64> = r.try_get("kec_row").map_err(internal)?;
        let desa_id: Option<i64> = r.try_get("desa_id").map_err(internal)?;
        let kecamatan_id: Option<i64> = r.try_get("kecamatan_id").map_err(internal)?;
        // `desa` dan `kecamatan` ada bila kolom foreign key terisi dan barisnya ditemukan.
        let desa = match (desa_id, desa_row) {
            (Some(_), Some(dr)) => Some((dr, r.try_get("desa_nama").map_err(internal)?)),
            _ => None,
        };
        let kecamatan = match (kecamatan_id, kec_row) {
            (Some(_), Some(kr)) => Some((kr, r.try_get("kec_nama").map_err(internal)?)),
            _ => None,
        };
        map.insert(
            id,
            Pkj {
                id,
                nama_paket: r.try_get("nama_paket").map_err(internal)?,
                pagu: r.try_get("pagu").map_err(internal)?,
                desa_id,
                kecamatan_id,
                tahun_anggaran: r.try_get("tahun_anggaran").map_err(internal)?,
                desa,
                kecamatan,
                outputs: Vec::new(),
                penerima_count: 0,
                penerima_jiwa: 0,
                kontrak_sum: 0.0,
                progress: None,
                spm_links: Vec::new(),
            },
        );
    }

    for r in rows(
        c,
        &format!(
            "SELECT CAST(o.id AS SIGNED) AS id, CAST(o.pekerjaan_id AS SIGNED) AS pekerjaan_id, \
             o.komponen, o.satuan, CAST(COALESCE(o.volume, 0) AS DOUBLE) AS volume \
             FROM tbl_output o WHERE o.pekerjaan_id IN ({ph}) ORDER BY o.id"
        ),
        &args,
    )
    .await?
    {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        if let Some(p) = map.get_mut(&pid) {
            p.outputs.push(OutRow {
                id: r.try_get("id").map_err(internal)?,
                komponen: r.try_get::<Option<String>, _>("komponen").map_err(internal)?.unwrap_or_default(),
                satuan: r.try_get::<Option<String>, _>("satuan").map_err(internal)?.unwrap_or_default(),
                volume: r.try_get("volume").map_err(internal)?,
            });
        }
    }

    for r in rows(
        c,
        &format!(
            "SELECT CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(COUNT(*) AS SIGNED) AS cnt, \
             CAST(COALESCE(SUM(jumlah_jiwa), 0) AS SIGNED) AS jiwa \
             FROM tbl_penerima WHERE pekerjaan_id IN ({ph}) GROUP BY pekerjaan_id"
        ),
        &args,
    )
    .await?
    {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        if let Some(p) = map.get_mut(&pid) {
            p.penerima_count = r.try_get("cnt").map_err(internal)?;
            p.penerima_jiwa = r.try_get("jiwa").map_err(internal)?;
        }
    }

    for r in rows(
        c,
        &format!(
            "SELECT CAST(kp.pekerjaan_id AS SIGNED) AS pekerjaan_id, \
             CAST(COALESCE(SUM(kt.nilai_kontrak), 0) AS DOUBLE) AS nilai \
             FROM kontrak_pekerjaan kp JOIN tbl_kontrak kt ON kt.id = kp.kontrak_id \
             WHERE kp.pekerjaan_id IN ({ph}) GROUP BY kp.pekerjaan_id"
        ),
        &args,
    )
    .await?
    {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        if let Some(p) = map.get_mut(&pid) {
            p.kontrak_sum = r.try_get("nilai").map_err(internal)?;
        }
    }

    // hasOne `progress`: baris pertama menurut id.
    for r in rows(
        c,
        &format!(
            "SELECT CAST(pr.pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(pr.content AS CHAR) AS content \
             FROM tbl_progress pr WHERE pr.pekerjaan_id IN ({ph}) ORDER BY pr.id"
        ),
        &args,
    )
    .await?
    {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        let content: Option<String> = r.try_get("content").map_err(internal)?;
        if let Some(p) = map.get_mut(&pid) {
            if p.progress.is_none() {
                p.progress = content.and_then(|t| serde_json::from_str::<Value>(&t).ok());
            }
        }
    }

    for r in rows(
        c,
        &format!(
            "SELECT CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(spm_sanitasi_id AS SIGNED) AS spm_id \
             FROM tbl_spm_sanitasi_pekerjaan WHERE pekerjaan_id IN ({ph}) ORDER BY spm_sanitasi_id"
        ),
        &args,
    )
    .await?
    {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        if let Some(p) = map.get_mut(&pid) {
            p.spm_links.push(r.try_get("spm_id").map_err(internal)?);
        }
    }

    Ok(ids.iter().filter_map(|id| map.remove(id)).collect())
}

/// Output sanitasi yang sudah diklasifikasi (`sanitasiOutputsForPekerjaan`).
pub struct SanitasiOutput {
    pub id: i64,
    pub komponen: String,
    pub satuan: String,
    pub volume: f64,
    pub output_type: &'static str,
    pub target_jenis_list: &'static [&'static str],
}

pub fn sanitasi_outputs(p: &Pkj) -> Vec<SanitasiOutput> {
    p.outputs
        .iter()
        .filter_map(|o| {
            classify_sanitasi_komponen(&o.komponen).map(|t| SanitasiOutput {
                id: o.id,
                komponen: o.komponen.clone(),
                satuan: o.satuan.clone(),
                volume: o.volume,
                output_type: t,
                target_jenis_list: spm_jenis_list_for_output_type(Some(t)),
            })
        })
        .collect()
}

/// Metrik turunan (`derivedMetricsForPekerjaan`).
#[derive(Clone, Copy, Debug)]
pub struct Derived {
    pub unit: i64,
    pub kk: i64,
    pub jiwa: i64,
    pub nilai_kontrak: f64,
    pub tahun_konstruksi_suggested: Option<i64>,
    pub progress_total: f64,
}

pub fn derived(p: &Pkj) -> Derived {
    let mut unit: i64 = 0;
    for o in sanitasi_outputs(p) {
        // Volume bersatuan panjang, luas, volume, atau LS tidak dihitung sebagai unit.
        if !is_countable_satuan(Some(o.satuan.as_str())) {
            continue;
        }
        unit += o.volume.round() as i64;
    }

    let mut kk = p.penerima_count;
    if kk == 0 && unit > 0 {
        kk = unit;
    }
    let jiwa = if p.penerima_jiwa > 0 {
        p.penerima_jiwa
    } else {
        kk * JIWA_PER_KK
    };
    let pembiayaan = if p.kontrak_sum > 0.0 { p.kontrak_sum } else { p.pagu };

    let tahun_konstruksi_suggested = p.tahun_anggaran.as_deref().and_then(|t| {
        let parsed = spm_sanitasi::php_int(t);
        (1900..=2100).contains(&parsed).then_some(parsed)
    });

    Derived {
        unit,
        kk,
        jiwa,
        nilai_kontrak: pembiayaan,
        tahun_konstruksi_suggested,
        progress_total: calculate_progress_total(p.progress.as_ref()),
    }
}

/// `calculateProgressTotal`: rata-rata `progress` item (array atau objek), dibulatkan satu desimal.
fn calculate_progress_total(content: Option<&Value>) -> f64 {
    let items: Vec<&Value> = match content {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(Value::Object(o)) => o.values().collect(),
        _ => return 0.0,
    };
    let values: Vec<f64> = items
        .into_iter()
        .filter_map(|item| match item {
            Value::Object(o) => o.get("progress").filter(|v| !v.is_null()).map(php_float),
            _ => None,
        })
        .collect();
    if values.is_empty() {
        return 0.0;
    }
    let avg = values.iter().sum::<f64>() / values.len() as f64;
    (avg * 10.0).round() / 10.0
}

/// `resolvePembiayaanFromPekerjaan` sudah ada di `derived`. Fungsi ini dipakai aggregate dan format.
pub fn derived_json(d: &Derived, pembiayaan: f64) -> Value {
    json!({
        "unit": d.unit,
        "mck_unit": d.unit,
        "kk": d.kk,
        "jiwa": d.jiwa,
        "nilai_kontrak": pembiayaan,
        "pembiayaan_suggested": pembiayaan,
        "tahun_konstruksi_suggested": d.tahun_konstruksi_suggested,
        "progress_total": d.progress_total,
    })
}

/// `formatSanitasiPekerjaan`: bentuk daftar integrasi. `linked` menentukan `is_linked` bila diberikan.
pub fn format_pekerjaan(p: &Pkj, linked: Option<i64>) -> Value {
    let d = derived(p);
    let outs = sanitasi_outputs(p);
    let outputs_json: Vec<Value> = outs
        .iter()
        .map(|o| {
            let target = o.target_jenis_list.first().copied();
            json!({
                "id": o.id,
                "komponen": o.komponen,
                "satuan": o.satuan,
                "volume": o.volume,
                "output_type": o.output_type,
                "target_jenis": target,
                "target_jenis_list": o.target_jenis_list,
                "mck_type": o.output_type,
            })
        })
        .collect();

    let mut output_types: Vec<&str> = Vec::new();
    let mut target_list: Vec<&str> = Vec::new();
    for o in &outs {
        if !output_types.contains(&o.output_type) {
            output_types.push(o.output_type);
        }
        for &t in o.target_jenis_list {
            if !target_list.contains(&t) {
                target_list.push(t);
            }
        }
    }

    let is_linked = match linked {
        Some(id) => p.spm_links.contains(&id),
        None => !p.spm_links.is_empty(),
    };

    json!({
        "id": p.id,
        "nama_paket": p.nama_paket,
        "pagu": p.pagu,
        "tahun_anggaran": p.tahun_anggaran,
        "desa": {
            "id": p.desa.as_ref().map(|d| d.0),
            "n_desa": p.desa.as_ref().and_then(|d| d.1.clone()),
        },
        "kecamatan": {
            "id": p.kecamatan.as_ref().map(|k| k.0),
            "n_kec": p.kecamatan.as_ref().and_then(|k| k.1.clone()),
        },
        "sanitasi_outputs": outputs_json,
        "mck_outputs": outputs_json,
        "output_types": output_types,
        "mck_types": output_types,
        "target_jenis_list": target_list,
        "derived": derived_json(&d, d.nilai_kontrak),
        "is_linked": is_linked,
        "linked_spm_ids": p.spm_links,
    })
}

// ---------------------------------------------------------------------------
// Daftar paket (`sanitasiPekerjaanQuery`)
// ---------------------------------------------------------------------------

/// Filter daftar paket. Setiap field `Some` menambah satu klausa.
#[derive(Default, Clone)]
pub struct PkjFilter {
    pub tahun: Option<String>,
    pub kecamatan_id: Option<i64>,
    pub desa_id: Option<i64>,
    pub search: Option<String>,
    pub output_type: Option<String>,
    /// Batasi ke desa SPM (bila SPM punya desa). Tidak ada efek bila SPM tidak ditemukan.
    pub spm_id: Option<i64>,
    /// `unlinked_only`: paket yang belum tertaut ke SPM ini.
    pub unlinked_spm: Option<i64>,
    /// Hanya satu id paket (dipakai attach).
    pub only_id: Option<i64>,
}

/// Klausa `WHERE` sebagai ` AND ...` (alias `p`), dengan `byUserRole()` untuk user yang login.
async fn filter_clauses(
    c: &mut MySqlConnection,
    f: &PkjFilter,
    user_id: u64,
    roles: &[(u64, String)],
) -> Result<(String, Vec<Arg>), ApiError> {
    let mut sql = format!(" AND EXISTS (SELECT 1 FROM tbl_output o WHERE o.pekerjaan_id = p.id AND {SANITASI_SCOPE_SQL}");
    let mut args: Vec<Arg> = Vec::new();
    if let Some(t) = &f.output_type {
        sql.push_str(&format!(" AND {}", output_type_sql(t)));
    }
    sql.push(')');

    if let Some(t) = &f.tahun {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM tbl_kegiatan k WHERE k.id = p.kegiatan_id AND k.tahun_anggaran = ?)",
        );
        args.push(Arg::S(t.clone()));
    }
    if let Some(k) = f.kecamatan_id {
        sql.push_str(" AND p.kecamatan_id = ?");
        args.push(Arg::I(k));
    }
    if let Some(d) = f.desa_id {
        sql.push_str(" AND p.desa_id = ?");
        args.push(Arg::I(d));
    }
    if let Some(spm) = f.spm_id {
        let desa: Option<Option<i64>> = sqlx::query_scalar(
            "SELECT CAST(desa_id AS SIGNED) FROM tbl_spm_sanitasi WHERE id = ?",
        )
        .bind(spm)
        .fetch_optional(&mut *c)
        .await
        .map_err(internal)?;
        if let Some(Some(desa_id)) = desa {
            sql.push_str(" AND p.desa_id = ?");
            args.push(Arg::I(desa_id));
        }
    }
    if let Some(s) = &f.search {
        let like = format!("%{s}%");
        sql.push_str(
            " AND (p.nama_paket LIKE ? \
             OR EXISTS (SELECT 1 FROM tbl_desa dd WHERE dd.id = p.desa_id AND dd.n_desa LIKE ?) \
             OR EXISTS (SELECT 1 FROM tbl_kecamatan kc WHERE kc.id = p.kecamatan_id AND kc.n_kec LIKE ?))",
        );
        for _ in 0..3 {
            args.push(Arg::S(like.clone()));
        }
    }
    if let Some(spm) = f.unlinked_spm {
        sql.push_str(
            " AND NOT EXISTS (SELECT 1 FROM tbl_spm_sanitasi_pekerjaan sp \
             WHERE sp.pekerjaan_id = p.id AND sp.spm_sanitasi_id = ?)",
        );
        args.push(Arg::I(spm));
    }
    if let Some(id) = f.only_id {
        sql.push_str(" AND p.id = ?");
        args.push(Arg::I(id));
    }

    // byUserRole(): admin tanpa batasan, selain itu dibatasi penugasan dan peran kegiatan.
    let r = access::restriction(user_id, roles, "p");
    sql.push_str(&r.sql);
    args.extend(r.binds.into_iter().map(|b| Arg::I(b as i64)));

    Ok((sql, args))
}

/// Id paket yang cocok dengan filter, urut `order` (`ASC` atau `DESC`).
pub async fn pekerjaan_ids(
    c: &mut MySqlConnection,
    f: &PkjFilter,
    user_id: u64,
    roles: &[(u64, String)],
    desc: bool,
) -> Result<Vec<i64>, ApiError> {
    let (clauses, args) = filter_clauses(c, f, user_id, roles).await?;
    let order = if desc { "DESC" } else { "ASC" };
    let sql = format!(
        "SELECT CAST(p.id AS SIGNED) FROM tbl_pekerjaan p WHERE 1 = 1{clauses} ORDER BY p.id {order}"
    );
    rows(c, &sql, &args)
        .await?
        .iter()
        .map(|r| r.try_get::<i64, _>(0).map_err(internal))
        .collect()
}

/// Jumlah paket untuk halaman (`COUNT` dengan filter yang sama).
pub async fn pekerjaan_count(
    c: &mut MySqlConnection,
    f: &PkjFilter,
    user_id: u64,
    roles: &[(u64, String)],
) -> Result<i64, ApiError> {
    let (clauses, args) = filter_clauses(c, f, user_id, roles).await?;
    let sql = format!("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_pekerjaan p WHERE 1 = 1{clauses}");
    let r = rows(c, &sql, &args).await?;
    r.first()
        .map(|r| r.try_get::<i64, _>(0).map_err(internal))
        .unwrap_or(Ok(0))
}

// ---------------------------------------------------------------------------
// Model SPM dan pekerjaan sebagai `toArray()`
// ---------------------------------------------------------------------------

/// Relasi yang dimuat untuk pekerjaan di dalam `pekerjaan` SPM.
#[derive(Clone, Copy, Default)]
pub struct Rel {
    pub kegiatan: bool,
    pub output: bool,
    pub desa: bool,
    pub kontrak: bool,
}

/// Pekerjaan tertaut ke SPM sebagai model JSON: atribut, relasi yang diminta, dan `pivot`.
pub async fn spm_pekerjaan_models(
    c: &mut MySqlConnection,
    spm_id: i64,
    rel: Rel,
) -> Result<Vec<Value>, ApiError> {
    let pivots = rows(
        c,
        "SELECT CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(output_id AS SIGNED) AS output_id, \
         DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') AS created_at, \
         DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s') AS updated_at \
         FROM tbl_spm_sanitasi_pekerjaan WHERE spm_sanitasi_id = ? ORDER BY id",
        &[Arg::I(spm_id)],
    )
    .await?;
    if pivots.is_empty() {
        return Ok(Vec::new());
    }
    let mut pivot_rows: Vec<(i64, Map<String, Value>)> = Vec::new();
    for r in &pivots {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        let oid: Option<i64> = r.try_get("output_id").map_err(internal)?;
        let mut pv = Map::new();
        pv.insert("spm_sanitasi_id".into(), json!(spm_id));
        pv.insert("pekerjaan_id".into(), json!(pid));
        pv.insert("output_id".into(), json!(oid));
        pv.insert(
            "created_at".into(),
            json!(r.try_get::<Option<String>, _>("created_at").map_err(internal)?),
        );
        pv.insert(
            "updated_at".into(),
            json!(r.try_get::<Option<String>, _>("updated_at").map_err(internal)?),
        );
        pivot_rows.push((pid, pv));
    }

    let ids: Vec<i64> = pivot_rows.iter().map(|(p, _)| *p).collect();
    let ph = placeholders(ids.len());
    let args = id_args(&ids);

    let mut pekerjaan: HashMap<i64, Map<String, Value>> = HashMap::new();
    for r in rows(
        c,
        &format!(
            "SELECT {} FROM tbl_pekerjaan WHERE id IN ({ph})",
            select_list(PEKERJAAN_COLS)
        ),
        &args,
    )
    .await?
    {
        let m = read_cols(&r, PEKERJAAN_COLS)?;
        let id = m["id"].as_i64().unwrap_or_default();
        pekerjaan.insert(id, m);
    }

    let kegiatan_ids: Vec<i64> = pekerjaan
        .values()
        .filter_map(|m| m["kegiatan_id"].as_i64())
        .collect();
    let mut kegiatan: HashMap<i64, Map<String, Value>> = HashMap::new();
    if rel.kegiatan && !kegiatan_ids.is_empty() {
        let kph = placeholders(kegiatan_ids.len());
        for r in rows(
            c,
            &format!(
                "SELECT {} FROM tbl_kegiatan WHERE id IN ({kph})",
                select_list(KEGIATAN_COLS)
            ),
            &id_args(&kegiatan_ids),
        )
        .await?
        {
            let m = read_cols(&r, KEGIATAN_COLS)?;
            kegiatan.insert(m["id"].as_i64().unwrap_or_default(), m);
        }
    }

    let mut outputs: HashMap<i64, Vec<Value>> = HashMap::new();
    if rel.output {
        for r in rows(
            c,
            &format!(
                "SELECT {} FROM tbl_output WHERE pekerjaan_id IN ({ph}) ORDER BY id",
                select_list(OUTPUT_COLS)
            ),
            &args,
        )
        .await?
        {
            let m = read_cols(&r, OUTPUT_COLS)?;
            let pid = m["pekerjaan_id"].as_i64().unwrap_or_default();
            outputs.entry(pid).or_default().push(Value::Object(m));
        }
    }

    let desa_ids: Vec<i64> = pekerjaan.values().filter_map(|m| m["desa_id"].as_i64()).collect();
    let mut desa: HashMap<i64, Map<String, Value>> = HashMap::new();
    if rel.desa && !desa_ids.is_empty() {
        let dph = placeholders(desa_ids.len());
        for r in rows(
            c,
            &format!("SELECT {} FROM tbl_desa WHERE id IN ({dph})", select_list(DESA_COLS)),
            &id_args(&desa_ids),
        )
        .await?
        {
            let m = read_cols(&r, DESA_COLS)?;
            desa.insert(m["id"].as_i64().unwrap_or_default(), m);
        }
    }

    // belongsToMany `kontrak` lewat `kontrak_pekerjaan`, dengan pivot kontrak.
    let mut kontrak: HashMap<i64, Vec<(Map<String, Value>, Map<String, Value>)>> = HashMap::new();
    if rel.kontrak {
        let mut kontrak_by_id: HashMap<i64, Map<String, Value>> = HashMap::new();
        let mut links: Vec<(i64, i64, Map<String, Value>)> = Vec::new();
        for r in rows(
            c,
            &format!(
                "SELECT CAST(kontrak_id AS SIGNED) AS kontrak_id, CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, \
                 DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') AS created_at, \
                 DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s') AS updated_at \
                 FROM kontrak_pekerjaan WHERE pekerjaan_id IN ({ph}) ORDER BY kontrak_id"
            ),
            &args,
        )
        .await?
        {
            let kid: i64 = r.try_get("kontrak_id").map_err(internal)?;
            let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
            let mut pv = Map::new();
            pv.insert("pekerjaan_id".into(), json!(pid));
            pv.insert("kontrak_id".into(), json!(kid));
            pv.insert(
                "created_at".into(),
                json!(r.try_get::<Option<String>, _>("created_at").map_err(internal)?),
            );
            pv.insert(
                "updated_at".into(),
                json!(r.try_get::<Option<String>, _>("updated_at").map_err(internal)?),
            );
            links.push((pid, kid, pv));
        }
        let kids: Vec<i64> = links.iter().map(|(_, k, _)| *k).collect();
        if !kids.is_empty() {
            let kph = placeholders(kids.len());
            for r in rows(
                c,
                &format!(
                    "SELECT {} FROM tbl_kontrak WHERE id IN ({kph})",
                    select_list(KONTRAK_COLS)
                ),
                &id_args(&kids),
            )
            .await?
            {
                let m = read_cols(&r, KONTRAK_COLS)?;
                kontrak_by_id.insert(m["id"].as_i64().unwrap_or_default(), m);
            }
        }
        for (pid, kid, pv) in links {
            if let Some(m) = kontrak_by_id.get(&kid) {
                kontrak.entry(pid).or_default().push((m.clone(), pv));
            }
        }
    }

    let mut out = Vec::with_capacity(pivot_rows.len());
    for (pid, pv) in pivot_rows {
        let Some(mut attrs) = pekerjaan.get(&pid).cloned() else {
            // Relasi belongsToMany hanya memuat pekerjaan yang masih ada.
            continue;
        };
        if rel.kegiatan {
            let k = attrs["kegiatan_id"]
                .as_i64()
                .and_then(|kid| kegiatan.get(&kid).cloned())
                .map_or(Value::Null, Value::Object);
            attrs.insert("kegiatan".into(), k);
        }
        if rel.output {
            attrs.insert(
                "output".into(),
                Value::Array(outputs.remove(&pid).unwrap_or_default()),
            );
        }
        if rel.desa {
            let d = attrs["desa_id"]
                .as_i64()
                .and_then(|did| desa.get(&did).cloned())
                .map_or(Value::Null, Value::Object);
            attrs.insert("desa".into(), d);
        }
        if rel.kontrak {
            let list: Vec<Value> = kontrak
                .remove(&pid)
                .unwrap_or_default()
                .into_iter()
                .map(|(mut km, kpv)| {
                    km.insert("pivot".into(), Value::Object(kpv));
                    Value::Object(km)
                })
                .collect();
            attrs.insert("kontrak".into(), Value::Array(list));
        }
        attrs.insert("pivot".into(), Value::Object(pv));
        out.push(Value::Object(attrs));
    }
    Ok(out)
}

/// Model SPM beserta `desa` (dengan kecamatan) dan `pekerjaan` sesuai relasi yang diminta.
async fn spm_model(
    pool: &MySqlPool,
    spm_id: i64,
    rel: Option<Rel>,
) -> Result<Option<Value>, ApiError> {
    let Some(attrs) = find(pool, spm_id).await.map_err(internal)? else {
        return Ok(None);
    };
    let mut data = resource_of(pool, attrs).await?;
    if let (Some(rel), Some(obj)) = (rel, data.as_object_mut()) {
        let mut c = pool.acquire().await.map_err(internal)?;
        obj.insert(
            "pekerjaan".into(),
            Value::Array(spm_pekerjaan_models(&mut c, spm_id, rel).await?),
        );
    }
    Ok(Some(data))
}

// ---------------------------------------------------------------------------
// Sinkron master SPM dari paket tertaut
// ---------------------------------------------------------------------------

/// Konteks audit untuk perubahan yang dipicu sinkron: header, pengguna, dan URL permintaan.
pub struct Audit<'a> {
    pub headers: &'a HeaderMap,
    pub actor: u64,
    pub url: &'a str,
}

/// Nilai kolom yang ditulis sinkron.
#[derive(Clone, Copy)]
enum V {
    I(Option<i64>),
    F(Option<f64>),
    B(bool),
}

impl V {
    fn json(self) -> Value {
        match self {
            V::I(v) => json!(v),
            V::F(v) => json!(v),
            V::B(v) => json!(v),
        }
    }
}

fn same(before: &Value, after: &Value) -> bool {
    match (before, after) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(_), Value::Number(_)) => before.as_f64() == after.as_f64(),
        _ => before == after,
    }
}

/// `syncInfrastrukturFromLinkedPekerjaan`: isi KK, jiwa, pembiayaan, dan tahun dari paket tertaut.
/// Hanya baris yang berubah ditulis, dan perubahannya diaudit.
pub async fn sync_infrastruktur(
    tx: &mut Transaction<'_, MySql>,
    audit: &Audit<'_>,
    spm_id: i64,
) -> Result<(), ApiError> {
    let Some(current) = find(&mut **tx, spm_id).await.map_err(internal)? else {
        return Ok(());
    };
    let geti = |k: &str| current.get(k).and_then(Value::as_i64);
    let getf = |k: &str| current.get(k).and_then(Value::as_f64);
    let getb = |k: &str| current.get(k).and_then(Value::as_bool).unwrap_or(false);

    let mut updates: Vec<(&'static str, V)> = Vec::new();
    let link_ids: Vec<i64> = {
        let rs = rows(
            &mut **tx,
            "SELECT CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id FROM tbl_spm_sanitasi_pekerjaan \
             WHERE spm_sanitasi_id = ? ORDER BY id",
            &[Arg::I(spm_id)],
        )
        .await?;
        rs.iter()
            .map(|r| r.try_get::<i64, _>(0).map_err(internal))
            .collect::<Result<_, _>>()?
    };

    if link_ids.is_empty() {
        if getb("pemanfaat_dari_integrasi") {
            updates.push(("jumlah_pemanfaat_kk", V::I(None)));
            updates.push(("jumlah_pemanfaat_jiwa", V::I(None)));
            updates.push(("pemanfaat_dari_integrasi", V::B(false)));
        }
        if getb("pembiayaan_dari_integrasi") {
            updates.push(("pembiayaan_total", V::F(None)));
            updates.push(("pembiayaan_dari_integrasi", V::B(false)));
        }
    } else {
        let pkjs = load_pekerjaan(&mut **tx, &link_ids).await?;
        let mut kk_total: i64 = 0;
        let mut jiwa_total: i64 = 0;
        let mut pembiayaan_total: f64 = 0.0;
        let mut tahun_candidates: Vec<i64> = Vec::new();

        for p in &pkjs {
            let d = derived(p);
            if let Some(t) = d.tahun_konstruksi_suggested {
                tahun_candidates.push(t);
            }
            // KK dan biaya hanya dihitung pada master dengan id terkecil dari paket ini.
            let owner = p.spm_links.iter().min().copied().unwrap_or(0);
            if owner != spm_id {
                continue;
            }
            kk_total += d.kk;
            jiwa_total += d.jiwa;
            pembiayaan_total += d.nilai_kontrak;
        }

        let pemanfaat_kosong = geti("jumlah_pemanfaat_kk").unwrap_or(0) == 0;
        if getb("pemanfaat_dari_integrasi") || (pemanfaat_kosong && kk_total > 0) {
            updates.push(("jumlah_pemanfaat_kk", V::I((kk_total > 0).then_some(kk_total))));
            updates.push(("jumlah_pemanfaat_jiwa", V::I((kk_total > 0).then_some(jiwa_total))));
            updates.push(("pemanfaat_dari_integrasi", V::B(true)));
        }

        let pembiayaan_kosong = getf("pembiayaan_total").unwrap_or(0.0) <= 0.0;
        if getb("pembiayaan_dari_integrasi") || (pembiayaan_kosong && pembiayaan_total > 0.0) {
            updates.push((
                "pembiayaan_total",
                V::F((pembiayaan_total > 0.0).then_some(pembiayaan_total)),
            ));
            updates.push(("pembiayaan_dari_integrasi", V::B(true)));
        }

        if geti("tahun_konstruksi").is_none() {
            if let Some(t) = tahun_candidates.iter().min() {
                updates.push(("tahun_konstruksi", V::I(Some(*t))));
            }
        }
    }

    // Eloquent hanya menulis kolom yang benar-benar berubah.
    let dirty: Vec<(&'static str, V)> = updates
        .into_iter()
        .filter(|(name, v)| {
            let before = current.get(*name).cloned().unwrap_or(Value::Null);
            !same(&before, &v.json())
        })
        .collect();
    if dirty.is_empty() {
        return Ok(());
    }

    let sets: Vec<String> = dirty.iter().map(|(n, _)| format!("{n} = ?")).collect();
    let sql = format!(
        "UPDATE tbl_spm_sanitasi SET {}, updated_at = NOW() WHERE id = ?",
        sets.join(", ")
    );
    let mut q = sqlx::query(&sql);
    for (_, v) in &dirty {
        q = match v {
            V::I(x) => q.bind(*x),
            V::F(x) => q.bind(*x),
            V::B(x) => q.bind(i64::from(*x)),
        };
    }
    q.bind(spm_id).execute(&mut **tx).await.map_err(internal)?;

    let after = find(&mut **tx, spm_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let mut old = Map::new();
    let mut new = Map::new();
    for n in dirty.iter().map(|(n, _)| *n).chain(std::iter::once("updated_at")) {
        old.insert(n.to_string(), current.get(n).cloned().unwrap_or(Value::Null));
        new.insert(n.to_string(), after.get(n).cloned().unwrap_or(Value::Null));
    }
    changes::log_linked(
        tx,
        audit.headers,
        audit.actor,
        &SPM_TARGET,
        "updated",
        spm_id,
        Some(old),
        Some(new),
        None,
        audit.url,
    )
    .await?;
    Ok(())
}

/// `resyncInfrastrukturSharingPekerjaan`: sinkron SPM ini, lalu SPM lain yang juga tertaut ke paket ini.
pub async fn resync_sharing(
    tx: &mut Transaction<'_, MySql>,
    audit: &Audit<'_>,
    spm_id: i64,
    pekerjaan_id: i64,
) -> Result<(), ApiError> {
    sync_infrastruktur(tx, audit, spm_id).await?;
    let others: Vec<i64> = rows(
        &mut **tx,
        "SELECT CAST(spm_sanitasi_id AS SIGNED) AS spm_id FROM tbl_spm_sanitasi_pekerjaan \
         WHERE pekerjaan_id = ? AND spm_sanitasi_id <> ? ORDER BY spm_sanitasi_id",
        &[Arg::I(pekerjaan_id), Arg::I(spm_id)],
    )
    .await?
    .iter()
    .map(|r| r.try_get::<i64, _>(0).map_err(internal))
    .collect::<Result<_, _>>()?;
    for other in others {
        sync_infrastruktur(tx, audit, other).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

fn url_for(state: &AppState, suffix: &str) -> String {
    format!("{}/api/spm-sanitasi{suffix}", state.app_url.trim_end_matches('/'))
}

fn parse_object(body: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// `filled()` untuk teks dari query.
fn filled(q: &HashMap<String, String>, key: &str) -> Option<String> {
    spm_sanitasi::input(q, key)
}

/// Nilai `in:` untuk tipe output, dengan nama atribut Laravel.
fn check_output_type(e: &mut Errors, field: &str, attr: &str, raw: Option<&String>) -> Option<String> {
    let v = raw?;
    if OUTPUT_TYPES.contains(&v.as_str()) {
        Some(v.clone())
    } else {
        e.add(field, format!("The selected {attr} is invalid."));
        None
    }
}

/// `GET /api/spm-sanitasi/{id}`: model SPM dengan `desa.kecamatan` dan `pekerjaan` (kegiatan, output, desa).
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let rel = Rel {
        kegiatan: true,
        output: true,
        desa: true,
        kontrak: false,
    };
    let data = spm_model(&state.pool, id, Some(rel))
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "success": true, "data": data })).into_response())
}

/// `GET /api/spm-sanitasi/mck-pekerjaan`: paginator paket sanitasi dengan metrik turunan.
pub async fn mck_pekerjaan(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let mut e = Errors::default();
    let mck_type = check_output_type(&mut e, "mck_type", "mck type", filled(&q, "mck_type").as_ref());
    let output_type = check_output_type(&mut e, "output_type", "output type", filled(&q, "output_type").as_ref());
    e.finish()?;

    // `output_type` diutamakan, lalu `mck_type`.
    let output_type = output_type.or(mck_type);
    let spm_id = spm_sanitasi::int_or_null(&q, "spm_sanitasi_id");
    let unlinked = spm_sanitasi::input(&q, "unlinked_only")
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "on" | "yes"))
        .unwrap_or(false);
    let per_page = spm_sanitasi::int_or(&q, "per_page", 15).max(1) as u64;
    let page = spm_sanitasi::int_or(&q, "page", 1).max(1) as u64;

    let filter = PkjFilter {
        tahun: filled(&q, "tahun"),
        kecamatan_id: spm_sanitasi::int_or_null(&q, "kecamatan_id"),
        desa_id: spm_sanitasi::int_or_null(&q, "desa_id"),
        search: filled(&q, "search"),
        output_type,
        spm_id,
        unlinked_spm: spm_id.filter(|_| unlinked),
        only_id: None,
    };

    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let mut c = state.pool.acquire().await.map_err(internal)?;
    let total = pekerjaan_count(&mut c, &filter, user.user_id, &roles).await?;
    let all = pekerjaan_ids(&mut c, &filter, user.user_id, &roles, true).await?;

    let last_page = (total as u64).div_ceil(per_page).max(1);
    let offset = ((page - 1) * per_page) as usize;
    let page_ids: Vec<i64> = all
        .into_iter()
        .skip(offset)
        .take(per_page as usize)
        .collect();
    let pkjs = load_pekerjaan(&mut c, &page_ids).await?;
    let data: Vec<Value> = pkjs.iter().map(|p| format_pekerjaan(p, spm_id)).collect();

    Ok(Json(json!({
        "success": true,
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

/// Pesan respon attach dan detach seperti Laravel.
const ATTACH_MESSAGE: &str = "Pekerjaan berhasil ditautkan. Tahun konstruksi dan total pembiayaan diperbarui dari data pekerjaan (kontrak, atau pagu jika kontrak kosong).";
const DETACH_MESSAGE: &str = "Tautan pekerjaan berhasil dihapus. Total pembiayaan disesuaikan ulang dari pekerjaan yang masih tertaut.";

/// Respon 422 dengan bentuk `{success: false, message}` seperti `InvalidArgumentException` Laravel.
fn invalid(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "success": false, "message": message })),
    )
        .into_response()
}

/// `POST /api/spm-sanitasi/{id}/pekerjaan`: tautkan paket beserta output opsional, lalu sinkron.
pub async fn attach_pekerjaan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let spm_id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let spm = find(&state.pool, spm_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let spm_jenis = spm
        .get("jenis")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // Validasi: pekerjaan_id required|exists:tbl_pekerjaan,id; output_id nullable|exists:tbl_output,id.
    let input = parse_object(&body);
    let mut e = Errors::default();
    let pekerjaan_raw = input
        .get("pekerjaan_id")
        .filter(|v| !v.is_null() && v.as_str() != Some(""));
    let pekerjaan_id = match pekerjaan_raw {
        None => {
            e.add("pekerjaan_id", "The pekerjaan id field is required.");
            None
        }
        Some(v) => {
            let found = match json_int(v) {
                Some(pid) => {
                    let n: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_pekerjaan WHERE id = ?")
                        .bind(pid)
                        .fetch_one(&state.pool)
                        .await
                        .map_err(internal)?;
                    (n > 0).then_some(pid)
                }
                None => None,
            };
            if found.is_none() {
                e.add("pekerjaan_id", "The selected pekerjaan id is invalid.");
            }
            found
        }
    };
    let output_id = match input
        .get("output_id")
        .filter(|v| !v.is_null() && v.as_str() != Some(""))
    {
        None => None,
        Some(v) => {
            let found = match json_int(v) {
                Some(oid) => {
                    let n: i64 = sqlx::query_scalar("SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_output WHERE id = ?")
                        .bind(oid)
                        .fetch_one(&state.pool)
                        .await
                        .map_err(internal)?;
                    (n > 0).then_some(oid)
                }
                None => None,
            };
            if found.is_none() {
                e.add("output_id", "The selected output id is invalid.");
            }
            found
        }
    };
    e.finish()?;
    let pekerjaan_id = pekerjaan_id.unwrap_or_default();

    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let url = url_for(&state, &format!("/{spm_id}/pekerjaan"));
    let audit = Audit {
        headers: &headers,
        actor: user.user_id,
        url: &url,
    };
    let mut tx = state.pool.begin().await.map_err(internal)?;

    // sanitasiPekerjaanQuery(...)->where(id)->firstOrFail(): di luar scope berarti 404.
    let scope = PkjFilter {
        spm_id: Some(spm_id),
        only_id: Some(pekerjaan_id),
        ..Default::default()
    };
    let visible = pekerjaan_ids(&mut *tx, &scope, user.user_id, &roles, false).await?;
    if visible.is_empty() {
        return Ok(ApiError::not_found().into_response());
    }
    let pkj = load_pekerjaan(&mut *tx, &[pekerjaan_id])
        .await?
        .into_iter()
        .next()
        .ok_or_else(ApiError::not_found)?;

    let spm_jenis_str = spm_jenis.as_str();
    let resolved: Option<i64> = match output_id {
        Some(oid) => {
            // firstOrFail pada output milik paket ini.
            let Some(out) = pkj.outputs.iter().find(|o| o.id == oid) else {
                return Ok(ApiError::not_found().into_response());
            };
            if classify_sanitasi_komponen(&out.komponen).is_none() {
                tx.rollback().await.map_err(internal)?;
                return Ok(invalid("Output bukan komponen sanitasi yang didukung."));
            }
            let t = classify_sanitasi_komponen(&out.komponen);
            if output_matches_spm_jenis(t, spm_jenis_str) {
                Some(oid)
            } else {
                // Frontend kadang mengirim output generic; pilih output sejenis di paket yang sama.
                let fallback = first_matching_output(&pkj, spm_jenis_str);
                match fallback {
                    Some(fid) => Some(fid),
                    None => {
                        tx.rollback().await.map_err(internal)?;
                        return Ok(invalid(
                            "Output tidak sesuai jenis infrastruktur. Tangki Septik/SPALDS → SPALDS, IPAL/IPLT/SPALDT → SPALDT/IPLT, MCK → MCK.",
                        ));
                    }
                }
            }
        }
        // Tanpa output_id: output sejenis pertama dipakai sebagai default pivot, bila ada.
        None => first_matching_output(&pkj, spm_jenis_str),
    };

    // syncWithoutDetaching: tambah pivot, atau perbarui output_id dan updated_at bila sudah ada.
    sqlx::query(
        "INSERT INTO tbl_spm_sanitasi_pekerjaan (spm_sanitasi_id, pekerjaan_id, output_id, created_at, updated_at) \
         VALUES (?, ?, ?, NOW(), NOW()) \
         ON DUPLICATE KEY UPDATE output_id = VALUES(output_id), updated_at = NOW()",
    )
    .bind(spm_id)
    .bind(pekerjaan_id)
    .bind(resolved)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;

    resync_sharing(&mut tx, &audit, spm_id, pekerjaan_id).await?;
    tx.commit().await.map_err(internal)?;

    // `refresh()->load(['desa.kecamatan', 'pekerjaan.kegiatan', 'pekerjaan.output', 'pekerjaan.kontrak'])`.
    let rel = Rel {
        kegiatan: true,
        output: true,
        desa: false,
        kontrak: true,
    };
    let mut data = spm_model(&state.pool, spm_id, None)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut c = state.pool.acquire().await.map_err(internal)?;
    if let Some(obj) = data.as_object_mut() {
        obj.insert(
            "pekerjaan".into(),
            Value::Array(spm_pekerjaan_models(&mut c, spm_id, rel).await?),
        );
    }
    Ok(Json(json!({
        "success": true,
        "message": ATTACH_MESSAGE,
        "data": data,
    }))
    .into_response())
}

/// `firstMatchingOutputOnPekerjaan`: output pertama (urut id) yang klasifikasinya cocok dengan jenis SPM.
fn first_matching_output(p: &Pkj, spm_jenis: &str) -> Option<i64> {
    p.outputs
        .iter()
        .find(|o| output_matches_spm_jenis(classify_sanitasi_komponen(&o.komponen), spm_jenis))
        .map(|o| o.id)
}

/// `DELETE /api/spm-sanitasi/{id}/pekerjaan/{pekerjaanId}`: lepas tautan, lalu sinkron.
pub async fn detach_pekerjaan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, pekerjaan_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let spm_id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let pekerjaan_id: i64 = pekerjaan_id.parse().map_err(|_| ApiError::not_found())?;
    find(&state.pool, spm_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;

    let url = url_for(&state, &format!("/{spm_id}/pekerjaan/{pekerjaan_id}"));
    let audit = Audit {
        headers: &headers,
        actor: user.user_id,
        url: &url,
    };
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_spm_sanitasi_pekerjaan WHERE spm_sanitasi_id = ? AND pekerjaan_id = ?")
        .bind(spm_id)
        .bind(pekerjaan_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    resync_sharing(&mut tx, &audit, spm_id, pekerjaan_id).await?;
    tx.commit().await.map_err(internal)?;

    // Setelah `refresh()` di Laravel hanya relasi `pekerjaan` yang dimuat ulang, tanpa relasi bersarang.
    // Relasi `desa` tidak ada di respon, karena model dari route binding tidak memuatnya.
    let mut data = spm_model(&state.pool, spm_id, None)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut c = state.pool.acquire().await.map_err(internal)?;
    if let Some(obj) = data.as_object_mut() {
        obj.remove("desa");
        obj.insert(
            "pekerjaan".into(),
            Value::Array(spm_pekerjaan_models(&mut c, spm_id, Rel::default()).await?),
        );
    }
    Ok(Json(json!({
        "success": true,
        "message": DETACH_MESSAGE,
        "data": data,
    }))
    .into_response())
}
