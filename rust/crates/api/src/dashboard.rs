//! `GET /api/dashboard/stats` dan `GET /api/dashboard/executive-progress` (`DashboardController`).
//!
//! Semantik yang ditiru:
//! - Tipe JSON mengikuti PHP dengan driver native (`ATTR_EMULATE_PREPARES=false`): `COUNT` dan
//!   `SUM` pada kolom `DECIMAL` menjadi string (`"1234.00"`), `SUM` pada kolom `FLOAT`/`DOUBLE`
//!   menjadi float, `round()` selalu float, dan `SUM` kosong (NULL) menjadi `0` karena `?? 0`.
//! - Agregasi `SUM(pagu)` untuk kegiatan memakai `DECIMAL` (string), untuk pekerjaan `FLOAT` (float).
//! - Filter `tahun` pada `stats` memakai bind string, sedangkan `executive-progress` memakai bind
//!   integer (`(int)` di PHP), sehingga perbandingan pada `tahun_anggaran` (VARCHAR) ikut berbeda.
//! - `latestFisik` pada `stats` memakai `keyBy()` sehingga yang terpakai adalah baris terakhir
//!   dari urutan `tanggal DESC, id DESC`, yaitu realisasi fisik TERLAMA per paket. Ini diikuti apa
//!   adanya (kemungkinan bug di Laravel, lihat laporan).
//!
//! Perbedaan yang diketahui:
//! - Cache Laravel (`Cache::remember`, 30 menit, kunci versi) tidak ada. Respon selalu segar.
//! - Laravel tidak memakai `ORDER BY` pada beberapa `GROUP BY`. Urutan di sini ditetapkan
//!   (`ORDER BY` nama atau jumlah), sehingga urutan baris bisa berbeda bila Laravel mengembalikan
//!   urutan lain.
//! - Urutan kunci objek JSON dinormalisasi oleh `serde_json` (abjad), tidak mengikuti urutan PHP.

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{RawQuery, State},
    http::HeaderMap,
    Json,
};
use chrono::{Datelike, NaiveDate, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{mysql::MySqlRow, MySqlPool, Row};

use crate::{desa::internal, php, require_auth, AppState};

/// Parameter SQL. `Text` untuk string dari query, `Int` untuk `(int)` (PHP mengirim integer).
#[derive(Debug, Clone, PartialEq)]
pub enum Bind {
    Text(String),
    Int(i64),
}

/// Jalankan SQL dengan bind berurutan.
pub async fn rows(pool: &MySqlPool, sql: &str, binds: &[Bind]) -> Result<Vec<MySqlRow>, sqlx::Error> {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = match b {
            Bind::Text(s) => q.bind(s.clone()),
            Bind::Int(i) => q.bind(*i),
        };
    }
    q.fetch_all(pool).await
}

/// `SELECT <satu agregat>`: nilai pertama sebagai i64 (0 bila tanpa baris).
async fn count(pool: &MySqlPool, sql: &str, binds: &[Bind]) -> Result<i64, sqlx::Error> {
    let r = rows(pool, sql, binds).await?;
    match r.first() {
        Some(row) => row.try_get::<i64, _>(0),
        None => Ok(0),
    }
}

/// `SELECT CAST(... AS CHAR)`: nilai pertama sebagai string (None bila NULL atau tanpa baris).
async fn opt_text(pool: &MySqlPool, sql: &str, binds: &[Bind]) -> Result<Option<String>, sqlx::Error> {
    let r = rows(pool, sql, binds).await?;
    match r.first() {
        Some(row) => row.try_get::<Option<String>, _>(0),
        None => Ok(None),
    }
}

/// `SELECT CAST(... AS DOUBLE)`: nilai pertama sebagai f64 (None bila NULL atau tanpa baris).
async fn opt_f64(pool: &MySqlPool, sql: &str, binds: &[Bind]) -> Result<Option<f64>, sqlx::Error> {
    let r = rows(pool, sql, binds).await?;
    match r.first() {
        Some(row) => row.try_get::<Option<f64>, _>(0),
        None => Ok(None),
    }
}

/// Hasil `sum()` Laravel dari kolom DECIMAL: string bila ada, `0` bila NULL (`?? 0`).
fn decimal_or_zero(v: Option<String>) -> Value {
    match v {
        Some(s) => Value::String(s),
        None => json!(0),
    }
}

/// Hasil `sum()` Laravel dari kolom FLOAT/DOUBLE: float bila ada, `0` bila NULL.
fn float_or_zero(v: Option<f64>) -> Value {
    match v {
        Some(f) => json!(f),
        None => json!(0),
    }
}

/// String dari kolom yang bisa NULL, `(string)` PHP (NULL menjadi "").
fn php_string(v: Option<String>) -> String {
    v.unwrap_or_default()
}

/// Kondisi SQL berurutan beserta bind-nya.
#[derive(Debug, Default, Clone)]
pub struct Cond {
    pub sql: Vec<String>,
    pub binds: Vec<Bind>,
}

impl Cond {
    fn push(&mut self, sql: impl Into<String>, binds: Vec<Bind>) {
        self.sql.push(sql.into());
        self.binds.extend(binds);
    }

    /// Klausa untuk `WHERE`. `1 = 1` bila tidak ada kondisi.
    pub fn clause(&self) -> String {
        if self.sql.is_empty() {
            "1 = 1".to_string()
        } else {
            self.sql.join(" AND ")
        }
    }

    /// Gabungkan dengan kondisi lain (tanpa mengubah bind urutannya).
    pub fn and(mut self, other: &Cond) -> Self {
        self.sql.extend(other.sql.iter().cloned());
        self.binds.extend(other.binds.iter().cloned());
        self
    }
}

/// Filter dasar yang dipakai beberapa endpoint dashboard.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// `tahun` (sudah lolos truthy bila dipakai).
    pub tahun: Option<Bind>,
    /// Daftar kecamatan. `None` berarti tanpa filter.
    pub kecamatan: Option<Vec<Bind>>,
    /// `tag_id` (sudah di-`(int)`, 0 berarti tanpa filter).
    pub tag: Option<i64>,
}

const NOT_CANCELED: &str = "(tbl_pekerjaan.status IS NULL OR tbl_pekerjaan.status <> 'canceled')";
const WITH_KONTRAK: &str = "(EXISTS (SELECT * FROM tbl_kontrak WHERE tbl_kontrak.id_pekerjaan = tbl_pekerjaan.id) \
     OR EXISTS (SELECT * FROM kontrak_pekerjaan WHERE kontrak_pekerjaan.pekerjaan_id = tbl_pekerjaan.id))";

