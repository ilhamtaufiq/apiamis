//! Port `SpamPekerjaanIntegrationService` (`app/Services/SpamPekerjaanIntegrationService.php`).
//!
//! Menghubungkan paket pekerjaan air minum (`tbl_pekerjaan` dengan kegiatan sub bidang "Air Minum")
//! ke unit SPAM lewat `tbl_unit_spam_pekerjaan`, lalu menulis ulang rekam integrasi
//! (`tbl_spam_achievements` sumber = integrasi) dan anggaran (`tbl_spam_budgets` dengan `pekerjaan_id`).
//!
//! Catatan paritas:
//! - `byUserRole()` diterapkan lewat `access::restriction`. Tamu (tanpa token) mendapat `1 = 0`,
//!   seperti `scopeByUserRole()` tanpa `auth()->user()`. Header konteks app lapangan belum dibaca.
//! - Model event Laravel (audit `tbl_audit_logs` dan notifikasi admin) ditulis di transaksi yang sama
//!   untuk setiap `save()` yang benar-benar mengubah baris. Hapus massal (builder `delete`) tidak memicu
//!   event, sama seperti Laravel.
//! - Pembacaan relasi dibatasi ke kolom yang dipakai perhitungan. Bentuk JSON relasi ada di `spam_units`.

use std::collections::{BTreeSet, HashMap};

use axum::http::{HeaderMap, StatusCode};
use chrono::{Datelike, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlConnection, Row, Transaction};

use crate::{access, format::number_like_php, notify};

/// Tahun terakhir acuan master unit SPAM (import awal); tidak ditimpa integrasi.
pub const BASELINE_CAP_TAHUN: &str = "2025";
/// Integrasi / akumulasi pekerjaan dimulai dari tahun ini.
pub const ACCUMULATION_START_TAHUN: &str = "2026";
/// Catatan pada rekam achievement hasil integrasi paket.
pub const INTEGRASI_CATATAN: &str = "Akumulasi dari paket pekerjaan tertaut";

pub const MODEL_ACHIEVEMENT: &str = "App\\Models\\SpamAchievement";
pub const MODEL_BUDGET: &str = "App\\Models\\SpamBudget";
pub const MODEL_UNIT: &str = "App\\Models\\UnitSpam";
pub const SUMBER_INTEGRASI: &str = "integrasi";
pub const SUMBER_MANUAL: &str = "manual";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ---------------------------------------------------------------------------
// Helper PHP
// ---------------------------------------------------------------------------

/// `(int) $string` PHP: angka di awal string, selainnya 0.
pub fn php_int(s: &str) -> i64 {
    let t = s.trim_start();
    let bytes = t.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let digits = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits {
        return 0;
    }
    t[..i].parse::<i64>().unwrap_or(0)
}

/// `(float)` dari nilai JSON seperti PHP: angka, string angka di awal, bool, null = 0.
fn php_float(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => {
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
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}

/// `round($x, 2)` / `round($x, 1)` PHP (setengah menjauhi nol).
fn round_to(v: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    (v * f).round() / f
}

/// `$tahun` truthy di PHP: tidak kosong dan bukan "0".
pub fn truthy_tahun(t: Option<&str>) -> Option<String> {
    t.map(str::trim)
        .filter(|s| !s.is_empty() && *s != "0")
        .map(str::to_string)
}

/// `Tahun ini - 1` (zona waktu aplikasi UTC).
pub fn manual_cap_tahun() -> String {
    (Utc::now().year() - 1).to_string()
}

pub fn combined_scope_label(tahun: Option<&str>) -> String {
    match truthy_tahun(tahun) {
        Some(t) => format!("Tahun {t}"),
        None => format!(
            "Terakumulasi (acuan s/d {BASELINE_CAP_TAHUN} + integrasi {ACCUMULATION_START_TAHUN}+)"
        ),
    }
}

/// `isAccumulationTahun`: tahun integrasi (2026 ke atas). `unknown` dan kosong ditolak.
pub fn is_accumulation_tahun(tahun: &str) -> bool {
    if tahun.is_empty() || tahun == "unknown" {
        return false;
    }
    php_int(tahun) >= php_int(ACCUMULATION_START_TAHUN)
}

/// Klasifikasi komponen output air minum, `classifyAirMinumKomponen`.
pub fn classify_air_minum_komponen(komponen: &str) -> Option<&'static str> {
    let n = komponen.trim().to_lowercase();
    if n.contains("sambungan") && n.contains("rumah") {
        return Some("sambungan_rumah");
    }
    if contains_word(&n, "sr") && !n.contains("reservoir") {
        return Some("sambungan_rumah");
    }
    if n.contains("box sr") || n.contains("box sambungan") {
        return Some("sambungan_rumah");
    }
    if n.contains("pipa") || n.contains("perpipaan") || n.contains("jaringan") {
        return Some("pipa_jaringan");
    }
    if n.contains("reservoir") || n.contains("tandon") || n.contains("penampung") {
        return Some("reservoir");
    }
    if n.contains("sumur") {
        return Some("bjp");
    }
    if n.contains("mata air")
        || n.contains("intake")
        || n.contains("sumber air")
        || n.contains("pompa")
    {
        return Some("sumber_air");
    }
    None
}