impl Scope {
    /// Kondisi `Pekerjaan::query()` dengan `tahun`, kecamatan, dan tag. `active` menambah `notCanceled`.
    pub fn pekerjaan(&self, active: bool) -> Cond {
        let mut c = Cond::default();
        if active {
            c.push(NOT_CANCELED, vec![]);
        }
        if let Some(t) = &self.tahun {
            c.push(
                "EXISTS (SELECT * FROM tbl_kegiatan WHERE tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
                 AND tbl_kegiatan.tahun_anggaran = ?)",
                vec![t.clone()],
            );
        }
        if let Some(k) = &self.kecamatan {
            c.push(
                format!("tbl_pekerjaan.kecamatan_id IN ({})", placeholders(k.len())),
                k.clone(),
            );
        }
        if let Some(tag) = self.tag {
            c.push(
                "EXISTS (SELECT * FROM tbl_tags INNER JOIN pekerjaan_tag ON tbl_tags.id = pekerjaan_tag.tag_id \
                 WHERE tbl_pekerjaan.id = pekerjaan_tag.pekerjaan_id AND tbl_tags.id = ?)",
                vec![Bind::Int(tag)],
            );
        }
        c
    }

    /// Kondisi `Kegiatan::query()`: tahun, dan kecamatan lewat `tbl_pekerjaan`.
    pub fn kegiatan(&self) -> Cond {
        let mut c = Cond::default();
        if let Some(t) = &self.tahun {
            c.push("tbl_kegiatan.tahun_anggaran = ?", vec![t.clone()]);
        }
        if let Some(k) = &self.kecamatan {
            c.push(
                format!(
                    "tbl_kegiatan.id IN (SELECT tbl_pekerjaan.kegiatan_id FROM tbl_pekerjaan \
                     WHERE tbl_pekerjaan.kecamatan_id IN ({}))",
                    placeholders(k.len())
                ),
                k.clone(),
            );
        }
        c
    }
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// `$kecamatanIds` untuk `stats` dan `executive-progress`: `(int)` per elemen, elemen `''` dibuang.
fn kecamatan_ints(p: &php::Params) -> Option<Vec<Bind>> {
    let ints: Vec<Bind> = kecamatan_raw(p)
        .into_iter()
        .filter(|v| !v.is_empty())
        .map(|v| Bind::Int(php::intval_str(&v)))
        .collect();
    if ints.is_empty() {
        None
    } else {
        Some(ints)
    }
}

/// Elemen mentah `kecamatan_ids`: string dipecah koma (bila truthy), atau array `kecamatan_ids[]`.
pub fn kecamatan_raw(p: &php::Params) -> Vec<String> {
    let arr = p.array("kecamatan_ids");
    if !arr.is_empty() {
        return arr;
    }
    match p.get("kecamatan_ids") {
        Some(s) if php::truthy(Some(s)) => s.split(',').map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

fn stats_scope(p: &php::Params) -> Scope {
    let tahun = p
        .get("tahun")
        .filter(|t| php::truthy(Some(t)))
        .map(|t| Bind::Text(t.to_string()));
    let tag = match p.get("tag_id") {
        Some(v) if !v.is_empty() => Some(php::intval_str(v)).filter(|t| *t != 0),
        _ => None,
    };
    Scope {
        tahun,
        kecamatan: kecamatan_ints(p),
        tag,
    }
}

/// `GET /api/dashboard/stats`.
pub async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let p = php::Params::parse(raw.as_deref());
    let scope = stats_scope(&p);
    let data = build_stats(&state.pool, &scope).await.map_err(internal)?;
    Ok(Json(json!({ "data": data })))
}

/// Port `DashboardController::stats()` tanpa cache. Urutan blok mengikuti Laravel.
async fn build_stats(pool: &MySqlPool, s: &Scope) -> Result<Value, sqlx::Error> {
    let kc = s.kegiatan();
    let total_kegiatan = count(
        pool,
        &format!("SELECT COUNT(*) FROM tbl_kegiatan WHERE {}", kc.clause()),
        &kc.binds,
    )
    .await?;
    let total_pagu = opt_text(
        pool,
        &format!(
            "SELECT CAST(SUM(tbl_kegiatan.pagu) AS CHAR) FROM tbl_kegiatan WHERE {}",
            kc.clause()
        ),
        &kc.binds,
    )
    .await?;

    let kegiatan_per_tahun = rows(
        pool,
        &format!(
            "SELECT tbl_kegiatan.tahun_anggaran AS name, COUNT(*) AS value FROM tbl_kegiatan WHERE {} \
             GROUP BY tbl_kegiatan.tahun_anggaran ORDER BY tbl_kegiatan.tahun_anggaran",
            kc.clause()
        ),
        &kc.binds,
    )
    .await?
    .iter()
    .map(|r| {
        Ok(json!({
            "name": php_string(r.try_get::<Option<String>, _>("name")?),
            "value": r.try_get::<i64, _>("value")?,
        }))
    })
    .collect::<Result<Vec<Value>, sqlx::Error>>()?;

    let kegiatan_per_sumber_dana = rows(
        pool,
        &format!(
            "SELECT tbl_kegiatan.sumber_dana AS name, COUNT(*) AS value FROM tbl_kegiatan WHERE {} \
             GROUP BY tbl_kegiatan.sumber_dana ORDER BY tbl_kegiatan.sumber_dana",
            kc.clause()
        ),
        &kc.binds,
    )
    .await?
    .iter()
    .map(|r| {
        Ok(json!({
            "name": r.try_get::<Option<String>, _>("name")?.unwrap_or_else(|| "N/A".into()),
            "value": r.try_get::<i64, _>("value")?,
        }))
    })
    .collect::<Result<Vec<Value>, sqlx::Error>>()?;

    // Pekerjaan aktif (notCanceled) dan semua pekerjaan. Semua memakai filter yang sama.
    let active = s.pekerjaan(true);
    let all = s.pekerjaan(false);
    let active_clause = active.clause();

    // Sub kegiatan: paket aktif yang sudah berkontrak, dijumlahkan per nama sub kegiatan.
    let sub_cond = active.clone().and(&Cond {
        sql: vec![WITH_KONTRAK.to_string()],
        binds: vec![],
    });
    let sub_rows = rows(
        pool,
        &format!(
            "SELECT tbl_kegiatan.nama_sub_kegiatan AS name, COUNT(*) AS cnt, \
             CAST(SUM(tbl_pekerjaan.pagu) AS DOUBLE) AS pagu \
             FROM tbl_pekerjaan INNER JOIN tbl_kegiatan ON tbl_pekerjaan.kegiatan_id = tbl_kegiatan.id \
             WHERE {} AND tbl_kegiatan.nama_sub_kegiatan IS NOT NULL AND tbl_kegiatan.nama_sub_kegiatan <> '' \
             GROUP BY tbl_kegiatan.nama_sub_kegiatan ORDER BY tbl_kegiatan.nama_sub_kegiatan",
            sub_cond.clause()
        ),
        &sub_cond.binds,
    )
    .await?;

    // Batal per sub kegiatan (semua paket, status canceled).
    let batal_cond = all.clone();
    let batal_rows = rows(
        pool,
        &format!(
            "SELECT tbl_kegiatan.nama_sub_kegiatan AS name, COUNT(*) AS batal \
             FROM tbl_pekerjaan INNER JOIN tbl_kegiatan ON tbl_pekerjaan.kegiatan_id = tbl_kegiatan.id \
             WHERE {} AND tbl_pekerjaan.status = 'canceled' AND tbl_kegiatan.nama_sub_kegiatan IS NOT NULL \
             GROUP BY tbl_kegiatan.nama_sub_kegiatan",
            batal_cond.clause()
        ),
        &batal_cond.binds,
    )
    .await?;
    let mut batal_by_sub: HashMap<String, i64> = HashMap::new();
    for r in &batal_rows {
        if let Some(name) = r.try_get::<Option<String>, _>("name")? {
            batal_by_sub.insert(name, r.try_get::<i64, _>("batal")?);
        }
    }

    // Belum berkontrak per sub kegiatan: `whereDoesntHave('kontraks')` (pivot saja, join ke tbl_kontrak).
    let belum_rows = rows(
        pool,
        &format!(
            "SELECT tbl_kegiatan.nama_sub_kegiatan AS name, COUNT(*) AS belum \
             FROM tbl_pekerjaan INNER JOIN tbl_kegiatan ON tbl_pekerjaan.kegiatan_id = tbl_kegiatan.id \
             WHERE {} AND NOT EXISTS (SELECT * FROM tbl_kontrak INNER JOIN kontrak_pekerjaan \
             ON tbl_kontrak.id = kontrak_pekerjaan.kontrak_id WHERE tbl_pekerjaan.id = kontrak_pekerjaan.pekerjaan_id) \
             AND tbl_kegiatan.nama_sub_kegiatan IS NOT NULL GROUP BY tbl_kegiatan.nama_sub_kegiatan",
            active_clause
        ),
        &active.binds,
    )
    .await?;
    let mut belum_by_sub: HashMap<String, i64> = HashMap::new();
    for r in &belum_rows {
        if let Some(name) = r.try_get::<Option<String>, _>("name")? {
            belum_by_sub.insert(name, r.try_get::<i64, _>("belum")?);
        }
    }

    // Paket aktif beserta nama sub kegiatan (LEFT JOIN: kegiatan hilang berarti nama null).
    let pekerjaan_sub_rows = rows(
        pool,
        &format!(
            "SELECT CAST(tbl_pekerjaan.id AS SIGNED) AS id, tbl_kegiatan.nama_sub_kegiatan AS name FROM tbl_pekerjaan \
             LEFT JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id WHERE {active_clause}"
        ),
        &active.binds,
    )
    .await?;
    let mut pekerjaan_names: Vec<(i64, Option<String>)> = Vec::with_capacity(pekerjaan_sub_rows.len());
    for r in &pekerjaan_sub_rows {
        pekerjaan_names.push((r.try_get::<i64, _>("id")?, r.try_get::<Option<String>, _>("name")?));
    }

    let mut progress_acc: HashMap<String, (f64, i64)> = HashMap::new();
    let mut sp2d_acc: HashMap<String, f64> = HashMap::new();
    let mut kontrak_acc: HashMap<String, f64> = HashMap::new();

    if !pekerjaan_names.is_empty() {
        // Realisasi fisik: `keyBy` memakai baris terakhir dari urutan tanggal DESC, id DESC,
        // yaitu (tanggal, id) terkecil per paket.
        let fisik_rows = rows(
            pool,
            &format!(
                "SELECT CAST(h.pekerjaan_id AS SIGNED) AS pid, h.tanggal AS tgl, CAST(h.id AS SIGNED) AS hid, CAST(h.persen AS DOUBLE) AS persen \
                 FROM pekerjaan_progress_estimasi_history h \
                 WHERE h.tipe = 'realisasi' AND h.jenis = 'fisik' AND h.pekerjaan_id IN \
                 (SELECT tbl_pekerjaan.id FROM tbl_pekerjaan WHERE {active_clause})"
            ),
            &active.binds,
        )
        .await?;
        let mut latest_fisik: HashMap<i64, (NaiveDate, i64, f64)> = HashMap::new();
        for r in &fisik_rows {
            let pid: i64 = r.try_get("pid")?;
            let tgl: NaiveDate = r.try_get("tgl")?;
            let hid: i64 = r.try_get("hid")?;
            let persen: f64 = r.try_get::<Option<f64>, _>("persen")?.unwrap_or(0.0);
            let keep_new = match latest_fisik.get(&pid) {
                None => true,
                Some((t, i, _)) => (tgl, hid) < (*t, *i),
            };
            if keep_new {
                latest_fisik.insert(pid, (tgl, hid, persen));
            }
        }

        // Total SP2D keuangan per paket.
        let sp2d_rows = rows(
            pool,
            &format!(
                "SELECT CAST(h.pekerjaan_id AS SIGNED) AS pid, CAST(SUM(h.nilai) AS DOUBLE) AS total \
                 FROM pekerjaan_progress_estimasi_history h \
                 WHERE h.tipe = 'realisasi' AND h.jenis = 'keuangan' AND h.pekerjaan_id IN \
                 (SELECT tbl_pekerjaan.id FROM tbl_pekerjaan WHERE {active_clause}) GROUP BY h.pekerjaan_id"
            ),
            &active.binds,
        )
        .await?;
        let mut sp2d_per_pekerjaan: HashMap<i64, f64> = HashMap::new();
        for r in &sp2d_rows {
            let pid: i64 = r.try_get("pid")?;
            sp2d_per_pekerjaan.insert(pid, r.try_get::<Option<f64>, _>("total")?.unwrap_or(0.0));
        }

        for (pid, name) in &pekerjaan_names {
            let Some(name) = name.as_ref().filter(|n| !n.is_empty() && n.as_str() != "0") else {
                continue;
            };
            if let Some((_, _, persen)) = latest_fisik.get(pid) {
                let e = progress_acc.entry(name.clone()).or_insert((0.0, 0));
                e.0 += persen;
                e.1 += 1;
            }
            let sp2d = sp2d_per_pekerjaan.get(pid).copied().unwrap_or(0.0);
            if sp2d > 0.0 {
                *sp2d_acc.entry(name.clone()).or_insert(0.0) += sp2d;
            }
        }

        // Nilai kontrak per sub kegiatan: distinct kontrak per sub, lewat tautan legacy dan pivot.
        let legacy = rows(
            pool,
            &format!(
                "SELECT CAST(tbl_kontrak.id AS SIGNED) AS kid, tbl_kegiatan.nama_sub_kegiatan AS name FROM tbl_kontrak \
                 INNER JOIN tbl_pekerjaan ON tbl_pekerjaan.id = tbl_kontrak.id_pekerjaan \
                 INNER JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
                 WHERE tbl_kontrak.id_pekerjaan IN (SELECT tbl_pekerjaan.id FROM tbl_pekerjaan WHERE {active_clause}) \
                 AND tbl_kegiatan.nama_sub_kegiatan IS NOT NULL"
            ),
            &active.binds,
        )
        .await?;
        let pivot = rows(
            pool,
            &format!(
                "SELECT CAST(kontrak_pekerjaan.kontrak_id AS SIGNED) AS kid, tbl_kegiatan.nama_sub_kegiatan AS name \
                 FROM kontrak_pekerjaan \
                 INNER JOIN tbl_pekerjaan ON tbl_pekerjaan.id = kontrak_pekerjaan.pekerjaan_id \
                 INNER JOIN tbl_kegiatan ON tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
                 WHERE kontrak_pekerjaan.pekerjaan_id IN (SELECT tbl_pekerjaan.id FROM tbl_pekerjaan WHERE {active_clause}) \
                 AND tbl_kegiatan.nama_sub_kegiatan IS NOT NULL"
            ),
            &active.binds,
        )
        .await?;
        // name -> urutan kontrak id unik (urutan kemunculan, seperti array PHP).
        let mut kontrak_by_sub: BTreeMap<String, Vec<i64>> = BTreeMap::new();
        for r in legacy.iter().chain(pivot.iter()) {
            let kid: Option<i64> = r.try_get("kid")?;
            let Some(kid) = kid else { continue };
            let name: String = r.try_get("name")?;
            let list = kontrak_by_sub.entry(name).or_default();
            if !list.contains(&kid) {
                list.push(kid);
            }
        }
        let mut all_kids: Vec<i64> = kontrak_by_sub.values().flatten().copied().collect();
        all_kids.sort_unstable();
        all_kids.dedup();
        if !all_kids.is_empty() {
            let nilai_rows = rows(
                pool,
                &format!(
                    "SELECT CAST(tbl_kontrak.id AS SIGNED) AS id, CAST(tbl_kontrak.nilai_kontrak AS CHAR) AS nilai \
                     FROM tbl_kontrak WHERE tbl_kontrak.id IN ({})",
                    placeholders(all_kids.len())
                ),
                &all_kids.iter().map(|k| Bind::Int(*k)).collect::<Vec<_>>(),
            )
            .await?;
            let mut nilai_by_id: HashMap<i64, f64> = HashMap::new();
            for r in &nilai_rows {
                let id: i64 = r.try_get("id")?;
                let nilai = r
                    .try_get::<Option<String>, _>("nilai")?
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                nilai_by_id.insert(id, nilai);
            }
            for (name, kids) in &kontrak_by_sub {
                let sum: f64 = kids.iter().map(|k| nilai_by_id.get(k).copied().unwrap_or(0.0)).sum();
                kontrak_acc.insert(name.clone(), sum);
            }
        }
    }

    let mut sub_kegiatan_stats = Vec::with_capacity(sub_rows.len());
    for r in &sub_rows {
        let name: String = r.try_get("name")?;
        let cnt: i64 = r.try_get("cnt")?;
        let pagu: f64 = r.try_get::<Option<f64>, _>("pagu")?.unwrap_or(0.0);
        let acc = progress_acc.get(&name);
        let progress = match acc {
            Some((sum, n)) if *n > 0 => json!(php::round(sum / *n as f64, 1)),
            _ => json!(0),
        };
        sub_kegiatan_stats.push(json!({
            "name": name,
            "count": cnt,
            "paguM": php::round(pagu / 1_000_000.0, 2),
            "progress": progress,
            "hasProgress": acc.is_some(),
            "sp2dTotal": php::round(sp2d_acc.get(&name).copied().unwrap_or(0.0), 0),
            "kontrakTotal": php::round(kontrak_acc.get(&name).copied().unwrap_or(0.0), 0),
            "batal": batal_by_sub.get(&name).copied().unwrap_or(0),
            "belumBerkontrak": belum_by_sub.get(&name).copied().unwrap_or(0),
        }));
    }

    let pagu_per_tahun = rows(
        pool,
        &format!(
            "SELECT tbl_kegiatan.tahun_anggaran AS name, CAST(SUM(tbl_kegiatan.pagu) / 1000000 AS CHAR) AS value \
             FROM tbl_kegiatan WHERE {} GROUP BY tbl_kegiatan.tahun_anggaran ORDER BY tbl_kegiatan.tahun_anggaran",
            kc.clause()
        ),
        &kc.binds,
    )
    .await?;
    let mut pagu_per_tahun_out = Vec::with_capacity(pagu_per_tahun.len());
    for r in &pagu_per_tahun {
        let value = r
            .try_get::<Option<String>, _>("value")?
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        pagu_per_tahun_out.push(json!({
            "name": php_string(r.try_get::<Option<String>, _>("name")?),
            "value": php::round(value, 2),
        }));
    }

    let available_years: Vec<Value> = rows(
        pool,
        "SELECT DISTINCT tbl_kegiatan.tahun_anggaran AS y FROM tbl_kegiatan \
         ORDER BY tbl_kegiatan.tahun_anggaran DESC",
        &[],
    )
    .await?
    .iter()
    .map(|r| Ok(r.try_get::<Option<String>, _>("y")?.map_or(Value::Null, Value::String)))
    .collect::<Result<_, sqlx::Error>>()?;

    // Rekap status paket.
    let mut batal_cond = all.clone();
    batal_cond.push("tbl_pekerjaan.status = 'canceled'", vec![]);
    let pekerjaan_batal = count(
        pool,
        &format!("SELECT COUNT(*) FROM tbl_pekerjaan WHERE {}", batal_cond.clause()),
        &batal_cond.binds,
    )
    .await?;
    let pekerjaan_aktif = count(
        pool,
        &format!("SELECT COUNT(*) FROM tbl_pekerjaan WHERE {active_clause}"),
        &active.binds,
    )
    .await?;
    let berkontrak_cond = active.clone().and(&Cond {
        sql: vec![WITH_KONTRAK.to_string()],
        binds: vec![],
    });
    let pekerjaan_berkontrak = count(
        pool,
        &format!("SELECT COUNT(*) FROM tbl_pekerjaan WHERE {}", berkontrak_cond.clause()),
        &berkontrak_cond.binds,
    )
    .await?;
    let pekerjaan_belum_berkontrak = (pekerjaan_aktif - pekerjaan_berkontrak).max(0);

    let fisik_sql = "(tbl_pekerjaan.is_konsultan = 0 OR tbl_pekerjaan.is_konsultan IS NULL)";
    let konsultan_sql = "tbl_pekerjaan.is_konsultan = 1";
    let fisik_cond = active.clone().and(&Cond {
        sql: vec![fisik_sql.to_string()],
        binds: vec![],
    });
    let konsultan_cond = active.clone().and(&Cond {
        sql: vec![konsultan_sql.to_string()],
        binds: vec![],
    });
    let pekerjaan_fisik = count(
        pool,
        &format!("SELECT COUNT(*) FROM tbl_pekerjaan WHERE {}", fisik_cond.clause()),
        &fisik_cond.binds,
    )
    .await?;
    let pekerjaan_konsultan = count(
        pool,
        &format!("SELECT COUNT(*) FROM tbl_pekerjaan WHERE {}", konsultan_cond.clause()),
        &konsultan_cond.binds,
    )
    .await?;
    let fisik_berkontrak_cond = fisik_cond.clone().and(&Cond {
        sql: vec![WITH_KONTRAK.to_string()],
        binds: vec![],
    });
    let pekerjaan_fisik_berkontrak = count(
        pool,
        &format!(
            "SELECT COUNT(*) FROM tbl_pekerjaan WHERE {}",
            fisik_berkontrak_cond.clause()
        ),
        &fisik_berkontrak_cond.binds,
    )
    .await?;
    let pekerjaan_fisik_belum_berkontrak = (pekerjaan_fisik - pekerjaan_fisik_berkontrak).max(0);

    let total_pagu_pekerjaan = opt_f64(
        pool,
        &format!(
            "SELECT CAST(SUM(tbl_pekerjaan.pagu) AS DOUBLE) FROM tbl_pekerjaan WHERE {active_clause}"
        ),
        &active.binds,
    )
    .await?;
    let total_pagu_fisik = opt_f64(
        pool,
        &format!(
            "SELECT CAST(SUM(tbl_pekerjaan.pagu) AS DOUBLE) FROM tbl_pekerjaan WHERE {}",
            fisik_cond.clause()
        ),
        &fisik_cond.binds,
    )
    .await?;
    let total_pagu_konsultan = opt_f64(
        pool,
        &format!(
            "SELECT CAST(SUM(tbl_pekerjaan.pagu) AS DOUBLE) FROM tbl_pekerjaan WHERE {}",
            konsultan_cond.clause()
        ),
        &konsultan_cond.binds,
    )
    .await?;

    let kecamatan_names = kecamatan_map(pool).await?;
    let pekerjaan_per_kecamatan = rows(
        pool,
        &format!(
            "SELECT CAST(tbl_pekerjaan.kecamatan_id AS SIGNED) AS kid, COUNT(*) AS value FROM tbl_pekerjaan \
             WHERE {active_clause} GROUP BY tbl_pekerjaan.kecamatan_id ORDER BY tbl_pekerjaan.kecamatan_id"
        ),
        &active.binds,
    )
    .await?;
    let mut pekerjaan_per_kecamatan_out = Vec::new();
    for r in &pekerjaan_per_kecamatan {
        let kid: Option<i64> = r.try_get("kid")?;
        pekerjaan_per_kecamatan_out.push(json!({
            "name": kid.and_then(|k| kecamatan_names.get(&k).cloned()).unwrap_or_else(|| "N/A".into()),
            "value": r.try_get::<i64, _>("value")?,
        }));
    }

    let desa_names = desa_map(pool).await?;
    let pekerjaan_per_desa = rows(
        pool,
        &format!(
            "SELECT CAST(tbl_pekerjaan.desa_id AS SIGNED) AS kid, COUNT(*) AS value, \
             CAST(SUM(tbl_pekerjaan.pagu) / 1000000 AS DOUBLE) AS pagu_jt FROM tbl_pekerjaan \
             WHERE {active_clause} GROUP BY tbl_pekerjaan.desa_id ORDER BY value DESC, kid"
        ),
        &active.binds,
    )
    .await?;
    let mut pekerjaan_per_desa_out = Vec::new();
    for r in &pekerjaan_per_desa {
        let kid: Option<i64> = r.try_get("kid")?;
        let pagu_jt: f64 = r.try_get::<Option<f64>, _>("pagu_jt")?.unwrap_or(0.0);
        pekerjaan_per_desa_out.push(json!({
            "name": kid.and_then(|k| desa_names.get(&k).cloned()).unwrap_or_else(|| "N/A".into()),
            "value": r.try_get::<i64, _>("value")?,
            "paguJt": php::round(pagu_jt, 2),
        }));
    }

    let pagu_pekerjaan_per_kecamatan = rows(
        pool,
        &format!(
            "SELECT CAST(tbl_pekerjaan.kecamatan_id AS SIGNED) AS kid, \
             CAST(SUM(tbl_pekerjaan.pagu) / 1000000 AS DOUBLE) AS value FROM tbl_pekerjaan \
             WHERE {active_clause} GROUP BY tbl_pekerjaan.kecamatan_id ORDER BY tbl_pekerjaan.kecamatan_id"
        ),
        &active.binds,
    )
    .await?;
    let mut pagu_pekerjaan_per_kecamatan_out = Vec::new();
    for r in &pagu_pekerjaan_per_kecamatan {
        let kid: Option<i64> = r.try_get("kid")?;
        let value: f64 = r.try_get::<Option<f64>, _>("value")?.unwrap_or(0.0);
        pagu_pekerjaan_per_kecamatan_out.push(json!({
            "name": kid.and_then(|k| kecamatan_names.get(&k).cloned()).unwrap_or_else(|| "N/A".into()),
            "value": php::round(value, 2),
        }));
    }

    // Kontrak: tautan legacy atau pivot ke paket aktif (tahun saja, tanpa kecamatan dan tag).
    let kontrak_cond = kontrak_scope(s);
    let total_kontrak = count(
        pool,
        &format!("SELECT COUNT(*) FROM tbl_kontrak WHERE {}", kontrak_cond.clause()),
        &kontrak_cond.binds,
    )
    .await?;
    let total_nilai_kontrak = opt_text(
        pool,
        &format!(
            "SELECT CAST(SUM(tbl_kontrak.nilai_kontrak) AS CHAR) FROM tbl_kontrak WHERE {}",
            kontrak_cond.clause()
        ),
        &kontrak_cond.binds,
    )
    .await?;

    let penyedia_names = penyedia_map(pool).await?;
    let kontrak_per_penyedia = rows(
        pool,
        &format!(
            "SELECT CAST(tbl_kontrak.id_penyedia AS SIGNED) AS pid, COUNT(*) AS value FROM tbl_kontrak \
             WHERE {} GROUP BY tbl_kontrak.id_penyedia ORDER BY value DESC, pid LIMIT 10",
            kontrak_cond.clause()
        ),
        &kontrak_cond.binds,
    )
    .await?;
    let mut kontrak_per_penyedia_out = Vec::new();
    for r in &kontrak_per_penyedia {
        let pid: Option<i64> = r.try_get("pid")?;
        kontrak_per_penyedia_out.push(json!({
            "name": pid.and_then(|p| penyedia_names.get(&p).cloned()).unwrap_or_else(|| "N/A".into()),
            "value": r.try_get::<i64, _>("value")?,
        }));
    }

    let nilai_kontrak_per_penyedia = rows(
        pool,
        &format!(
            "SELECT CAST(tbl_kontrak.id_penyedia AS SIGNED) AS pid, \
             CAST(SUM(tbl_kontrak.nilai_kontrak) / 1000000 AS CHAR) AS value FROM tbl_kontrak \
             WHERE {} GROUP BY tbl_kontrak.id_penyedia ORDER BY SUM(tbl_kontrak.nilai_kontrak) DESC, pid LIMIT 10",
            kontrak_cond.clause()
        ),
        &kontrak_cond.binds,
    )
    .await?;
    let mut nilai_kontrak_per_penyedia_out = Vec::new();
    for r in &nilai_kontrak_per_penyedia {
        let pid: Option<i64> = r.try_get("pid")?;
        let value = r
            .try_get::<Option<String>, _>("value")?
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        nilai_kontrak_per_penyedia_out.push(json!({
            "name": pid.and_then(|p| penyedia_names.get(&p).cloned()).unwrap_or_else(|| "N/A".into()),
            "value": php::round(value, 2),
        }));
    }

    // Output (paket aktif).
    let output_cond = Cond {
        sql: vec![format!(
            "EXISTS (SELECT * FROM tbl_pekerjaan WHERE tbl_pekerjaan.id = tbl_output.pekerjaan_id AND {})",
            active_clause
        )],
        binds: active.binds.clone(),
    };
    let total_output = count(
        pool,
        &format!("SELECT COUNT(*) FROM tbl_output WHERE {}", output_cond.clause()),
        &output_cond.binds,
    )
    .await?;
    let output_per_satuan = group_named(
        pool,
        "tbl_output",
        "satuan",
        &output_cond,
        "value DESC, name",
    )
    .await?;
    let output_per_komponen = group_named(
        pool,
        "tbl_output",
        "komponen",
        &output_cond,
        "value DESC, name",
    )
    .await?;

    // Penerima (paket aktif).
    let penerima_cond = Cond {
        sql: vec![format!(
            "EXISTS (SELECT * FROM tbl_pekerjaan WHERE tbl_pekerjaan.id = tbl_penerima.pekerjaan_id AND {})",
            active_clause
        )],
        binds: active.binds.clone(),
    };
    let total_penerima = count(
        pool,
        &format!("SELECT COUNT(*) FROM tbl_penerima WHERE {}", penerima_cond.clause()),
        &penerima_cond.binds,
    )
    .await?;
    let total_jiwa = opt_text(
        pool,
        &format!(
            "SELECT CAST(SUM(tbl_penerima.jumlah_jiwa) AS CHAR) FROM tbl_penerima WHERE {}",
            penerima_cond.clause()
        ),
        &penerima_cond.binds,
    )
    .await?;
    let komunal_rows = rows(
        pool,
        &format!(
            "SELECT CAST(tbl_penerima.is_komunal AS SIGNED) AS k, COUNT(*) AS value FROM tbl_penerima \
             WHERE {} GROUP BY tbl_penerima.is_komunal ORDER BY k",
            penerima_cond.clause()
        ),
        &penerima_cond.binds,
    )
    .await?;
    let mut komunal_out = Vec::new();
    for r in &komunal_rows {
        let k: i64 = r.try_get("k")?;
        komunal_out.push(json!({
            "name": if k != 0 { "Komunal" } else { "Individu" },
            "value": r.try_get::<i64, _>("value")?,
        }));
    }

    Ok(json!({
        "totalKegiatan": total_kegiatan,
        "totalPagu": decimal_or_zero(total_pagu),
        "kegiatanPerTahun": kegiatan_per_tahun,
        "kegiatanPerSumberDana": kegiatan_per_sumber_dana,
        "subKegiatanStats": sub_kegiatan_stats,
        "paguPerTahun": pagu_per_tahun_out,
        "availableYears": available_years,
        "totalPekerjaan": pekerjaan_aktif,
        "totalPaguPekerjaan": float_or_zero(total_pagu_pekerjaan),
        "pekerjaanAktif": pekerjaan_aktif,
        "pekerjaanBatal": pekerjaan_batal,
        "pekerjaanBerkontrak": pekerjaan_berkontrak,
        "pekerjaanBelumBerkontrak": pekerjaan_belum_berkontrak,
        "pekerjaanFisik": pekerjaan_fisik,
        "pekerjaanKonsultan": pekerjaan_konsultan,
        "pekerjaanFisikBerkontrak": pekerjaan_fisik_berkontrak,
        "pekerjaanFisikBelumBerkontrak": pekerjaan_fisik_belum_berkontrak,
        "totalPaguPekerjaanFisik": float_or_zero(total_pagu_fisik),
        "totalPaguPekerjaanKonsultan": float_or_zero(total_pagu_konsultan),
        "pekerjaanPerKecamatan": pekerjaan_per_kecamatan_out,
        "pekerjaanPerDesa": pekerjaan_per_desa_out,
        "paguPekerjaanPerKecamatan": pagu_pekerjaan_per_kecamatan_out,
        "totalKontrak": total_kontrak,
        "totalNilaiKontrak": decimal_or_zero(total_nilai_kontrak),
        "kontrakPerPenyedia": kontrak_per_penyedia_out,
        "nilaiKontrakPerPenyedia": nilai_kontrak_per_penyedia_out,
        "totalOutput": total_output,
        "outputPerSatuan": output_per_satuan,
        "outputPerKomponen": output_per_komponen,
        "totalPenerima": total_penerima,
        "totalJiwa": decimal_or_zero(total_jiwa),
        "penerimaKomunalVsIndividu": komunal_out,
    }))
}

/// `Kontrak::linkedToPekerjaan(notCanceled + tahun)`: kontrak yang punya paket aktif (legacy atau pivot).
/// Filter kecamatan dan tag tidak dipakai di sini, sama seperti Laravel.
pub fn kontrak_scope(s: &Scope) -> Cond {
    let mut inner = vec![NOT_CANCELED.to_string()];
    let mut inner_binds = vec![];
    if let Some(t) = &s.tahun {
        inner.push(
            "EXISTS (SELECT * FROM tbl_kegiatan WHERE tbl_kegiatan.id = tbl_pekerjaan.kegiatan_id \
             AND tbl_kegiatan.tahun_anggaran = ?)"
                .to_string(),
        );
        inner_binds.push(t.clone());
    }
    let inner_sql = inner.join(" AND ");
    let sql = format!(
        "(EXISTS (SELECT * FROM tbl_pekerjaan WHERE tbl_pekerjaan.id = tbl_kontrak.id_pekerjaan AND {inner_sql}) \
         OR EXISTS (SELECT * FROM tbl_pekerjaan INNER JOIN kontrak_pekerjaan \
         ON tbl_pekerjaan.id = kontrak_pekerjaan.pekerjaan_id \
         WHERE tbl_kontrak.id = kontrak_pekerjaan.kontrak_id AND {inner_sql}))"
    );
    let mut binds = inner_binds.clone();
    binds.extend(inner_binds);
    Cond {
        sql: vec![sql],
        binds,
    }
}

/// `GROUP BY kolom` dengan `count` dan urutan `value DESC` (lalu nama). Nilai NULL menjadi `N/A`.
async fn group_named(
    pool: &MySqlPool,
    table: &str,
    column: &str,
    cond: &Cond,
    order: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    let sql = format!(
        "SELECT {table}.{column} AS name, COUNT(*) AS value FROM {table} WHERE {} \
         GROUP BY {table}.{column} ORDER BY {order}",
        cond.clause()
    );
    let out = rows(pool, &sql, &cond.binds)
        .await?
        .iter()
        .map(|r| {
            Ok(json!({
                "name": r.try_get::<Option<String>, _>("name")?.unwrap_or_else(|| "N/A".into()),
                "value": r.try_get::<i64, _>("value")?,
            }))
        })
        .collect::<Result<Vec<Value>, sqlx::Error>>()?;
    Ok(out)
}

async fn kecamatan_map(pool: &MySqlPool) -> Result<HashMap<i64, String>, sqlx::Error> {
    let mut out = HashMap::new();
    for r in rows(
        pool,
        "SELECT CAST(id AS SIGNED) AS id, n_kec FROM tbl_kecamatan",
        &[],
    )
    .await?
    {
        out.insert(r.try_get("id")?, r.try_get("n_kec")?);
    }
    Ok(out)
}

/// Nama desa; `n_desa` NULL disimpan sebagai `N/A` (seperti `?? 'N/A'`).
async fn desa_map(pool: &MySqlPool) -> Result<HashMap<i64, String>, sqlx::Error> {
    let mut out = HashMap::new();
    for r in rows(
        pool,
        "SELECT CAST(id AS SIGNED) AS id, n_desa FROM tbl_desa",
        &[],
    )
    .await?
    {
        let name = r.try_get::<Option<String>, _>("n_desa")?.unwrap_or_else(|| "N/A".into());
        out.insert(r.try_get("id")?, name);
    }
    Ok(out)
}

async fn penyedia_map(pool: &MySqlPool) -> Result<HashMap<i64, String>, sqlx::Error> {
    let mut out = HashMap::new();
    for r in rows(
        pool,
        "SELECT CAST(id AS SIGNED) AS id, nama FROM tbl_penyedia",
        &[],
    )
    .await?
    {
        out.insert(r.try_get("id")?, r.try_get("nama")?);
    }
    Ok(out)
}

/// `GET /api/dashboard/executive-progress`: tren bulanan fisik, rencana, dan SP2D.
pub async fn executive_progress(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let p = php::Params::parse(raw.as_deref());
    let tahun: i64 = match p.get("tahun") {
        Some(v) => php::intval_str(v),
        None => i64::from(Utc::now().year()),
    };
    let scope = Scope {
        tahun: Some(Bind::Int(tahun)),
        kecamatan: kecamatan_ints(&p),
        tag: None,
    };
    let data = build_executive(&state.pool, &p, tahun, &scope)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "success": true, "data": data })))
}