/// `\bkata\b` tanpa mode Unicode (karakter non-ASCII dianggap bukan kata).
fn contains_word(s: &str, word: &str) -> bool {
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = s.as_bytes();
    let mut from = 0;
    while let Some(pos) = s[from..].find(word) {
        let start = from + pos;
        let end = start + word.len();
        let before_ok = start == 0 || !is_word(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_word(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

pub fn is_air_minum_komponen(komponen: &str) -> bool {
    classify_air_minum_komponen(komponen).is_some()
}

pub fn is_bjp_komponen(komponen: &str) -> bool {
    classify_air_minum_komponen(komponen) == Some("bjp")
}

const NON_COUNT: &[&str] = &[
    "m",
    "m1",
    "m2",
    "m3",
    "mtr",
    "meter",
    "meter persegi",
    "meter kubik",
    "km",
    "cm",
    "mm",
    "ha",
    "are",
    "ls",
    "lumpsum",
    "lump sum",
    "paket",
    "pkt",
    "set",
    "kg",
    "ton",
    "liter",
    "ltr",
    "l",
    "hari",
    "bulan",
    "minggu",
    "jam",
    "oh",
    "hok",
];

/// `OutputSatuan::isCountable`: volume boleh dibaca sebagai jumlah unit/KK.
pub fn is_countable_satuan(satuan: Option<&str>) -> bool {
    let lower = satuan.unwrap_or("").trim().to_lowercase();
    if lower.is_empty() {
        return true;
    }
    let mut replaced = String::new();
    for c in lower.chars() {
        match c {
            '²' => replaced.push('2'),
            '³' => replaced.push('3'),
            '\'' | '’' | '.' => {}
            other => replaced.push(other),
        }
    }
    // preg_replace('/\s+/', ' ')
    let mut normalized = String::new();
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
    !NON_COUNT.contains(&normalized.as_str())
}

/// Ekspresi SQL untuk `applyAirMinumOutputScope` (alias tabel output `o`).
const AIR_OUTPUT_SCOPE: &str = "(LOWER(o.komponen) LIKE '%sambungan%rumah%' \
    OR LOWER(o.komponen) REGEXP '(^|[^a-z])sr([^a-z]|$)' \
    OR LOWER(o.komponen) LIKE '%box sr%' \
    OR LOWER(o.komponen) LIKE '%pipa%' \
    OR LOWER(o.komponen) LIKE '%perpipaan%' \
    OR LOWER(o.komponen) LIKE '%jaringan%' \
    OR LOWER(o.komponen) LIKE '%reservoir%' \
    OR LOWER(o.komponen) LIKE '%tandon%' \
    OR LOWER(o.komponen) LIKE '%penampung%' \
    OR LOWER(o.komponen) LIKE '%sumur%' \
    OR LOWER(o.komponen) LIKE '%mata air%' \
    OR LOWER(o.komponen) LIKE '%intake%' \
    OR LOWER(o.komponen) LIKE '%sumber air%' \
    OR LOWER(o.komponen) LIKE '%pompa%')";

/// `applyOutputTypeSqlFilter` (alias `o`). Tipe tak dikenal menghasilkan `1 = 0`.
fn output_type_sql(output_type: &str) -> &'static str {
    match output_type {
        "sambungan_rumah" => {
            "(LOWER(o.komponen) LIKE '%sambungan%rumah%' \
            OR LOWER(o.komponen) REGEXP '(^|[^a-z])sr([^a-z]|$)' \
            OR LOWER(o.komponen) LIKE '%box sr%' \
            OR LOWER(o.komponen) LIKE '%box sambungan%')"
        }
        "pipa_jaringan" => {
            "(LOWER(o.komponen) LIKE '%pipa%' \
            OR LOWER(o.komponen) LIKE '%perpipaan%' \
            OR LOWER(o.komponen) LIKE '%jaringan%')"
        }
        "reservoir" => {
            "(LOWER(o.komponen) LIKE '%reservoir%' \
            OR LOWER(o.komponen) LIKE '%tandon%' \
            OR LOWER(o.komponen) LIKE '%penampung%')"
        }
        "bjp" => "LOWER(o.komponen) LIKE '%sumur%'",
        "sumber_air" => {
            "(LOWER(o.komponen) LIKE '%mata air%' \
            OR LOWER(o.komponen) LIKE '%intake%' \
            OR LOWER(o.komponen) LIKE '%sumber air%' \
            OR LOWER(o.komponen) LIKE '%pompa%')"
        }
        _ => "1 = 0",
    }
}

// ---------------------------------------------------------------------------
// Konteks, query terparameter
// ---------------------------------------------------------------------------

/// Konteks pemanggil: user (atau tamu) untuk `byUserRole()`, serta data audit.
pub struct Ctx<'a> {
    /// `None` = tamu (`auth()->user()` null).
    pub user: Option<u64>,
    pub roles: &'a [(u64, String)],
    pub url: &'a str,
    pub headers: &'a HeaderMap,
}

#[derive(Clone, Debug)]
pub enum B {
    S(String),
    I(i64),
    F(f64),
}

/// SQL dengan parameter berurutan.
#[derive(Default, Debug, Clone)]
pub struct Sq {
    pub sql: String,
    pub binds: Vec<B>,
}

impl Sq {
    pub fn push(&mut self, frag: &str, binds: Vec<B>) {
        self.sql.push_str(frag);
        self.binds.extend(binds);
    }
}

fn s(v: &str) -> B {
    B::S(v.to_string())
}

/// Klausa `byUserRole()` untuk tabel pekerjaan dengan alias `p`.
fn restriction(ctx: &Ctx) -> (String, Vec<B>) {
    match ctx.user {
        None => (" AND 1 = 0".to_string(), Vec::new()),
        Some(uid) => {
            let r = access::restriction(uid, ctx.roles, "p");
            (r.sql, r.binds.into_iter().map(|b| B::I(b as i64)).collect())
        }
    }
}

pub async fn fetch_rows(
    c: &mut MySqlConnection,
    sq: &Sq,
) -> Result<Vec<sqlx::mysql::MySqlRow>, ApiError> {
    let mut q = sqlx::query(&sq.sql);
    for b in &sq.binds {
        q = match b {
            B::S(v) => q.bind(v.clone()),
            B::I(v) => q.bind(*v),
            B::F(v) => q.bind(*v),
        };
    }
    q.fetch_all(c).await.map_err(internal)
}

async fn fetch_i64s(c: &mut MySqlConnection, sq: &Sq) -> Result<Vec<i64>, ApiError> {
    let rows = fetch_rows(c, sq).await?;
    rows.iter()
        .map(|r| r.try_get::<i64, _>(0).map_err(internal))
        .collect()
}

async fn fetch_scalar_i64(c: &mut MySqlConnection, sq: &Sq) -> Result<i64, ApiError> {
    let rows = fetch_rows(c, sq).await?;
    match rows.first() {
        Some(r) => r.try_get::<i64, _>(0).map_err(internal),
        None => Ok(0),
    }
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// `unit_spam.desa_id` untuk filter `unit_spam_id` di `airMinumQuery`.
async fn unit_desa(c: &mut MySqlConnection, unit_id: i64) -> Result<Option<i64>, ApiError> {
    let v: Option<i64> =
        sqlx::query_scalar("SELECT CAST(desa_id AS SIGNED) FROM tbl_unit_spam WHERE id = ?")
            .bind(unit_id)
            .fetch_optional(c)
            .await
            .map_err(internal)?;
    Ok(v)
}

/// Filter untuk `airMinumQuery`.
#[derive(Default, Clone, Debug)]
pub struct AirFilter {
    pub tahun: Option<String>,
    pub kecamatan_id: Option<i64>,
    pub desa_id: Option<i64>,
    pub search: Option<String>,
    pub output_type: Option<String>,
    pub komponen: Option<String>,
    pub unit_spam_id: Option<i64>,
    pub accumulation_only: Option<bool>,
}

/// `airMinumQuery`: SQL `SELECT <select> FROM tbl_pekerjaan p WHERE ...` (belum diurutkan).
pub async fn air_minum_sq(
    c: &mut MySqlConnection,
    ctx: &Ctx<'_>,
    f: &AirFilter,
    select: &str,
) -> Result<Sq, ApiError> {
    let tahun = truthy_tahun(f.tahun.as_deref());
    let forward =
        f.accumulation_only == Some(true) || (f.accumulation_only.is_none() && tahun.is_none());

    let mut q = Sq::default();
    q.push(&format!("SELECT {select} FROM tbl_pekerjaan p WHERE EXISTS (SELECT 1 FROM tbl_kegiatan k WHERE k.id = p.kegiatan_id AND k.sub_bidang = ?"), vec![s("Air Minum")]);
    if let Some(t) = &tahun {
        q.push(" AND k.tahun_anggaran = ?", vec![s(t)]);
    } else if forward {
        q.push(
            " AND k.tahun_anggaran >= ?",
            vec![s(ACCUMULATION_START_TAHUN)],
        );
    }
    q.push(")", vec![]);

    q.push(
        " AND EXISTS (SELECT 1 FROM tbl_output o WHERE o.pekerjaan_id = p.id",
        vec![],
    );
    if let Some(k) = &f.komponen {
        q.push(" AND o.komponen = ?", vec![s(k)]);
    } else {
        q.push(&format!(" AND {AIR_OUTPUT_SCOPE}"), vec![]);
        if let Some(t) = &f.output_type {
            q.push(&format!(" AND {}", output_type_sql(t)), vec![]);
        }
    }
    q.push(")", vec![]);

    if let Some(kec) = f.kecamatan_id {
        q.push(" AND p.kecamatan_id = ?", vec![B::I(kec)]);
    }
    if let Some(desa) = f.desa_id {
        q.push(" AND p.desa_id = ?", vec![B::I(desa)]);
    }
    if let Some(unit) = f.unit_spam_id {
        if let Some(desa) = unit_desa(c, unit).await? {
            q.push(" AND p.desa_id = ?", vec![B::I(desa)]);
        }
    }
    if let Some(search) = f.search.as_deref().filter(|v| !v.is_empty()) {
        let term = format!("%{search}%");
        q.push(
            " AND (p.nama_paket LIKE ? OR EXISTS (SELECT 1 FROM tbl_desa dq WHERE dq.id = p.desa_id AND dq.n_desa LIKE ?) \
             OR EXISTS (SELECT 1 FROM tbl_kecamatan kq WHERE kq.id = p.kecamatan_id AND kq.n_kec LIKE ?))",
            vec![s(&term), s(&term), s(&term)],
        );
    }

    let (rsql, rbinds) = restriction(ctx);
    q.push(&rsql, rbinds);
    Ok(q)
}

/// Id pekerjaan hasil `airMinumQuery`, urut id.
pub async fn air_minum_ids(
    c: &mut MySqlConnection,
    ctx: &Ctx<'_>,
    f: &AirFilter,
) -> Result<Vec<i64>, ApiError> {
    let mut q = air_minum_sq(c, ctx, f, "CAST(p.id AS SIGNED)").await?;
    q.push(" ORDER BY p.id", vec![]);
    fetch_i64s(c, &q).await
}

// ---------------------------------------------------------------------------
// Pekerjaan yang sudah dimuat
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct OutRow {
    pub id: i64,
    pub komponen: Option<String>,
    pub satuan: Option<String>,
    pub volume: f64,
}

/// Pivot `tbl_unit_spam_pekerjaan` untuk satu unit.
#[derive(Clone, Debug)]
pub struct Pivot {
    pub output_id: Option<i64>,
    pub capaian_metric: String,
}

/// Pekerjaan dengan relasi yang dipakai perhitungan integrasi.
#[derive(Clone, Debug)]
pub struct Pkj {
    pub id: i64,
    pub nama_paket: Option<String>,
    pub pagu: f64,
    pub desa_id: Option<i64>,
    pub kecamatan_id: Option<i64>,
    pub kegiatan_id: Option<i64>,
    /// `kegiatan?->tahun_anggaran`.
    pub kegiatan_tahun: Option<String>,
    /// `kegiatan?->sumber_dana`.
    pub kegiatan_sumber_dana: Option<String>,
    pub has_kegiatan: bool,
    pub outputs: Vec<OutRow>,
    pub penerima_count: i64,
    pub penerima_jiwa: i64,
    pub kontrak_sum: f64,
    pub progress: Option<Value>,
    pub foto_count: i64,
    /// `unitSpam` (urut id unit).
    pub unit_ids: Vec<i64>,
}

impl Pkj {
    /// `(string) ($pekerjaan->kegiatan?->tahun_anggaran ?? '')`.
    pub fn tahun(&self) -> String {
        self.kegiatan_tahun.clone().unwrap_or_default()
    }
}

/// Muat pekerjaan beserta relasi perhitungan, bulk per relasi. Urut id.
pub async fn load_pekerjaan(
    c: &mut MySqlConnection,
    ids: &[i64],
) -> Result<HashMap<i64, Pkj>, ApiError> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let ph = placeholders(ids.len());
    let binds: Vec<B> = ids.iter().map(|i| B::I(*i)).collect();

    let base = Sq {
        sql: format!(
            "SELECT CAST(p.id AS SIGNED) AS id, p.nama_paket, CAST(COALESCE(p.pagu, 0) AS DOUBLE) AS pagu, CAST(p.desa_id AS SIGNED) AS desa_id, \
             CAST(p.kecamatan_id AS SIGNED) AS kecamatan_id, CAST(p.kegiatan_id AS SIGNED) AS kegiatan_id, k.tahun_anggaran, k.sumber_dana, (k.id IS NOT NULL) AS has_kegiatan \
             FROM tbl_pekerjaan p LEFT JOIN tbl_kegiatan k ON k.id = p.kegiatan_id WHERE p.id IN ({ph}) ORDER BY p.id"
        ),
        binds: binds.clone(),
    };
    for r in fetch_rows(c, &base).await? {
        let id: i64 = r.try_get("id").map_err(internal)?;
        let has: i64 = r.try_get("has_kegiatan").unwrap_or(0);
        out.insert(
            id,
            Pkj {
                id,
                nama_paket: r.try_get("nama_paket").map_err(internal)?,
                pagu: r.try_get("pagu").map_err(internal)?,
                desa_id: r.try_get("desa_id").map_err(internal)?,
                kecamatan_id: r.try_get("kecamatan_id").map_err(internal)?,
                kegiatan_id: r.try_get("kegiatan_id").map_err(internal)?,
                kegiatan_tahun: r.try_get("tahun_anggaran").map_err(internal)?,
                kegiatan_sumber_dana: r.try_get("sumber_dana").map_err(internal)?,
                has_kegiatan: has != 0,
                outputs: Vec::new(),
                penerima_count: 0,
                penerima_jiwa: 0,
                kontrak_sum: 0.0,
                progress: None,
                foto_count: 0,
                unit_ids: Vec::new(),
            },
        );
    }

    let outs = Sq {
        sql: format!(
            "SELECT CAST(o.id AS SIGNED) AS id, CAST(o.pekerjaan_id AS SIGNED) AS pekerjaan_id, o.komponen, o.satuan, CAST(COALESCE(o.volume, 0) AS DOUBLE) AS volume \
             FROM tbl_output o WHERE o.pekerjaan_id IN ({ph}) ORDER BY o.id"
        ),
        binds: binds.clone(),
    };
    for r in fetch_rows(c, &outs).await? {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        if let Some(p) = out.get_mut(&pid) {
            p.outputs.push(OutRow {
                id: r.try_get("id").map_err(internal)?,
                komponen: r.try_get("komponen").map_err(internal)?,
                satuan: r.try_get("satuan").map_err(internal)?,
                volume: r.try_get("volume").map_err(internal)?,
            });
        }
    }

    let pen = Sq {
        sql: format!(
            "SELECT CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(COUNT(*) AS SIGNED) AS cnt, CAST(COALESCE(SUM(jumlah_jiwa), 0) AS SIGNED) AS jiwa \
             FROM tbl_penerima WHERE pekerjaan_id IN ({ph}) GROUP BY pekerjaan_id"
        ),
        binds: binds.clone(),
    };
    for r in fetch_rows(c, &pen).await? {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        if let Some(p) = out.get_mut(&pid) {
            p.penerima_count = r.try_get("cnt").map_err(internal)?;
            p.penerima_jiwa = r.try_get("jiwa").map_err(internal)?;
        }
    }

    let kon = Sq {
        sql: format!(
            "SELECT CAST(kp.pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(COALESCE(SUM(kt.nilai_kontrak), 0) AS DOUBLE) AS nilai \
             FROM kontrak_pekerjaan kp JOIN tbl_kontrak kt ON kt.id = kp.kontrak_id \
             WHERE kp.pekerjaan_id IN ({ph}) GROUP BY kp.pekerjaan_id"
        ),
        binds: binds.clone(),
    };
    for r in fetch_rows(c, &kon).await? {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        if let Some(p) = out.get_mut(&pid) {
            p.kontrak_sum = r.try_get("nilai").map_err(internal)?;
        }
    }

    let prog = Sq {
        sql: format!(
            "SELECT CAST(pr.pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(pr.content AS CHAR) AS content FROM tbl_progress pr \
             WHERE pr.pekerjaan_id IN ({ph}) ORDER BY pr.id"
        ),
        binds: binds.clone(),
    };
    for r in fetch_rows(c, &prog).await? {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        let content: Option<String> = r.try_get("content").map_err(internal)?;
        if let Some(p) = out.get_mut(&pid) {
            if p.progress.is_none() {
                p.progress = content.and_then(|t| serde_json::from_str::<Value>(&t).ok());
            }
        }
    }

    let foto = Sq {
        sql: format!(
            "SELECT CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(COUNT(*) AS SIGNED) AS cnt FROM tbl_foto WHERE pekerjaan_id IN ({ph}) GROUP BY pekerjaan_id"
        ),
        binds: binds.clone(),
    };
    for r in fetch_rows(c, &foto).await? {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        if let Some(p) = out.get_mut(&pid) {
            p.foto_count = r.try_get("cnt").map_err(internal)?;
        }
    }

    let units = Sq {
        sql: format!(
            "SELECT CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(unit_spam_id AS SIGNED) AS unit_spam_id FROM tbl_unit_spam_pekerjaan WHERE pekerjaan_id IN ({ph}) ORDER BY unit_spam_id"
        ),
        binds,
    };
    for r in fetch_rows(c, &units).await? {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        if let Some(p) = out.get_mut(&pid) {
            p.unit_ids
                .push(r.try_get("unit_spam_id").map_err(internal)?);
        }
    }
    Ok(out)
}

/// Pivot semua pekerjaan tertaut ke satu unit, dikunci id pekerjaan.
pub async fn unit_pivots(
    c: &mut MySqlConnection,
    unit_id: i64,
) -> Result<HashMap<i64, Pivot>, ApiError> {
    let sq = Sq {
        sql: "SELECT CAST(pekerjaan_id AS SIGNED) AS pekerjaan_id, CAST(output_id AS SIGNED) AS output_id, capaian_metric FROM tbl_unit_spam_pekerjaan WHERE unit_spam_id = ? ORDER BY pekerjaan_id"
            .to_string(),
        binds: vec![B::I(unit_id)],
    };
    let mut map = HashMap::new();
    for r in fetch_rows(c, &sq).await? {
        let pid: i64 = r.try_get("pekerjaan_id").map_err(internal)?;
        map.insert(
            pid,
            Pivot {
                output_id: r.try_get("output_id").map_err(internal)?,
                capaian_metric: r.try_get("capaian_metric").map_err(internal)?,
            },
        );
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Metrik turunan
// ---------------------------------------------------------------------------

/// `resolveCapaianMetric`.
pub fn resolve_capaian_metric(
    override_: Option<&str>,
    output: Option<&OutRow>,
    outputs: &[OutRow],
) -> &'static str {
    match override_ {
        Some("jp") => return "jp",
        Some("bjp") => return "bjp",
        _ => {}
    }
    if let Some(o) = output {
        if is_bjp_komponen(o.komponen.as_deref().unwrap_or("")) {
            return "bjp";
        }
    }
    if outputs
        .iter()
        .any(|o| is_bjp_komponen(o.komponen.as_deref().unwrap_or("")))
    {
        return "bjp";
    }
    "jp"
}

/// Baris output air minum (`airMinumOutputsForPekerjaan`).
pub struct AirOut {
    pub id: i64,
    pub komponen: Option<String>,
    pub satuan: Option<String>,
    pub volume: f64,
    pub output_type: Option<&'static str>,
    pub suggested: &'static str,
}

pub fn air_minum_outputs(p: &Pkj, include_unclassified: bool) -> Vec<AirOut> {
    p.outputs
        .iter()
        .filter(|o| {
            include_unclassified || is_air_minum_komponen(o.komponen.as_deref().unwrap_or(""))
        })
        .map(|o| {
            let output_type = classify_air_minum_komponen(o.komponen.as_deref().unwrap_or(""));
            let suggested = if output_type.is_none() || output_type == Some("bjp") {
                "bjp"
            } else {
                "jp"
            };
            AirOut {
                id: o.id,
                komponen: o.komponen.clone(),
                satuan: o.satuan.clone(),
                volume: o.volume,
                output_type,
                suggested,
            }
        })
        .collect()
}

/// `derivedMetricsForPekerjaan`. `pivot` ada bila pekerjaan dimuat lewat unit (`$unitSpam->pekerjaan`).
#[derive(Clone, Debug)]
pub struct Derived {
    pub sr: i64,
    pub kk: i64,
    pub jiwa: i64,
    pub bjp_kk: i64,
    pub bjp_jiwa: i64,
    pub capaian_metric: &'static str,
    pub nilai_kontrak: f64,
    pub progress_total: f64,
}

impl Derived {
    pub fn to_json(&self) -> Value {
        json!({
            "sr": self.sr,
            "kk": self.kk,
            "jiwa": self.jiwa,
            "bjp_kk": self.bjp_kk,
            "bjp_jiwa": self.bjp_jiwa,
            "capaian_metric": self.capaian_metric,
            "nilai_kontrak": number_like_php(self.nilai_kontrak),
            "pembiayaan_suggested": number_like_php(self.nilai_kontrak),
            "progress_total": number_like_php(self.progress_total),
        })
    }
}

/// `calculateProgressTotal` atas isi `tbl_progress.content`.
fn calculate_progress_total(content: Option<&Value>) -> f64 {
    let items = content
        .and_then(|c| c.get("items"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut total = 0.0;
    for item in &items {
        let weight = php_float(item.get("bobot"));
        let target = php_float(item.get("target_volume"));
        let mut actual = 0.0;
        if let Some(weekly) = item.get("weekly_data").and_then(Value::as_array) {
            for w in weekly {
                match w.get("realisasi") {
                    None | Some(Value::Null) => {}
                    v => actual += php_float(v),
                }
            }
        }
        let percent = if target > 0.0 {
            (actual / target) * 100.0
        } else {
            0.0
        };
        total += (percent * weight) / 100.0;
    }
    round_to(total, 2)
}

pub fn derived(p: &Pkj, pivot: Option<&Pivot>) -> Derived {
    let selected = pivot
        .and_then(|pv| pv.output_id)
        .and_then(|oid| p.outputs.iter().find(|o| o.id == oid));
    let override_ = pivot.map(|pv| pv.capaian_metric.as_str());
    let metric = resolve_capaian_metric(override_, selected, &p.outputs);

    let pembiayaan = if p.kontrak_sum > 0.0 {
        p.kontrak_sum
    } else {
        p.pagu
    };
    let progress_total = calculate_progress_total(p.progress.as_ref());
    let penerima_kk = p.penerima_count;
    let jiwa_sum = p.penerima_jiwa;

    if metric == "bjp" {
        // BJP: tidak pernah menambah SR; volume sambungan rumah/sumur dihitung sebagai KK BJP.
        let mut volume_as_kk: i64 = 0;
        for o in air_minum_outputs(p, true) {
            let vol = o.volume.round() as i64;
            if vol <= 0 {
                continue;
            }
            match o.output_type {
                Some("sambungan_rumah") | Some("bjp") => volume_as_kk += vol,
                None if is_countable_satuan(o.satuan.as_deref()) => volume_as_kk += vol,
                _ => {}
            }
        }
        let mut bjp_kk = penerima_kk;
        if bjp_kk == 0 {
            bjp_kk = volume_as_kk;
        } else if volume_as_kk > bjp_kk {
            bjp_kk = volume_as_kk;
        }
        let bjp_jiwa = if jiwa_sum > 0 { jiwa_sum } else { bjp_kk * 5 };
        return Derived {
            sr: 0,
            kk: 0,
            jiwa: 0,
            bjp_kk,
            bjp_jiwa,
            capaian_metric: "bjp",
            nilai_kontrak: pembiayaan,
            progress_total,
        };
    }

    // JP: hanya output sambungan rumah yang mengisi kolom SR.
    let mut sr: i64 = 0;
    for o in air_minum_outputs(p, false) {
        if o.output_type == Some("sambungan_rumah") {
            sr += o.volume.round() as i64;
        }
    }
    let mut kk = penerima_kk;
    if kk == 0 && sr > 0 {
        kk = sr;
    }
    let jiwa = if jiwa_sum > 0 { jiwa_sum } else { kk * 5 };
    Derived {
        sr,
        kk,
        jiwa,
        bjp_kk: 0,
        bjp_jiwa: 0,
        capaian_metric: "jp",
        nilai_kontrak: pembiayaan,
        progress_total,
    }
}

/// `aggregateDerived`: jumlah metrik semua pekerjaan dan rata-rata progres (1 desimal).
#[derive(Default, Debug, Clone)]
pub struct Agg {
    pub sr: i64,
    pub kk: i64,
    pub jiwa: i64,
    pub bjp_kk: i64,
    pub bjp_jiwa: i64,
    pub nilai_kontrak: f64,
    pub progress_avg: f64,
}

impl Agg {
    pub fn to_json(&self) -> Value {
        json!({
            "sr": self.sr,
            "kk": self.kk,
            "jiwa": self.jiwa,
            "bjp_kk": self.bjp_kk,
            "bjp_jiwa": self.bjp_jiwa,
            "nilai_kontrak": number_like_php(self.nilai_kontrak),
            "progress_avg": number_like_php(self.progress_avg),
        })
    }
}

pub fn aggregate_derived(items: &[(&Pkj, Option<&Pivot>)]) -> Agg {
    let mut a = Agg::default();
    let mut progress: Vec<f64> = Vec::new();
    for (p, pv) in items {
        let d = derived(p, *pv);
        a.sr += d.sr;
        a.kk += d.kk;
        a.jiwa += d.jiwa;
        a.bjp_kk += d.bjp_kk;
        a.bjp_jiwa += d.bjp_jiwa;
        a.nilai_kontrak += d.nilai_kontrak;
        progress.push(d.progress_total);
    }
    a.progress_avg = if progress.is_empty() {
        0.0
    } else {
        round_to(progress.iter().sum::<f64>() / progress.len() as f64, 1)
    };
    a
}

// ---------------------------------------------------------------------------
// Rekam manual (achievement dan budget) per tahun
// ---------------------------------------------------------------------------

/// Kondisi `applyAchievementBudgetTahunScope` sebagai (operator, nilai) pada kolom `tahun`.
fn tahun_conds(
    tahun: Option<&str>,
    min: Option<i64>,
    max: Option<i64>,
) -> Vec<(&'static str, String)> {
    let mut conds: Vec<(&'static str, String)> = Vec::new();
    if let Some(t) = tahun {
        conds.push(("=", t.to_string()));
    } else if min.is_some() {
        // Cakupan integrasi ke depan: batas bawah saja.
    } else if let Some(mx) = max {
        conds.push(("<=", mx.to_string()));
    } else {
        conds.push(("<=", manual_cap_tahun()));
    }
    if let Some(mn) = min {
        conds.push((">=", mn.to_string()));
    }
    if let (Some(mx), Some(_)) = (max, min) {
        conds.push(("<=", mx.to_string()));
    }
    conds
}

fn push_tahun(q: &mut Sq, col: &str, conds: Vec<(&'static str, String)>) {
    for (op, v) in conds {
        q.push(&format!(" AND {col} {op} ?"), vec![s(&v)]);
    }
}

/// `aggregateManualForDesa`.
pub async fn aggregate_manual_for_desa(
    c: &mut MySqlConnection,
    desa_id: i64,
    tahun: Option<&str>,
    unit_ids: &[i64],
    min: Option<i64>,
    max: Option<i64>,
) -> Result<Manual, ApiError> {
    let _ = desa_id;
    if unit_ids.is_empty() {
        return Ok(Manual::default());
    }
    let ph = placeholders(unit_ids.len());
    let ids: Vec<B> = unit_ids.iter().map(|i| B::I(*i)).collect();
    let conds = tahun_conds(tahun, min, max);

    let mut ach = Sq {
        sql: format!(
            "SELECT CAST(COALESCE(SUM(a.jumlah_sr),0) AS SIGNED), CAST(COALESCE(SUM(a.jumlah_kk),0) AS SIGNED), \
             CAST(COALESCE(SUM(a.jumlah_jiwa),0) AS SIGNED) FROM tbl_spam_achievements a WHERE a.unit_spam_id IN ({ph})"
        ),
        binds: ids.clone(),
    };
    push_tahun(&mut ach, "a.tahun", conds.clone());
    let (sr, kk, jiwa) = fetch_three(c, &ach).await?;

    let mut bud = Sq {
        sql: format!("SELECT CAST(COALESCE(SUM(b.nilai_kontrak),0) AS DOUBLE) FROM tbl_spam_budgets b WHERE b.unit_spam_id IN ({ph})"),
        binds: ids,
    };
    push_tahun(&mut bud, "b.tahun", conds);
    let nilai = fetch_f64(c, &bud).await?;

    Ok(Manual {
        sr,
        kk,
        jiwa,
        nilai,
    })
}

#[derive(Default, Debug, Clone, Copy)]
pub struct Manual {
    pub sr: i64,
    pub kk: i64,
    pub jiwa: i64,
    pub nilai: f64,
}

impl Manual {
    pub fn to_json(self) -> Value {
        json!({
            "sr": self.sr,
            "kk": self.kk,
            "jiwa": self.jiwa,
            "nilai_kontrak": number_like_php(self.nilai),
        })
    }
}

async fn fetch_three(c: &mut MySqlConnection, sq: &Sq) -> Result<(i64, i64, i64), ApiError> {
    let rows = fetch_rows(c, sq).await?;
    let Some(r) = rows.first() else {
        return Ok((0, 0, 0));
    };
    Ok((
        r.try_get::<i64, _>(0).map_err(internal)?,
        r.try_get::<i64, _>(1).map_err(internal)?,
        r.try_get::<i64, _>(2).map_err(internal)?,
    ))
}

async fn fetch_f64(c: &mut MySqlConnection, sq: &Sq) -> Result<f64, ApiError> {
    let rows = fetch_rows(c, sq).await?;
    match rows.first() {
        Some(r) => r.try_get::<f64, _>(0).map_err(internal),
        None => Ok(0.0),
    }
}

/// `aggregateManualGlobal`, opsional dibatasi kecamatan.
pub async fn aggregate_manual_global(
    c: &mut MySqlConnection,
    tahun: Option<&str>,
    kecamatan_id: Option<i64>,
    min: Option<i64>,
    max: Option<i64>,
) -> Result<Manual, ApiError> {
    let conds = tahun_conds(tahun, min, max);
    let kec_ach = " AND EXISTS (SELECT 1 FROM tbl_unit_spam u JOIN tbl_desa d ON d.id = u.desa_id WHERE u.id = a.unit_spam_id AND d.kecamatan_id = ?)";
    let kec_bud = " AND EXISTS (SELECT 1 FROM tbl_unit_spam u JOIN tbl_desa d ON d.id = u.desa_id WHERE u.id = b.unit_spam_id AND d.kecamatan_id = ?)";

    let mut ach = Sq {
        sql: "SELECT CAST(COALESCE(SUM(a.jumlah_sr),0) AS SIGNED), CAST(COALESCE(SUM(a.jumlah_kk),0) AS SIGNED), \
              CAST(COALESCE(SUM(a.jumlah_jiwa),0) AS SIGNED) FROM tbl_spam_achievements a WHERE 1 = 1"
            .to_string(),
        binds: vec![],
    };
    push_tahun(&mut ach, "a.tahun", conds.clone());
    if let Some(kec) = kecamatan_id {
        ach.push(kec_ach, vec![B::I(kec)]);
    }
    let (sr, kk, jiwa) = fetch_three(c, &ach).await?;

    let mut bud = Sq {
        sql: "SELECT CAST(COALESCE(SUM(b.nilai_kontrak),0) AS DOUBLE) FROM tbl_spam_budgets b WHERE 1 = 1".to_string(),
        binds: vec![],
    };
    push_tahun(&mut bud, "b.tahun", conds);
    if let Some(kec) = kecamatan_id {
        bud.push(kec_bud, vec![B::I(kec)]);
    }
    let nilai = fetch_f64(c, &bud).await?;
    Ok(Manual {
        sr,
        kk,
        jiwa,
        nilai,
    })
}

// ---------------------------------------------------------------------------
// Baris desa dan integrasi
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct DesaInfo {
    pub id: i64,
    pub n_desa: Option<String>,
    pub target: i64,
    pub bjp_master: i64,
    pub kecamatan_id: Option<i64>,
    pub n_kec: Option<String>,
}

fn sync_status(unit_count: usize, pekerjaan_count: usize, linked: usize) -> &'static str {
    if unit_count == 0 && pekerjaan_count == 0 {
        return "no_data";
    }
    if unit_count == 0 {
        return "no_unit";
    }
    if pekerjaan_count == 0 {
        return "no_pekerjaan";
    }
    if linked >= pekerjaan_count {
        "matched"
    } else {
        "partial"
    }
}

/// `formatAirMinumPekerjaan`.
pub fn format_air_minum_pekerjaan(p: &Pkj, linked_unit_id: Option<i64>) -> Value {
    let metrics = derived(p, None);
    let outputs = air_minum_outputs(p, false);
    let is_linked = match linked_unit_id {
        Some(u) => p.unit_ids.contains(&u),
        None => !p.unit_ids.is_empty(),
    };
    let mut output_types: Vec<&str> = Vec::new();
    for o in &outputs {
        if let Some(t) = o.output_type {
            if !output_types.contains(&t) {
                output_types.push(t);
            }
        }
    }
    let outputs_json: Vec<Value> = outputs
        .iter()
        .map(|o| {
            json!({
                "id": o.id,
                "komponen": o.komponen,
                "satuan": o.satuan,
                "volume": number_like_php(o.volume),
                "output_type": o.output_type,
                "suggested_capaian_metric": o.suggested,
            })
        })
        .collect();
    json!({
        "id": p.id,
        "nama_paket": p.nama_paket,
        "pagu": number_like_php(p.pagu),
        "tahun_anggaran": p.tahun(),
        "sumber_dana": p.kegiatan_sumber_dana.clone().unwrap_or_default(),
        "progress_total": number_like_php(metrics.progress_total),
        "nilai_kontrak": number_like_php(metrics.nilai_kontrak),
        "sr": metrics.sr,
        "kk": metrics.kk,
        "jiwa": metrics.jiwa,
        "bjp_kk": metrics.bjp_kk,
        "bjp_jiwa": metrics.bjp_jiwa,
        "capaian_metric": metrics.capaian_metric,
        "penerima_count": p.penerima_count,
        "foto_count": p.foto_count,
        "air_minum_outputs": outputs_json,
        "output_types": output_types,
        "derived": metrics.to_json(),
        "is_linked": is_linked,
        "linked_unit_ids": p.unit_ids,
    })
}

/// `buildDesaIntegrationRow`.
pub async fn desa_integration_row(
    c: &mut MySqlConnection,
    ctx: &Ctx<'_>,
    desa: &DesaInfo,
    tahun: Option<&str>,
    output_type: Option<&str>,
    komponen: Option<&str>,
) -> Result<Value, ApiError> {
    let units = unit_rows_for_desa(c, desa.id).await?;
    let unit_ids: Vec<i64> = units.iter().map(|u| u.id).collect();

    let filter = AirFilter {
        tahun: tahun.map(str::to_string),
        desa_id: Some(desa.id),
        output_type: output_type.map(str::to_string),
        komponen: komponen.map(str::to_string),
        accumulation_only: Some(true),
        ..Default::default()
    };
    let ids = air_minum_ids(c, ctx, &filter).await?;
    let loaded = load_pekerjaan(c, &ids).await?;
    let pekerjaan: Vec<&Pkj> = ids.iter().filter_map(|id| loaded.get(id)).collect();

    let items: Vec<(&Pkj, Option<&Pivot>)> = pekerjaan.iter().map(|p| (*p, None)).collect();
    let agg = aggregate_derived(&items);
    let manual = aggregate_manual_for_desa(c, desa.id, tahun, &unit_ids, None, None).await?;
    let manual_integrasi = aggregate_manual_for_desa(
        c,
        desa.id,
        tahun,
        &unit_ids,
        Some(php_int(ACCUMULATION_START_TAHUN)),
        None,
    )
    .await?;
    let linked_count = pekerjaan.iter().filter(|p| !p.unit_ids.is_empty()).count();

    let formatted: Vec<Value> = pekerjaan
        .iter()
        .map(|p| format_air_minum_pekerjaan(p, None))
        .collect();
    let mut output_types: Vec<String> = Vec::new();
    for f in &formatted {
        if let Some(arr) = f.get("output_types").and_then(Value::as_array) {
            for t in arr.iter().filter_map(Value::as_str) {
                if !output_types.iter().any(|x| x == t) {
                    output_types.push(t.to_string());
                }
            }
        }
    }

    let units_json: Vec<Value> = units
        .iter()
        .map(|u| {
            json!({
                "id": u.id,
                "name": u.name,
                "is_simspam": u.is_simspam,
                "sistem_layanan": u.sistem_layanan,
                "pokmas": u.pokmas,
                "kepala": u.kepala,
                "linked_pekerjaan_count": u.linked_count,
            })
        })
        .collect();

    Ok(json!({
        "desa": {
            "id": desa.id,
            "n_desa": desa.n_desa,
            "target": desa.target,
            "bjp_master": desa.bjp_master,
            "kecamatan": { "id": desa.kecamatan_id, "n_kec": desa.n_kec },
        },
        "units": units_json,
        "unit_count": units.len(),
        "pekerjaan_count": pekerjaan.len(),
        "linked_count": linked_count,
        "pekerjaan": formatted,
        "output_types": output_types,
        "output_type_filter": output_type,
        "derived": agg.to_json(),
        "manual": manual.to_json(),
        "manual_integrasi": manual_integrasi.to_json(),
        "baseline_cap_tahun": BASELINE_CAP_TAHUN,
        "accumulation_start_tahun": ACCUMULATION_START_TAHUN,
        "sync_status": sync_status(units.len(), pekerjaan.len(), linked_count),
    }))
}

/// Unit satu desa dengan pengelola dan jumlah pekerjaan tertaut (`$unit->pekerjaan->count()`).
pub struct UnitBrief {
    pub id: i64,
    pub name: Option<String>,
    pub is_simspam: bool,
    pub sistem_layanan: Option<String>,
    pub pokmas: Option<String>,
    pub kepala: Option<String>,
    pub linked_count: i64,
}

async fn unit_rows_for_desa(
    c: &mut MySqlConnection,
    desa_id: i64,
) -> Result<Vec<UnitBrief>, ApiError> {
    let sq = Sq {
        sql: "SELECT CAST(u.id AS SIGNED) AS id, u.name, u.is_simspam, u.sistem_layanan, pg.pokmas, pg.kepala, \
              CAST((SELECT COUNT(*) FROM tbl_unit_spam_pekerjaan up WHERE up.unit_spam_id = u.id) AS SIGNED) AS linked_count \
              FROM tbl_unit_spam u LEFT JOIN tbl_pengelola pg ON pg.unit_spam_id = u.id WHERE u.desa_id = ? ORDER BY u.id"
            .to_string(),
        binds: vec![B::I(desa_id)],
    };
    let mut out = Vec::new();
    for r in fetch_rows(c, &sq).await? {
        let flag: i64 = r.try_get("is_simspam").map_err(internal)?;
        out.push(UnitBrief {
            id: r.try_get("id").map_err(internal)?,
            name: r.try_get("name").map_err(internal)?,
            is_simspam: flag != 0,
            sistem_layanan: r.try_get("sistem_layanan").map_err(internal)?,
            pokmas: r.try_get("pokmas").map_err(internal)?,
            kepala: r.try_get("kepala").map_err(internal)?,
            linked_count: r.try_get("linked_count").map_err(internal)?,
        });
    }
    Ok(out)
}

pub async fn desa_info(
    c: &mut MySqlConnection,
    desa_id: i64,
) -> Result<Option<DesaInfo>, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(d.id AS SIGNED) AS id, d.n_desa, CAST(COALESCE(d.target,0) AS SIGNED) AS target, CAST(COALESCE(d.bjp_master,0) AS SIGNED) AS bjp_master, \
         CAST(d.kecamatan_id AS SIGNED) AS kecamatan_id, k.n_kec FROM tbl_desa d LEFT JOIN tbl_kecamatan k ON k.id = d.kecamatan_id WHERE d.id = ?",
    )
    .bind(desa_id)
    .fetch_optional(c)
    .await
    .map_err(internal)?;
    match row {
        None => Ok(None),
        Some(r) => Ok(Some(DesaInfo {
            id: r.try_get("id").map_err(internal)?,
            n_desa: r.try_get("n_desa").map_err(internal)?,
            target: r.try_get("target").map_err(internal)?,
            bjp_master: r.try_get("bjp_master").map_err(internal)?,
            kecamatan_id: r.try_get("kecamatan_id").map_err(internal)?,
            n_kec: r.try_get("n_kec").map_err(internal)?,
        })),
    }
}

/// `integrationDesaQuery` lalu daftar desa terurut `n_desa`.
#[allow(clippy::too_many_arguments)]
pub async fn integration_desas(
    c: &mut MySqlConnection,
    ctx: &Ctx<'_>,
    tahun: Option<&str>,
    kecamatan_id: Option<i64>,
    desa_id: Option<i64>,
    search: Option<&str>,
    output_type: Option<&str>,
    komponen: Option<&str>,
) -> Result<Vec<DesaInfo>, ApiError> {
    // Desa dari unit SPAM.
    let mut from_units = Sq {
        sql: "SELECT DISTINCT CAST(u.desa_id AS SIGNED) FROM tbl_unit_spam u WHERE 1 = 1"
            .to_string(),
        binds: vec![],
    };
    if let Some(kec) = kecamatan_id {
        from_units.push(
            " AND EXISTS (SELECT 1 FROM tbl_desa d WHERE d.id = u.desa_id AND d.kecamatan_id = ?)",
            vec![B::I(kec)],
        );
    }
    if let Some(desa) = desa_id {
        from_units.push(" AND u.desa_id = ?", vec![B::I(desa)]);
    }
    let mut ids: BTreeSet<i64> = fetch_i64s(c, &from_units).await?.into_iter().collect();

    // Desa dari paket air minum (guna byUserRole).
    let filter = AirFilter {
        tahun: tahun.map(str::to_string),
        kecamatan_id,
        desa_id,
        search: search.map(str::to_string),
        output_type: output_type.map(str::to_string),
        komponen: komponen.map(str::to_string),
        unit_spam_id: None,
        accumulation_only: Some(true),
    };
    let pk_ids = air_minum_ids(c, ctx, &filter).await?;
    let loaded = load_pekerjaan(c, &pk_ids).await?;
    for p in loaded.values() {
        if let Some(d) = p.desa_id {
            ids.insert(d);
        }
    }
    ids.retain(|v| *v != 0);

    let id_list: Vec<i64> = if ids.is_empty() {
        vec![-1]
    } else {
        ids.into_iter().collect()
    };
    let mut q = Sq {
        sql: "SELECT CAST(d.id AS SIGNED) AS id, d.n_desa, CAST(COALESCE(d.target,0) AS SIGNED) AS target, CAST(COALESCE(d.bjp_master,0) AS SIGNED) AS bjp_master, \
              CAST(d.kecamatan_id AS SIGNED) AS kecamatan_id, k.n_kec FROM tbl_desa d LEFT JOIN tbl_kecamatan k ON k.id = d.kecamatan_id WHERE d.id IN (".to_string(),
        binds: id_list.iter().map(|i| B::I(*i)).collect(),
    };
    q.sql.push_str(&placeholders(id_list.len()));
    q.sql.push(')');
    if let Some(kec) = kecamatan_id {
        q.push(" AND d.kecamatan_id = ?", vec![B::I(kec)]);
    }
    if let Some(desa) = desa_id {
        q.push(" AND d.id = ?", vec![B::I(desa)]);
    }
    if let Some(term) = search.filter(|v| !v.is_empty()).map(|v| format!("%{v}%")) {
        let unit_desas = {
            let sq = Sq {
                sql: "SELECT DISTINCT CAST(desa_id AS SIGNED) FROM tbl_unit_spam WHERE name LIKE ?"
                    .to_string(),
                binds: vec![s(&term)],
            };
            fetch_i64s(c, &sq).await?
        };
        q.push(
            " AND (d.n_desa LIKE ? OR EXISTS (SELECT 1 FROM tbl_kecamatan kq WHERE kq.id = d.kecamatan_id AND kq.n_kec LIKE ?)",
            vec![s(&term), s(&term)],
        );
        if !unit_desas.is_empty() {
            q.push(
                &format!(" OR d.id IN ({})", placeholders(unit_desas.len())),
                unit_desas.iter().map(|i| B::I(*i)).collect(),
            );
        }
        q.push(")", vec![]);
        // Pengikatan kedua untuk LIKE kecamatan sudah termasuk di atas.
        let _ = &term;
    }
    q.push(" ORDER BY d.n_desa, d.id", vec![]);

    let mut out = Vec::new();
    for r in fetch_rows(c, &q).await? {
        out.push(DesaInfo {
            id: r.try_get("id").map_err(internal)?,
            n_desa: r.try_get("n_desa").map_err(internal)?,
            target: r.try_get("target").map_err(internal)?,
            bjp_master: r.try_get("bjp_master").map_err(internal)?,
            kecamatan_id: r.try_get("kecamatan_id").map_err(internal)?,
            n_kec: r.try_get("n_kec").map_err(internal)?,
        });
    }
    Ok(out)
}

/// `summarizeRows`.
pub fn summarize_rows(rows: &[Value]) -> Value {
    let mut matched = 0;
    let mut partial = 0;
    let mut no_unit = 0;
    let mut no_pekerjaan = 0;
    let mut total_pekerjaan = 0i64;
    let mut total_units = 0i64;
    let mut total_linked = 0i64;
    for r in rows {
        match r.get("sync_status").and_then(Value::as_str) {
            Some("matched") => matched += 1,
            Some("partial") => partial += 1,
            Some("no_unit") => no_unit += 1,
            Some("no_pekerjaan") => no_pekerjaan += 1,
            _ => {}
        }
        total_pekerjaan += r
            .get("pekerjaan_count")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        total_units += r.get("unit_count").and_then(Value::as_i64).unwrap_or(0);
        total_linked += r.get("linked_count").and_then(Value::as_i64).unwrap_or(0);
    }
    json!({
        "total_desa": rows.len(),
        "matched_count": matched,
        "partial_count": partial,
        "no_unit_count": no_unit,
        "no_pekerjaan_count": no_pekerjaan,
        "total_pekerjaan": total_pekerjaan,
        "total_units": total_units,
        "total_linked": total_linked,
    })
}

/// `paginateIntegration`. Mengembalikan `(data, meta, summary)`.
#[allow(clippy::too_many_arguments)]
pub async fn paginate_integration(
    c: &mut MySqlConnection,
    ctx: &Ctx<'_>,
    tahun: Option<&str>,
    kecamatan_id: Option<i64>,
    desa_id: Option<i64>,
    search: Option<&str>,
    sync_status_filter: Option<&str>,
    output_type: Option<&str>,
    komponen: Option<&str>,
    per_page: i64,
    page: i64,
) -> Result<(Vec<Value>, Value, Value), ApiError> {
    let desas = integration_desas(
        c,
        ctx,
        tahun,
        kecamatan_id,
        desa_id,
        search,
        output_type,
        komponen,
    )
    .await?;
    let mut rows = Vec::with_capacity(desas.len());
    for d in &desas {
        rows.push(desa_integration_row(c, ctx, d, tahun, output_type, komponen).await?);
    }
    if let Some(st) = sync_status_filter {
        rows.retain(|r| r.get("sync_status").and_then(Value::as_str) == Some(st));
    }
    let per_page = per_page.max(1);
    let total = rows.len() as i64;
    let last_page = std::cmp::max(1, (total + per_page - 1) / per_page);
    let page = page.clamp(1, last_page);
    let offset = ((page - 1) * per_page) as usize;
    let data: Vec<Value> = rows
        .iter()
        .skip(offset)
        .take(per_page as usize)
        .cloned()
        .collect();
    let meta = json!({ "current_page": page, "last_page": last_page, "per_page": per_page, "total": total });
    Ok((data, meta, summarize_rows(&rows)))
}

/// `paginateAirMinumPekerjaan`. Mengembalikan `(data, meta)`.
#[allow(clippy::too_many_arguments)]
pub async fn paginate_air_minum_pekerjaan(
    c: &mut MySqlConnection,
    ctx: &Ctx<'_>,
    tahun: Option<&str>,
    kecamatan_id: Option<i64>,
    desa_id: Option<i64>,
    search: Option<&str>,
    output_type: Option<&str>,
    unit_spam_id: Option<i64>,
    unlinked_only: bool,
    per_page: i64,
    page: i64,
) -> Result<(Vec<Value>, Value), ApiError> {
    let filter = AirFilter {
        tahun: tahun.map(str::to_string),
        kecamatan_id,
        desa_id,
        search: search.map(str::to_string),
        output_type: output_type.map(str::to_string),
        komponen: None,
        unit_spam_id,
        accumulation_only: Some(true),
    };
    let mut q = air_minum_sq(c, ctx, &filter, "CAST(p.id AS SIGNED)").await?;
    if unlinked_only {
        if let Some(unit) = unit_spam_id {
            q.push(
                " AND NOT EXISTS (SELECT 1 FROM tbl_unit_spam_pekerjaan up WHERE up.pekerjaan_id = p.id AND up.unit_spam_id = ?)",
                vec![B::I(unit)],
            );
        }
    }
    q.push(" ORDER BY p.id DESC", vec![]);
    let ids = fetch_i64s(c, &q).await?;
    let per_page = per_page.max(1);
    let total = ids.len() as i64;
    let last_page = std::cmp::max(1, (total + per_page - 1) / per_page);
    let page = page.clamp(1, last_page);
    let offset = ((page - 1) * per_page) as usize;
    let page_ids: Vec<i64> = ids
        .iter()
        .skip(offset)
        .take(per_page as usize)
        .copied()
        .collect();
    let loaded = load_pekerjaan(c, &page_ids).await?;
    let data: Vec<Value> = page_ids
        .iter()
        .filter_map(|id| loaded.get(id))
        .map(|p| format_air_minum_pekerjaan(p, unit_spam_id))
        .collect();
    let meta = json!({ "current_page": page, "last_page": last_page, "per_page": per_page, "total": total });
    Ok((data, meta))
}

/// `listIntegrationOutputOptions`.
pub async fn output_options(
    c: &mut MySqlConnection,
    ctx: &Ctx<'_>,
    tahun: Option<&str>,
    kecamatan_id: Option<i64>,
) -> Result<Vec<Value>, ApiError> {
    let tahun = truthy_tahun(tahun);
    let mut q = Sq {
        sql: "SELECT COALESCE(o.komponen, '') AS komponen, CAST(COUNT(DISTINCT o.pekerjaan_id) AS SIGNED) AS cnt \
              FROM tbl_output o WHERE EXISTS (SELECT 1 FROM tbl_pekerjaan p JOIN tbl_kegiatan k ON k.id = p.kegiatan_id \
              WHERE p.id = o.pekerjaan_id AND k.sub_bidang = ?"
            .to_string(),
        binds: vec![s("Air Minum")],
    };
    if let Some(t) = &tahun {
        q.push(" AND k.tahun_anggaran = ?", vec![s(t)]);
    } else {
        q.push(
            " AND k.tahun_anggaran >= ?",
            vec![s(ACCUMULATION_START_TAHUN)],
        );
    }
    let (rsql, rbinds) = restriction(ctx);
    q.push(&rsql, rbinds);
    if let Some(kec) = kecamatan_id {
        q.push(" AND p.kecamatan_id = ?", vec![B::I(kec)]);
    }
    q.push(
        ") GROUP BY COALESCE(o.komponen, '') ORDER BY komponen",
        vec![],
    );

    let mut out = Vec::new();
    for r in fetch_rows(c, &q).await? {
        let komponen: String = r.try_get("komponen").map_err(internal)?;
        let cnt: i64 = r.try_get("cnt").map_err(internal)?;
        let ot = classify_air_minum_komponen(&komponen);
        out.push(json!({
            "komponen": komponen,
            "output_type": ot,
            "is_integrasi": is_air_minum_komponen(&komponen),
            "pekerjaan_count": cnt,
            "label": komponen,
        }));
    }
    Ok(out)
}

/// `countLinkedUnits`.
pub async fn count_linked_units(
    c: &mut MySqlConnection,
    kecamatan_id: Option<i64>,
) -> Result<i64, ApiError> {
    let mut q = Sq {
        sql: "SELECT CAST(COUNT(*) AS SIGNED) FROM tbl_unit_spam u WHERE EXISTS (SELECT 1 FROM tbl_unit_spam_pekerjaan up WHERE up.unit_spam_id = u.id)"
            .to_string(),
        binds: vec![],
    };
    if let Some(kec) = kecamatan_id {
        q.push(
            " AND EXISTS (SELECT 1 FROM tbl_desa d WHERE d.id = u.desa_id AND d.kecamatan_id = ?)",
            vec![B::I(kec)],
        );
    }
    fetch_scalar_i64(c, &q).await
}

/// `buildStatsEnrichment`. Mengembalikan objek dengan kunci persis seperti PHP.
pub async fn stats_enrichment(
    c: &mut MySqlConnection,
    ctx: &Ctx<'_>,
    tahun: Option<&str>,
    kecamatan_id: Option<i64>,
) -> Result<Map<String, Value>, ApiError> {
    let tahun = truthy_tahun(tahun);
    let start = php_int(ACCUMULATION_START_TAHUN);
    let base = php_int(BASELINE_CAP_TAHUN);
    let integrasi = aggregate_manual_global(c, None, kecamatan_id, Some(start), None).await?;
    let baseline = aggregate_manual_global(c, None, kecamatan_id, None, Some(base)).await?;
    let capaian = if let Some(t) = &tahun {
        aggregate_manual_global(c, Some(t), kecamatan_id, None, None).await?
    } else {
        Manual {
            sr: baseline.sr + integrasi.sr,
            kk: baseline.kk + integrasi.kk,
            jiwa: baseline.jiwa + integrasi.jiwa,
            nilai: baseline.nilai + integrasi.nilai,
        }
    };

    let potensi_filter = AirFilter {
        tahun: tahun.clone(),
        kecamatan_id,
        accumulation_only: Some(true),
        ..Default::default()
    };
    let potensi_ids = air_minum_ids(c, ctx, &potensi_filter).await?;
    let potensi_loaded = load_pekerjaan(c, &potensi_ids).await?;
    let potensi_items: Vec<(&Pkj, Option<&Pivot>)> = potensi_ids
        .iter()
        .filter_map(|id| potensi_loaded.get(id))
        .map(|p| (p, None))
        .collect();
    let potensi = aggregate_derived(&potensi_items);

    let linked_ids = linked_pekerjaan_ids(c, ctx, tahun.as_deref(), kecamatan_id).await?;
    let linked_loaded = load_pekerjaan(c, &linked_ids).await?;
    let linked_items: Vec<(&Pkj, Option<&Pivot>)> = linked_ids
        .iter()
        .filter_map(|id| linked_loaded.get(id))
        .map(|p| (p, None))
        .collect();
    let linked = aggregate_derived(&linked_items);

    let linked_count = linked_ids.len() as i64;
    let potensi_count = potensi_ids.len() as i64;
    let units_linked = count_linked_units(c, kecamatan_id).await?;

    let mut m = Map::new();
    m.insert("linked_pekerjaan_count".into(), json!(linked_count));
    m.insert("linked_units_count".into(), json!(units_linked));
    m.insert(
        "paket_belum_tertaut".into(),
        json!((potensi_count - linked_count).max(0)),
    );
    m.insert("linked_sr".into(), json!(linked.sr));
    m.insert("linked_kk".into(), json!(linked.kk));
    m.insert("linked_jiwa".into(), json!(linked.jiwa));
    m.insert(
        "linked_nilai_kontrak".into(),
        number_like_php(linked.nilai_kontrak),
    );
    m.insert("baseline_cap_tahun".into(), json!(BASELINE_CAP_TAHUN));
    m.insert(
        "accumulation_start_tahun".into(),
        json!(ACCUMULATION_START_TAHUN),
    );
    m.insert("capaian_sr".into(), json!(capaian.sr));
    m.insert("capaian_kk".into(), json!(capaian.kk));
    m.insert("capaian_jiwa".into(), json!(capaian.jiwa));
    m.insert(
        "capaian_nilai_kontrak".into(),
        number_like_php(capaian.nilai),
    );
    m.insert("capaian_baseline_sr".into(), json!(baseline.sr));
    m.insert("capaian_baseline_kk".into(), json!(baseline.kk));
    m.insert("capaian_baseline_jiwa".into(), json!(baseline.jiwa));
    m.insert(
        "capaian_baseline_nilai_kontrak".into(),
        number_like_php(baseline.nilai),
    );
    m.insert("capaian_integrasi_sr".into(), json!(integrasi.sr));
    m.insert("capaian_integrasi_kk".into(), json!(integrasi.kk));
    m.insert("capaian_integrasi_jiwa".into(), json!(integrasi.jiwa));
    m.insert(
        "capaian_integrasi_nilai_kontrak".into(),
        number_like_php(integrasi.nilai),
    );
    m.insert("potensi_sr".into(), json!(potensi.sr));
    m.insert("potensi_kk".into(), json!(potensi.kk));
    m.insert("potensi_jiwa".into(), json!(potensi.jiwa));
    m.insert(
        "potensi_nilai_kontrak".into(),
        number_like_php(potensi.nilai_kontrak),
    );
    m.insert("selisih_sr".into(), json!(potensi.sr - integrasi.sr));
    m.insert("selisih_kk".into(), json!(potensi.kk - integrasi.kk));
    m.insert("selisih_jiwa".into(), json!(potensi.jiwa - integrasi.jiwa));
    m.insert(
        "selisih_nilai_kontrak".into(),
        number_like_php(potensi.nilai_kontrak - integrasi.nilai),
    );
    Ok(m)
}

/// `linkedPekerjaanQuery` (hanya id): paket air minum yang punya tautan unit.
async fn linked_pekerjaan_ids(
    c: &mut MySqlConnection,
    ctx: &Ctx<'_>,
    tahun: Option<&str>,
    kecamatan_id: Option<i64>,
) -> Result<Vec<i64>, ApiError> {
    let mut q = Sq {
        sql: "SELECT CAST(p.id AS SIGNED) FROM tbl_pekerjaan p WHERE EXISTS (SELECT 1 FROM tbl_unit_spam_pekerjaan up WHERE up.pekerjaan_id = p.id) \
              AND EXISTS (SELECT 1 FROM tbl_kegiatan k WHERE k.id = p.kegiatan_id AND k.sub_bidang = ?"
            .to_string(),
        binds: vec![s("Air Minum")],
    };
    match truthy_tahun(tahun) {
        Some(t) => q.push(" AND k.tahun_anggaran = ?", vec![s(&t)]),
        None => q.push(
            " AND k.tahun_anggaran >= ?",
            vec![s(ACCUMULATION_START_TAHUN)],
        ),
    }
    q.push(")", vec![]);
    if let Some(kec) = kecamatan_id {
        q.push(" AND p.kecamatan_id = ?", vec![B::I(kec)]);
    }
    let (rsql, rbinds) = restriction(ctx);
    q.push(&rsql, rbinds);
    q.push(" ORDER BY p.id", vec![]);
    fetch_i64s(c, &q).await
}

/// `integrationSummary`.
pub async fn integration_summary(
    c: &mut MySqlConnection,
    ctx: &Ctx<'_>,
    tahun: Option<&str>,
    kecamatan_id: Option<i64>,
    desa_id: Option<i64>,
) -> Result<Map<String, Value>, ApiError> {
    let desas = integration_desas(c, ctx, tahun, kecamatan_id, desa_id, None, None, None).await?;
    let mut rows = Vec::with_capacity(desas.len());
    for d in &desas {
        rows.push(desa_integration_row(c, ctx, d, tahun, None, None).await?);
    }
    let summary = summarize_rows(&rows);

    let mut sr = 0i64;
    let mut kk = 0i64;
    let mut jiwa = 0i64;
    let mut nilai = 0.0f64;
    for r in &rows {
        let d = &r["derived"];
        sr += d["sr"].as_i64().unwrap_or(0);
        kk += d["kk"].as_i64().unwrap_or(0);
        jiwa += d["jiwa"].as_i64().unwrap_or(0);
        nilai += d["nilai_kontrak"].as_f64().unwrap_or(0.0);
    }
    let manual = aggregate_manual_global(c, tahun, kecamatan_id, None, None).await?;
    let count_filter = AirFilter {
        tahun: tahun.map(str::to_string),
        kecamatan_id,
        desa_id,
        accumulation_only: Some(true),
        ..Default::default()
    };
    let mut cq = air_minum_sq(c, ctx, &count_filter, "COUNT(*)").await?;
    cq.sql = cq.sql.replacen(
        "SELECT COUNT(*) FROM",
        "SELECT CAST(COUNT(*) AS SIGNED) FROM",
        1,
    );
    let air_count = fetch_scalar_i64(c, &cq).await?;

    let mut out = Map::new();
    if let Value::Object(map) = summary {
        for (k, v) in map {
            out.insert(k, v);
        }
    }
    out.insert("pekerjaan_air_minum_count".into(), json!(air_count));
    out.insert("derived_sr".into(), json!(sr));
    out.insert("derived_kk".into(), json!(kk));
    out.insert("derived_jiwa".into(), json!(jiwa));
    out.insert("derived_nilai_kontrak".into(), number_like_php(nilai));
    out.insert("manual_sr".into(), json!(manual.sr));
    out.insert("manual_kk".into(), json!(manual.kk));
    out.insert("manual_jiwa".into(), json!(manual.jiwa));
    out.insert("manual_nilai_kontrak".into(), number_like_php(manual.nilai));
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tautan pekerjaan dan sinkron akumulasi
// ---------------------------------------------------------------------------

/// Ringkasan untuk pesan `attachPekerjaan`.
pub enum AttachError {
    /// `InvalidArgumentException` (422).
    Invalid(String),
    /// `ModelNotFoundException` (404).
    NotFound,
    Db(ApiError),
}

impl From<ApiError> for AttachError {
    fn from(e: ApiError) -> Self {
        AttachError::Db(e)
    }
}

impl From<sqlx::Error> for AttachError {
    fn from(e: sqlx::Error) -> Self {
        AttachError::Db(internal(e))
    }
}

/// `attachPekerjaan`: tautkan (atau perbarui pivot) lalu sinkron akumulasi.
pub async fn attach_pekerjaan(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    unit_id: i64,
    unit_desa: Option<i64>,
    pekerjaan_id: i64,
    output_id: Option<i64>,
    capaian_metric: Option<&str>,
) -> Result<(), AttachError> {
    let conn: &mut MySqlConnection = tx;
    // findPekerjaanForAttach
    let pekerjaan_loaded = if capaian_metric == Some("bjp") {
        let mut q = Sq {
            sql: "SELECT CAST(p.id AS SIGNED) FROM tbl_pekerjaan p WHERE p.id = ? AND EXISTS (SELECT 1 FROM tbl_kegiatan k WHERE k.id = p.kegiatan_id AND k.sub_bidang = ?)"
                .to_string(),
            binds: vec![B::I(pekerjaan_id), s("Air Minum")],
        };
        if let Some(desa) = unit_desa {
            q.push(" AND p.desa_id = ?", vec![B::I(desa)]);
        }
        let (rsql, rbinds) = restriction(ctx);
        q.push(&rsql, rbinds);
        fetch_i64s(conn, &q).await?
    } else {
        let filter = AirFilter {
            unit_spam_id: Some(unit_id),
            accumulation_only: None,
            ..Default::default()
        };
        let mut q = air_minum_sq(conn, ctx, &filter, "CAST(p.id AS SIGNED)").await?;
        q.push(" AND p.id = ?", vec![B::I(pekerjaan_id)]);
        fetch_i64s(conn, &q).await?
    };
    if pekerjaan_loaded.is_empty() {
        return Err(AttachError::NotFound);
    }
    let pekerjaan = load_pekerjaan(conn, &[pekerjaan_id])
        .await?
        .remove(&pekerjaan_id)
        .ok_or(AttachError::NotFound)?;

    let mut output: Option<OutRow> = None;
    if let Some(oid) = output_id {
        let found = pekerjaan.outputs.iter().find(|o| o.id == oid).cloned();
        let Some(o) = found else {
            return Err(AttachError::NotFound);
        };
        let resolved = resolve_capaian_metric(capaian_metric, Some(&o), &pekerjaan.outputs);
        if resolved != "bjp" && !is_air_minum_komponen(o.komponen.as_deref().unwrap_or("")) {
            return Err(AttachError::Invalid(
                "Output bukan komponen air minum yang didukung.".into(),
            ));
        }
        output = Some(o);
    }
    let resolved = resolve_capaian_metric(capaian_metric, output.as_ref(), &pekerjaan.outputs);

    // syncWithoutDetaching: tambah atau perbarui pivot yang sudah ada.
    upsert_pivot(tx, unit_id, pekerjaan_id, output_id, resolved, ctx).await?;
    sync_unit_accumulation(tx, ctx, unit_id).await?;
    Ok(())
}

/// Tulis pivot seperti `syncWithoutDetaching([id => [output_id, capaian_metric]])`.
async fn upsert_pivot(
    tx: &mut Transaction<'_, MySql>,
    unit_id: i64,
    pekerjaan_id: i64,
    output_id: Option<i64>,
    metric: &str,
    _ctx: &Ctx<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tbl_unit_spam_pekerjaan (unit_spam_id, pekerjaan_id, output_id, capaian_metric, created_at, updated_at) \
         VALUES (?, ?, ?, ?, NOW(), NOW()) \
         ON DUPLICATE KEY UPDATE \
         updated_at = IF(output_id <=> VALUES(output_id) AND capaian_metric = VALUES(capaian_metric), updated_at, NOW()), \
         output_id = VALUES(output_id), capaian_metric = VALUES(capaian_metric)",
    )
    .bind(unit_id)
    .bind(pekerjaan_id)
    .bind(output_id)
    .bind(metric)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// `detachPekerjaan`: hapus anggaran lama sesuai nama paket, lalu lepas pivot dan sinkron.
pub async fn detach_pekerjaan(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    unit_id: i64,
    pekerjaan_id: i64,
) -> Result<(), ApiError> {
    let row = sqlx::query(
        "SELECT p.nama_paket, k.tahun_anggaran FROM tbl_unit_spam_pekerjaan up \
         JOIN tbl_pekerjaan p ON p.id = up.pekerjaan_id LEFT JOIN tbl_kegiatan k ON k.id = p.kegiatan_id \
         WHERE up.unit_spam_id = ? AND up.pekerjaan_id = ?",
    )
    .bind(unit_id)
    .bind(pekerjaan_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;
    if let Some(r) = row {
        let nama: Option<String> = r.try_get("nama_paket").map_err(internal)?;
        let tahun: Option<String> = r.try_get("tahun_anggaran").map_err(internal)?;
        let tahun = tahun.unwrap_or_default();
        if let (true, Some(nama)) = (is_accumulation_tahun(&tahun), nama) {
            sqlx::query(
                "DELETE FROM tbl_spam_budgets WHERE unit_spam_id = ? AND pekerjaan_id IS NULL AND tahun = ? AND nama_paket = ?",
            )
            .bind(unit_id)
            .bind(&tahun)
            .bind(&nama)
            .execute(&mut **tx)
            .await
            .map_err(internal)?;
        }
    }

    sqlx::query("DELETE FROM tbl_unit_spam_pekerjaan WHERE unit_spam_id = ? AND pekerjaan_id = ?")
        .bind(unit_id)
        .bind(pekerjaan_id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    sync_unit_accumulation(tx, ctx, unit_id).await?;
    Ok(())
}

/// Mutasi model yang dicatat audit dan notifikasi admin (`Auditable` + `NotifiesAdminsOnChanges`).
pub struct ModelChange<'a> {
    pub event: &'a str,
    pub model: &'a str,
    pub id: u64,
    pub old: Option<Map<String, Value>>,
    pub new: Option<Map<String, Value>>,
}

/// Tulis audit lalu notifikasi admin untuk satu perubahan model.
pub async fn log_model_change(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    actor: u64,
    ch: ModelChange<'_>,
) -> Result<(), ApiError> {
    crate::audit::write(
        tx,
        crate::audit::Entry {
            actor,
            event: ch.event,
            auditable_type: ch.model,
            auditable_id: ch.id,
            old: ch.old,
            new: ch.new,
            url: ctx.url,
        },
        ctx.headers,
    )
    .await
    .map_err(internal)?;

    let short = ch.model.rsplit('\\').next().unwrap_or(ch.model);
    let action = match ch.event {
        "created" => "dibuat",
        "updated" => "diperbarui",
        _ => "dihapus",
    };
    let name = notify::actor_name(tx, actor).await.map_err(internal)?;
    let message = notify::change_message(short, ch.id, action, &name, false);
    let title = format!("Data {short} {action}");
    notify::admins(tx, actor, &title, &message, None)
        .await
        .map_err(internal)?;
    Ok(())
}

/// Ambil baris `tbl_spam_achievements` integrasi lalu upsert. Mengembalikan id baris.
async fn upsert_integrasi_achievement(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    actor: u64,
    unit_id: i64,
    tahun: &str,
    d: &Agg,
) -> Result<i64, ApiError> {
    let existing: Option<(i64, i64, i64, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT CAST(id AS SIGNED), CAST(jumlah_sr AS SIGNED), CAST(jumlah_kk AS SIGNED), CAST(jumlah_jiwa AS SIGNED), \
         CAST(jumlah_bjp_kk AS SIGNED), catatan FROM tbl_spam_achievements WHERE unit_spam_id = ? AND tahun = ? AND sumber = ?",
    )
    .bind(unit_id)
    .bind(tahun)
    .bind(SUMBER_INTEGRASI)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;

    let new_vals = (d.sr, d.kk, d.jiwa, d.bjp_kk, d.bjp_jiwa);
    match existing {
        Some((id, sr, kk, jiwa, bjp_kk, catatan)) => {
            let changed = sr != new_vals.0
                || kk != new_vals.1
                || jiwa != new_vals.2
                || bjp_kk != new_vals.3
                || catatan.as_deref() != Some(INTEGRASI_CATATAN);
            if changed {
                let old_map = json!({ "jumlah_sr": sr, "jumlah_kk": kk, "jumlah_jiwa": jiwa, "jumlah_bjp_kk": bjp_kk, "catatan": catatan })
                    .as_object()
                    .cloned();
                sqlx::query(
                    "UPDATE tbl_spam_achievements SET jumlah_sr = ?, jumlah_kk = ?, jumlah_jiwa = ?, jumlah_bjp_kk = ?, \
                     jumlah_bjp_jiwa = ?, catatan = ?, updated_at = NOW() WHERE id = ?",
                )
                .bind(d.sr)
                .bind(d.kk)
                .bind(d.jiwa)
                .bind(d.bjp_kk)
                .bind(d.bjp_jiwa)
                .bind(INTEGRASI_CATATAN)
                .bind(id)
                .execute(&mut **tx)
                .await
                .map_err(internal)?;
                let new_map = json!({
                    "jumlah_sr": d.sr, "jumlah_kk": d.kk, "jumlah_jiwa": d.jiwa,
                    "jumlah_bjp_kk": d.bjp_kk, "catatan": INTEGRASI_CATATAN,
                })
                .as_object()
                .cloned();
                log_model_change(
                    tx,
                    ctx,
                    actor,
                    ModelChange {
                        event: "updated",
                        model: MODEL_ACHIEVEMENT,
                        id: id as u64,
                        old: old_map,
                        new: new_map,
                    },
                )
                .await?;
            }
            Ok(id)
        }
        None => {
            let res = sqlx::query(
                "INSERT INTO tbl_spam_achievements (unit_spam_id, tahun, sumber, jumlah_sr, jumlah_kk, jumlah_jiwa, jumlah_bjp_kk, \
                 jumlah_bjp_jiwa, catatan, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
            )
            .bind(unit_id)
            .bind(tahun)
            .bind(SUMBER_INTEGRASI)
            .bind(d.sr)
            .bind(d.kk)
            .bind(d.jiwa)
            .bind(d.bjp_kk)
            .bind(d.bjp_jiwa)
            .bind(INTEGRASI_CATATAN)
            .execute(&mut **tx)
            .await
            .map_err(internal)?;
            let id = res.last_insert_id() as i64;
            let new_map = json!({
                "unit_spam_id": unit_id, "tahun": tahun, "sumber": SUMBER_INTEGRASI,
                "jumlah_sr": d.sr, "jumlah_kk": d.kk, "jumlah_jiwa": d.jiwa,
                "jumlah_bjp_kk": d.bjp_kk, "jumlah_bjp_jiwa": d.bjp_jiwa,
                "catatan": INTEGRASI_CATATAN, "id": id,
            })
            .as_object()
            .cloned();
            log_model_change(
                tx,
                ctx,
                actor,
                ModelChange {
                    event: "created",
                    model: MODEL_ACHIEVEMENT,
                    id: id as u64,
                    old: None,
                    new: new_map,
                },
            )
            .await?;
            Ok(id)
        }
    }
}

/// `syncUnitAccumulationFromLinks`. Mengembalikan `(id achievement, id budget)` yang disentuh.
pub async fn sync_unit_accumulation(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    unit_id: i64,
) -> Result<(Vec<i64>, Vec<i64>), ApiError> {
    let actor = ctx.user.unwrap_or(0);
    let mut ach_ids = Vec::new();
    let mut bud_ids = Vec::new();

    let unit_desa_id: i64 =
        sqlx::query_scalar("SELECT CAST(desa_id AS SIGNED) FROM tbl_unit_spam WHERE id = ?")
            .bind(unit_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(internal)?;

    let pivots = unit_pivots(&mut *tx, unit_id).await?;
    let linked_ids: Vec<i64> = pivots
        .keys()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let linked = load_pekerjaan(&mut *tx, &linked_ids).await?;

    // Kelompokkan per tahun anggaran, urut kemunculan pertama.
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<i64>> = HashMap::new();
    for id in &linked_ids {
        let Some(p) = linked.get(id) else { continue };
        let key = if p.has_kegiatan && p.kegiatan_tahun.is_some() {
            p.tahun()
        } else {
            "unknown".to_string()
        };
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(*id);
    }

    let mut active_tahun: Vec<String> = Vec::new();
    for tahun in &order {
        if tahun == "unknown" || !is_accumulation_tahun(tahun) {
            continue;
        }
        active_tahun.push(tahun.clone());
        let members = &groups[tahun];
        let items: Vec<(&Pkj, Option<&Pivot>)> = members
            .iter()
            .filter_map(|id| linked.get(id).map(|p| (p, pivots.get(id))))
            .collect();
        let agg = aggregate_derived(&items);
        let aid = upsert_integrasi_achievement(tx, ctx, actor, unit_id, tahun, &agg).await?;
        ach_ids.push(aid);
    }

    // Hapus rekam integrasi untuk tahun yang tidak lagi punya paket (tanpa event, seperti builder delete).
    let mut del =
        String::from("DELETE FROM tbl_spam_achievements WHERE unit_spam_id = ? AND sumber = ?");
    let mut binds: Vec<B> = vec![B::I(unit_id), s(SUMBER_INTEGRASI)];
    if !active_tahun.is_empty() {
        del.push_str(&format!(
            " AND tahun NOT IN ({})",
            placeholders(active_tahun.len())
        ));
        binds.extend(active_tahun.iter().map(|t| s(t)));
    }
    let mut dq = sqlx::query(&del);
    for b in &binds {
        dq = match b {
            B::S(v) => dq.bind(v.clone()),
            B::I(v) => dq.bind(*v),
            B::F(v) => dq.bind(*v),
        };
    }
    dq.execute(&mut **tx).await.map_err(internal)?;

    // Anggaran per paket tertaut.
    let mut kept: Vec<i64> = Vec::new();
    for id in &linked_ids {
        let Some(p) = linked.get(id) else { continue };
        let pivot = pivots.get(id);
        let metrics = derived(p, pivot);
        let tahun = p.tahun();
        if tahun.is_empty() || !is_accumulation_tahun(&tahun) || metrics.nilai_kontrak <= 0.0 {
            continue;
        }
        let nama = p.nama_paket.clone();
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT CAST(id AS SIGNED) FROM tbl_spam_budgets WHERE unit_spam_id = ? AND pekerjaan_id = ? ORDER BY id LIMIT 1",
        )
        .bind(unit_id)
        .bind(p.id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(internal)?;
        let budget_id = match existing {
            Some(bid) => Some(bid),
            None => sqlx::query_scalar(
                "SELECT CAST(id AS SIGNED) FROM tbl_spam_budgets WHERE unit_spam_id = ? AND pekerjaan_id IS NULL AND tahun = ? \
                 AND nama_paket = ? ORDER BY id LIMIT 1",
            )
            .bind(unit_id)
            .bind(&tahun)
            .bind(nama.clone())
            .fetch_optional(&mut **tx)
            .await
            .map_err(internal)?,
        };
        let sumber_dana = p
            .kegiatan_sumber_dana
            .clone()
            .unwrap_or_else(|| "APBD".to_string());
        let sumber_dana = if p.has_kegiatan {
            sumber_dana
        } else {
            "APBD".to_string()
        };
        let nilai = metrics.nilai_kontrak;
        let nama_text = nama.clone().unwrap_or_default();

        match budget_id {
            Some(bid) => {
                let old: Option<(Option<i64>, String, String, f64, String)> = sqlx::query_as(
                    "SELECT CAST(pekerjaan_id AS SIGNED), tahun, nama_paket, nilai_kontrak, sumber_dana FROM tbl_spam_budgets WHERE id = ?",
                )
                .bind(bid)
                .fetch_optional(&mut **tx)
                .await
                .map_err(internal)?;
                let changed = match &old {
                    Some((pid, t, n, v, sd)) => {
                        *pid != Some(p.id)
                            || *t != tahun
                            || *n != nama_text
                            || (*v - nilai).abs() > f64::EPSILON
                            || *sd != sumber_dana
                    }
                    None => true,
                };
                if changed {
                    sqlx::query(
                        "UPDATE tbl_spam_budgets SET pekerjaan_id = ?, tahun = ?, nama_paket = ?, nilai_kontrak = ?, sumber_dana = ?, \
                         updated_at = NOW() WHERE id = ?",
                    )
                    .bind(p.id)
                    .bind(&tahun)
                    .bind(&nama_text)
                    .bind(nilai)
                    .bind(&sumber_dana)
                    .bind(bid)
                    .execute(&mut **tx)
                    .await
                    .map_err(internal)?;
                    let new_map = json!({
                        "pekerjaan_id": p.id, "tahun": tahun, "nama_paket": nama_text,
                        "nilai_kontrak": nilai, "sumber_dana": sumber_dana,
                    })
                    .as_object()
                    .cloned();
                    let old_map = old.map(|(pid, t, n, v, sd)| {
                        json!({ "pekerjaan_id": pid, "tahun": t, "nama_paket": n, "nilai_kontrak": v, "sumber_dana": sd })
                            .as_object()
                            .cloned()
                            .unwrap_or_default()
                    });
                    log_model_change(
                        tx,
                        ctx,
                        actor,
                        ModelChange {
                            event: "updated",
                            model: MODEL_BUDGET,
                            id: bid as u64,
                            old: old_map,
                            new: new_map,
                        },
                    )
                    .await?;
                }
                kept.push(bid);
                bud_ids.push(bid);
            }
            None => {
                let res = sqlx::query(
                    "INSERT INTO tbl_spam_budgets (unit_spam_id, pekerjaan_id, tahun, nama_paket, nilai_kontrak, sumber_dana, created_at, updated_at) \
                     VALUES (?, ?, ?, ?, ?, ?, NOW(), NOW())",
                )
                .bind(unit_id)
                .bind(p.id)
                .bind(&tahun)
                .bind(&nama_text)
                .bind(nilai)
                .bind(&sumber_dana)
                .execute(&mut **tx)
                .await
                .map_err(internal)?;
                let bid = res.last_insert_id() as i64;
                let new_map = json!({
                    "unit_spam_id": unit_id, "pekerjaan_id": p.id, "tahun": tahun, "nama_paket": nama_text,
                    "nilai_kontrak": nilai, "sumber_dana": sumber_dana, "id": bid,
                })
                .as_object()
                .cloned();
                log_model_change(
                    tx,
                    ctx,
                    actor,
                    ModelChange {
                        event: "created",
                        model: MODEL_BUDGET,
                        id: bid as u64,
                        old: None,
                        new: new_map,
                    },
                )
                .await?;
                kept.push(bid);
                bud_ids.push(bid);
            }
        }
    }

    // Anggaran integrasi milik paket yang sudah tidak tertaut / tidak memenuhi syarat.
    let mut del_b = String::from(
        "DELETE FROM tbl_spam_budgets WHERE unit_spam_id = ? AND pekerjaan_id IS NOT NULL",
    );
    let mut bbinds: Vec<B> = vec![B::I(unit_id)];
    if !kept.is_empty() {
        del_b.push_str(&format!(" AND id NOT IN ({})", placeholders(kept.len())));
        bbinds.extend(kept.iter().map(|i| B::I(*i)));
    }
    let mut dbq = sqlx::query(&del_b);
    for b in &bbinds {
        dbq = match b {
            B::S(v) => dbq.bind(v.clone()),
            B::I(v) => dbq.bind(*v),
            B::F(v) => dbq.bind(*v),
        };
    }
    dbq.execute(&mut **tx).await.map_err(internal)?;

    // Anggaran lama (tanpa pekerjaan_id) untuk paket desa yang tidak tertaut.
    let filter = AirFilter {
        desa_id: Some(unit_desa_id),
        accumulation_only: Some(true),
        ..Default::default()
    };
    let candidates_ids = air_minum_ids(
        &mut *tx,
        &Ctx {
            user: ctx.user,
            roles: ctx.roles,
            url: ctx.url,
            headers: ctx.headers,
        },
        &filter,
    )
    .await?;
    let candidates = load_pekerjaan(&mut *tx, &candidates_ids).await?;
    for cand in candidates.values() {
        if linked_ids.contains(&cand.id) {
            continue;
        }
        let tahun = cand.tahun();
        if tahun.is_empty() || !is_accumulation_tahun(&tahun) {
            continue;
        }
        sqlx::query(
            "DELETE FROM tbl_spam_budgets WHERE unit_spam_id = ? AND pekerjaan_id IS NULL AND tahun = ? AND nama_paket = ?",
        )
        .bind(unit_id)
        .bind(&tahun)
        .bind(cand.nama_paket.clone())
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    }

    Ok((ach_ids, bud_ids))
}

/// `syncToUnit` (deprecated di Laravel, tetap dipakai `sync-pekerjaan`). Menautkan semua paket
/// air minum tahun integrasi di desa unit, lalu sinkron.
pub async fn sync_to_unit(
    tx: &mut Transaction<'_, MySql>,
    ctx: &Ctx<'_>,
    unit_id: i64,
    tahun: &str,
) -> Result<(Vec<i64>, Vec<i64>), ApiError> {
    let unit_desa_id: i64 =
        sqlx::query_scalar("SELECT CAST(desa_id AS SIGNED) FROM tbl_unit_spam WHERE id = ?")
            .bind(unit_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(internal)?;
    let filter = AirFilter {
        tahun: Some(tahun.to_string()),
        desa_id: Some(unit_desa_id),
        accumulation_only: Some(true),
        ..Default::default()
    };
    let ids = air_minum_ids(&mut *tx, ctx, &filter).await?;
    let loaded = load_pekerjaan(&mut *tx, &ids).await?;
    for id in &ids {
        let Some(p) = loaded.get(id) else { continue };
        if !is_accumulation_tahun(&p.tahun()) {
            continue;
        }
        sqlx::query(
            "INSERT INTO tbl_unit_spam_pekerjaan (unit_spam_id, pekerjaan_id, output_id, capaian_metric, created_at, updated_at) \
             VALUES (?, ?, NULL, 'jp', NOW(), NOW()) \
             ON DUPLICATE KEY UPDATE updated_at = IF(output_id IS NULL, updated_at, NOW()), output_id = NULL",
        )
        .bind(unit_id)
        .bind(p.id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    }
    sync_unit_accumulation(tx, ctx, unit_id).await
}

/// Rekam achievement/budget hasil sinkron, dibaca ulang dari DB untuk respons.
pub async fn fetch_raw_rows(
    c: &mut MySqlConnection,
    table: &str,
    ids: &[i64],
) -> Result<Vec<Value>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT * FROM {table} WHERE id IN ({}) ORDER BY id",
        placeholders(ids.len())
    );
    let mut q = sqlx::query(&sql);
    for id in ids {
        q = q.bind(*id);
    }
    let rows = q.fetch_all(c).await.map_err(internal)?;
    rows.iter()
        .map(|r| crate::desa_profile::raw_model(r, &[]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_komponen_like_laravel() {
        assert_eq!(
            classify_air_minum_komponen("Sambungan Rumah (SR)"),
            Some("sambungan_rumah")
        );
        assert_eq!(
            classify_air_minum_komponen("SR 20 unit"),
            Some("sambungan_rumah")
        );
        assert_eq!(
            classify_air_minum_komponen("Reservoir 10 m3"),
            Some("reservoir")
        );
        assert_eq!(
            classify_air_minum_komponen("Pipa PVC 300 m"),
            Some("pipa_jaringan")
        );
        assert_eq!(classify_air_minum_komponen("Sumur bor"), Some("bjp"));
        assert_eq!(
            classify_air_minum_komponen("Pompa submersible"),
            Some("sumber_air")
        );
        assert_eq!(classify_air_minum_komponen("Jalan desa"), None);
        // `\bsr\b`: bagian kata tidak dihitung.
        assert_eq!(
            classify_air_minum_komponen("Pasar sr"),
            Some("sambungan_rumah")
        );
        assert_eq!(classify_air_minum_komponen("Dasar"), None);
    }

    #[test]
    fn satuan_countable_like_output_satuan() {
        assert!(is_countable_satuan(None));
        assert!(is_countable_satuan(Some("unit")));
        assert!(is_countable_satuan(Some("KK")));
        assert!(!is_countable_satuan(Some("m3")));
        assert!(
            !is_countable_satuan(Some("M²")),
            "m2 dengan superskrip ditolak"
        );
        assert!(!is_countable_satuan(Some("Ls")));
        assert!(!is_countable_satuan(Some("paket")));
    }

    #[test]
    fn tahun_integrasi_rules() {
        assert!(is_accumulation_tahun("2026"));
        assert!(!is_accumulation_tahun("2025"));
        assert!(!is_accumulation_tahun("unknown"));
        assert!(!is_accumulation_tahun(""));
        assert_eq!(php_int(" 2026abc"), 2026);
        assert_eq!(php_int("abc"), 0);
    }
}