async fn build_executive(
    pool: &MySqlPool,
    p: &php::Params,
    tahun: i64,
    scope: &Scope,
) -> Result<Value, sqlx::Error> {
    let empty = json!({ "monthly_trend": [], "totals": { "keuangan_total": 0 } });
    let base = scope.pekerjaan(true);
    let active_ids: Vec<i64> = rows(
        pool,
        &format!("SELECT CAST(tbl_pekerjaan.id AS SIGNED) AS id FROM tbl_pekerjaan WHERE {}", base.clause()),
        &base.binds,
    )
    .await?
    .iter()
    .map(|r| r.try_get::<i64, _>("id"))
    .collect::<Result<_, _>>()?;

    let ids: Vec<i64> = match p.get("pekerjaan_ids").filter(|v| php::truthy(Some(v))) {
        Some(raw) => {
            let requested: Vec<i64> = raw.split(',').map(php::intval_str).collect();
            active_ids.into_iter().filter(|id| requested.contains(id)).collect()
        }
        None => active_ids,
    };
    if ids.is_empty() {
        return Ok(empty);
    }

    let mut binds: Vec<Bind> = ids.iter().map(|i| Bind::Int(*i)).collect();
    binds.push(Bind::Int(tahun));
    let history = rows(
        pool,
        &format!(
            "SELECT CAST(h.pekerjaan_id AS SIGNED) AS pid, h.tipe AS tipe, h.jenis AS jenis, h.tanggal AS tgl, \
             CAST(h.persen AS DOUBLE) AS persen, CAST(h.nilai AS DOUBLE) AS nilai \
             FROM pekerjaan_progress_estimasi_history h \
             WHERE h.pekerjaan_id IN ({}) AND YEAR(h.tanggal) = ? \
             ORDER BY h.tanggal DESC, h.id DESC",
            placeholders(ids.len())
        ),
        &binds,
    )
    .await?;

    // Realisasi/rencana fisik: entri pertama per (paket, bulan) dipakai (urutan terbaru dulu).
    let mut latest_fisik: BTreeMap<i64, BTreeMap<u32, f64>> = BTreeMap::new();
    let mut latest_rencana: BTreeMap<i64, BTreeMap<u32, f64>> = BTreeMap::new();
    let mut sp2d_by_month: BTreeMap<u32, f64> = BTreeMap::new();
    for r in &history {
        let pid: i64 = r.try_get("pid")?;
        let tipe: String = r.try_get("tipe")?;
        let jenis: String = r.try_get("jenis")?;
        let tgl: NaiveDate = r.try_get("tgl")?;
        let m = tgl.month();
        let persen = r.try_get::<Option<f64>, _>("persen")?.unwrap_or(0.0);
        if tipe == "realisasi" && jenis == "fisik" {
            latest_fisik.entry(pid).or_default().entry(m).or_insert(persen);
        } else if tipe == "rencana" && jenis == "fisik" {
            latest_rencana.entry(pid).or_default().entry(m).or_insert(persen);
        } else if tipe == "realisasi" && jenis == "keuangan" {
            let nilai = r.try_get::<Option<f64>, _>("nilai")?.unwrap_or(0.0);
            *sp2d_by_month.entry(m).or_insert(0.0) += nilai;
        }
    }

    // Carry forward per paket: bulan tanpa data memakai nilai bulan sebelumnya.
    let (fisik_sum, fisik_count) = carry_forward(&latest_fisik);
    let (rencana_sum, rencana_count) = carry_forward(&latest_rencana);

    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des",
    ];
    let mut monthly = Vec::with_capacity(12);
    for m in 1..=12u32 {
        let i = (m - 1) as usize;
        let fisik_avg = if fisik_count[i] > 0 {
            json!(php::round(fisik_sum[i] / fisik_count[i] as f64, 1))
        } else {
            json!(0)
        };
        let rencana_avg = if rencana_count[i] > 0 {
            json!(php::round(rencana_sum[i] / rencana_count[i] as f64, 1))
        } else {
            json!(0)
        };
        monthly.push(json!({
            "month": month_names[i],
            "fisik_avg": fisik_avg,
            "rencana_avg": rencana_avg,
            "keuangan_sum": php::round(sp2d_by_month.get(&m).copied().unwrap_or(0.0), 0),
        }));
    }
    let total: f64 = sp2d_by_month.values().sum();
    Ok(json!({
        "monthly_trend": monthly,
        "totals": { "keuangan_total": php::round(total, 0) },
    }))
}

/// Jumlah dan banyaknya paket per bulan (indeks 0 = Januari) dengan carry-forward per paket.
fn carry_forward(per_pekerjaan: &BTreeMap<i64, BTreeMap<u32, f64>>) -> ([f64; 12], [i64; 12]) {
    let mut sum = [0.0f64; 12];
    let mut count = [0i64; 12];
    for by_month in per_pekerjaan.values() {
        let mut prev: Option<f64> = None;
        for m in 1..=12u32 {
            if let Some(v) = by_month.get(&m) {
                prev = Some(*v);
            }
            if let Some(v) = prev {
                let i = (m - 1) as usize;
                sum[i] += v;
                count[i] += 1;
            }
        }
    }
    (sum, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carry_forward_keeps_previous_month_value() {
        let mut m = BTreeMap::new();
        m.insert(1i64, BTreeMap::from([(3u32, 40.0), (6u32, 70.0)]));
        let (sum, count) = carry_forward(&m);
        assert_eq!(count[0], 0, "Januari belum ada data");
        assert_eq!(count[2], 1, "Maret berisi 40");
        assert_eq!(sum[4], 40.0, "Mei terbawa dari Maret");
        assert_eq!(sum[5], 70.0, "Juni memakai 70");
        assert_eq!(count[11], 1, "Desember terbawa");
    }

    #[test]
    fn kecamatan_raw_accepts_csv_and_array() {
        let p = php::Params::parse(Some("kecamatan_ids=1,2,"));
        assert_eq!(kecamatan_raw(&p), vec!["1", "2", ""]);
        let p = php::Params::parse(Some("kecamatan_ids=0"));
        assert!(kecamatan_raw(&p).is_empty(), "\"0\" falsy di PHP");
        let p = php::Params::parse(Some("kecamatan_ids[]=3&kecamatan_ids[]=4"));
        assert_eq!(kecamatan_raw(&p), vec!["3", "4"]);
        assert!(kecamatan_ints(&php::Params::parse(Some("kecamatan_ids=,"))).is_none());
    }

    #[test]
    fn tag_zero_means_no_filter() {
        let p = php::Params::parse(Some("tag_id=0"));
        assert!(stats_scope(&p).tag.is_none());
        let p = php::Params::parse(Some("tag_id=abc"));
        assert!(stats_scope(&p).tag.is_none());
        let p = php::Params::parse(Some("tag_id=7"));
        assert_eq!(stats_scope(&p).tag, Some(7));
    }
}
