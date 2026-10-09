//! `GET /api/pekerjaan` dan `GET /api/pekerjaan/{id}` (hanya GET), setara `PekerjaanController@index`/`@show`.
//!
//! Dibatasi ke role yang di Laravel melihat semua data (`admin`, `manager`, `super-admin`, `operator`).
//! Role lain mendapat 403 karena scope RLS pengawas belum dipindah.
//!
//! BELUM DIPINDAH (dikirim sebagai null/kosong, dan respon diberi header `x-partial-response`):
//! progres (`progress_*`, `deviasi*`), foto (`foto_*`), kontrak (`kontrak`, `has_kontrak`, `kontrak_count`),
//! `assignment_sources`, pencarian lewat `kontrak.penyedia`, dan sort `penerima_count`.

use axum::{
    extract::{Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};
use std::collections::HashMap;

use crate::{
    desa::{self, internal},
    format::{iso8601_utc, number_like_php},
    kecamatan, kegiatan,
    lookup::carbon_json,
    pagination::{self, PageParams},
    require_auth, AppState,
};

pub const FULL_ACCESS_ROLES: &[&str] = &["admin", "manager", "super-admin", "operator"];

const SORTABLE: &[&str] = &[
    "id",
    "nama_paket",
    "kode_rekening",
    "pagu",
    "created_at",
    "updated_at",
];

/// Apakah role user boleh mengakses daftar penuh di Rust saat ini.
pub fn has_full_access(roles: &[String]) -> bool {
    roles
        .iter()
        .any(|r| FULL_ACCESS_ROLES.contains(&r.as_str()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct PekerjaanRow {
    pub id: u64,
    pub kode_rekening: Option<String>,
    pub nama_paket: Option<String>,
    pub pagu: Option<f64>,
    pub is_konsultan: bool,
    pub status: Option<String>,
    pub catatan: Option<String>,
    pub kecamatan_id: Option<i64>,
    pub desa_id: Option<i64>,
    pub kegiatan_id: Option<i64>,
    pub pengawas_id: Option<u64>,
    pub pendamping_id: Option<u64>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

const COLS: &str = "p.id, p.kode_rekening, p.nama_paket, p.pagu, p.is_konsultan, p.status, p.catatan, \
    p.kecamatan_id, p.desa_id, p.kegiatan_id, p.pengawas_id, p.pendamping_id, p.created_at, p.updated_at";

fn map_row(r: &sqlx::mysql::MySqlRow) -> Result<PekerjaanRow, sqlx::Error> {
    let is_konsultan: Option<i64> = r.try_get("is_konsultan")?;
    Ok(PekerjaanRow {
        id: r.try_get("id")?,
        kode_rekening: r.try_get("kode_rekening")?,
        nama_paket: r.try_get("nama_paket")?,
        pagu: r.try_get("pagu")?,
        is_konsultan: is_konsultan.unwrap_or(0) != 0,
        status: r.try_get("status")?,
        catatan: r.try_get("catatan")?,
        kecamatan_id: r.try_get("kecamatan_id")?,
        desa_id: r.try_get("desa_id")?,
        kegiatan_id: r.try_get("kegiatan_id")?,
        pengawas_id: r.try_get("pengawas_id")?,
        pendamping_id: r.try_get("pendamping_id")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

/// `trim()` PHP: spasi, tab, baris baru, CR, NUL, dan vertical tab.
fn php_trim(s: &str) -> &str {
    s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\0' | '\x0B'))
}

/// `!empty()` PHP untuk string: "" dan "0" dianggap kosong.
pub fn php_not_empty(s: &str) -> bool {
    !(s.is_empty() || s == "0")
}

/// `filter_var(.., FILTER_VALIDATE_BOOLEAN)` (dipakai `$request->boolean()`): "1", "true", "on", "yes".
pub fn php_bool(s: &str) -> bool {
    matches!(
        php_trim(s).to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes"
    )
}

/// `(int)` PHP untuk string: awalan numerik (termasuk bentuk `1.9` dan `1e2`), selain itu 0.
pub fn php_int(s: &str) -> i64 {
    let t = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C']);
    let b = t.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+' | b'-')));
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut has_digits = i > int_start;
    let mut is_float = false;
    if i < b.len() && b[i] == b'.' {
        let frac_start = i + 1;
        let mut j = frac_start;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if has_digits || j > frac_start {
            has_digits = true;
            is_float = true;
            i = j;
        }
    }
    if !has_digits {
        return 0;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            is_float = true;
            i = j;
        }
    }
    let num = &t[..i];
    if is_float {
        num.parse::<f64>().map(|f| f as i64).unwrap_or(0)
    } else {
        num.parse::<i64>().unwrap_or(if num.starts_with('-') {
            i64::MIN
        } else {
            i64::MAX
        })
    }
}

/// `filter_var(.., FILTER_VALIDATE_INT)` (dipakai `Paginator` untuk `page`): bilangan bulat desimal tanpa nol di depan.
pub fn php_filter_int(s: &str) -> Option<i64> {
    let t = php_trim(s);
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    let valid = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    if valid {
        t.parse::<i64>().ok()
    } else {
        None
    }
}

#[derive(Debug, Default, Clone)]
pub struct PekerjaanFilter {
    pub tahun: Option<String>,
    pub kecamatan_id: Option<String>,
    pub desa_id: Option<String>,
    pub kegiatan_id: Option<String>,
    pub nama_sub_kegiatan: Option<String>,
    pub sub_bidang: Option<String>,
    pub search: Option<String>,
    /// `status` selain `all`; `active` berarti tanpa yang dibatalkan.
    pub status: Option<String>,
    /// Ada di query (`has`): nilai dibaca dengan `boolean()`.
    pub is_konsultan: Option<bool>,
    pub pengawas_id: Option<String>,
    pub pendamping_id: Option<String>,
    pub tag_id: Option<String>,
    pub sort_by: String,
    pub sort_desc: bool,
}

impl PekerjaanFilter {
    /// Parameter index Laravel. `has() && !empty()` memakai `php_not_empty` (""/"0" diabaikan);
    /// `filled()` memakai trim, dan nilainya dikirim apa adanya.
    pub fn from_query(q: &HashMap<String, String>) -> Self {
        let nonempty = |k: &str| q.get(k).filter(|v| php_not_empty(v)).cloned();
        let filled = |k: &str| q.get(k).filter(|v| !php_trim(v).is_empty()).cloned();
        let sort_by = q.get("sort_by").cloned().unwrap_or_default();
        let sort_dir = q.get("sort_direction").map(|s| s.to_lowercase());
        Self {
            tahun: nonempty("tahun"),
            kecamatan_id: nonempty("kecamatan_id"),
            desa_id: nonempty("desa_id"),
            kegiatan_id: nonempty("kegiatan_id"),
            nama_sub_kegiatan: filled("nama_sub_kegiatan"),
            sub_bidang: filled("sub_bidang"),
            search: filled("search")
                .map(|s| php_trim(&s).to_string())
                .filter(|s| !s.is_empty()),
            status: filled("status").filter(|s| s != "all"),
            is_konsultan: q.get("is_konsultan").map(|v| php_bool(v)),
            pengawas_id: nonempty("pengawas_id"),
            pendamping_id: nonempty("pendamping_id"),
            tag_id: nonempty("tag_id"),
            sort_desc: sort_dir.as_deref() != Some("asc"),
            sort_by,
        }
    }

    /// WHERE dinamis dan nilai bind berurutan.
    fn where_clause(&self) -> (String, Vec<String>) {
        let mut sql = String::from(" WHERE 1=1");
        let mut b: Vec<String> = Vec::new();
        if let Some(v) = &self.kecamatan_id {
            sql.push_str(" AND p.kecamatan_id = ?");
            b.push(v.clone());
        }
        if let Some(v) = &self.desa_id {
            sql.push_str(" AND p.desa_id = ?");
            b.push(v.clone());
        }
        if let Some(v) = &self.kegiatan_id {
            sql.push_str(" AND p.kegiatan_id = ?");
            b.push(v.clone());
        }
        if let Some(v) = &self.tahun {
            sql.push_str(
                " AND p.kegiatan_id IN (SELECT id FROM tbl_kegiatan WHERE tahun_anggaran = ?)",
            );
            b.push(v.clone());
        }
        if let Some(v) = &self.nama_sub_kegiatan {
            sql.push_str(
                " AND p.kegiatan_id IN (SELECT id FROM tbl_kegiatan WHERE nama_sub_kegiatan = ?)",
            );
            b.push(v.clone());
        }
        if let Some(v) = &self.sub_bidang {
            sql.push_str(
                " AND p.kegiatan_id IN (SELECT id FROM tbl_kegiatan WHERE sub_bidang = ?)",
            );
            b.push(v.clone());
        }
        if let Some(v) = &self.pengawas_id {
            sql.push_str(" AND p.pengawas_id = ?");
            b.push(v.clone());
        }
        if let Some(v) = &self.pendamping_id {
            sql.push_str(" AND p.pendamping_id = ?");
            b.push(v.clone());
        }
        if let Some(v) = &self.tag_id {
            // whereHas('tags', tbl_tags.id = ?): tag harus ada di tbl_tags.
            sql.push_str(
                " AND p.id IN (SELECT pt.pekerjaan_id FROM pekerjaan_tag pt \
                 JOIN tbl_tags t ON t.id = pt.tag_id WHERE t.id = ?)",
            );
            b.push(v.clone());
        }
        if let Some(v) = self.is_konsultan {
            sql.push_str(" AND p.is_konsultan = ?");
            b.push(if v { "1" } else { "0" }.to_string());
        }
        // notCanceled(): status NULL dihitung aktif.
        match self.status.as_deref() {
            Some("active") => sql.push_str(" AND (p.status IS NULL OR p.status != 'canceled')"),
            Some(s) => {
                sql.push_str(" AND p.status = ?");
                b.push(s.to_string());
            }
            None => {}
        }
        if let Some(s) = &self.search {
            let like = format!("%{s}%");
            sql.push_str(
                " AND (p.nama_paket LIKE ? OR p.kode_rekening LIKE ? \
                 OR p.desa_id IN (SELECT id FROM tbl_desa WHERE n_desa LIKE ?) \
                 OR p.kecamatan_id IN (SELECT id FROM tbl_kecamatan WHERE n_kec LIKE ?) \
                 OR p.pengawas_id IN (SELECT id FROM pengawas WHERE nama LIKE ?) \
                 OR p.id IN (SELECT kp.pekerjaan_id FROM kontrak_pekerjaan kp \
                   JOIN tbl_kontrak k ON k.id = kp.kontrak_id \
                   JOIN tbl_penyedia py ON py.id = k.id_penyedia WHERE py.nama LIKE ?))",
            );
            for _ in 0..6 {
                b.push(like.clone());
            }
        }
        (sql, b)
    }

    fn order_sql(&self) -> String {
        let dir = if self.sort_desc { "DESC" } else { "ASC" };
        if self.sort_by == "penerima_count" {
            format!(" ORDER BY (SELECT COUNT(*) FROM tbl_penerima x WHERE x.pekerjaan_id = p.id) {dir}, p.id {dir}")
        } else if SORTABLE.contains(&self.sort_by.as_str()) {
            format!(" ORDER BY p.{} {dir}", self.sort_by)
        } else {
            " ORDER BY p.created_at DESC".to_string()
        }
    }
}

/// Daftar pekerjaan dan total. `page = None` berarti tanpa paginasi (dibatasi `cap` baris).
pub async fn list(
    pool: &MySqlPool,
    f: &PekerjaanFilter,
    scope: &crate::access::Restriction,
    page: Option<(u64, u64)>,
    cap: Option<u64>,
) -> Result<(Vec<PekerjaanRow>, u64), sqlx::Error> {
    let (mut where_sql, mut binds) = f.where_clause();
    // `scopeByUserRole()`: alias tabel di query ini adalah `p`.
    where_sql.push_str(&scope.sql);
    binds.extend(scope.binds.iter().map(u64::to_string));
    let count_sql = format!("SELECT COUNT(*) FROM tbl_pekerjaan p{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b);
    }
    let total = cq.fetch_one(pool).await? as u64;

    let mut sql = format!(
        "SELECT {COLS} FROM tbl_pekerjaan p{where_sql}{}",
        f.order_sql()
    );
    if page.is_some() {
        sql.push_str(" LIMIT ? OFFSET ?");
    } else if let Some(cap) = cap {
        sql.push_str(&format!(" LIMIT {cap}"));
    }
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    if let Some((limit, offset)) = page {
        q = q.bind(limit).bind(offset);
    }
    let rows = q.fetch_all(pool).await?;
    Ok((rows.iter().map(map_row).collect::<Result<_, _>>()?, total))
}

pub async fn find(pool: &MySqlPool, id: u64) -> Result<Option<PekerjaanRow>, sqlx::Error> {
    let row = sqlx::query(&format!(
        "SELECT {COLS} FROM tbl_pekerjaan p WHERE p.id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| map_row(&r)).transpose()
}

/// Relasi yang dimuat untuk satu halaman, dikumpulkan sekali (bukan per baris).
pub struct Loaded {
    pub kecamatan: HashMap<i64, Value>,
    pub desa: HashMap<i64, Value>,
    pub kegiatan: HashMap<i64, Value>,
    pub pengawas: HashMap<u64, Value>,
    pub tags: HashMap<u64, Vec<Value>>,
    /// Metrik progres dari `tbl_progress.content`. Tidak ada entri = progres belum dicatat (0).
    pub progress: HashMap<u64, crate::progress_metrics::Metrics>,
    /// Estimasi per pekerjaan. Terisi hanya bila `summary` aktif (`None` = tidak dimuat, key null).
    pub estimasi: Option<HashMap<u64, crate::progress_estimasi::Summary>>,
    pub counts: HashMap<u64, crate::pekerjaan_rel::Counts>,
    pub kontrak: HashMap<u64, Vec<Value>>,
    pub sources: HashMap<u64, Vec<&'static str>>,
    /// `output` dan `foto` dimuat hanya pada daftar `summary` yang terpaginasi.
    pub output_loaded: bool,
    pub outputs: HashMap<u64, Vec<crate::pekerjaan_rel::OutputRow>>,
    pub foto_groups: HashMap<u64, Vec<crate::pekerjaan_rel::FotoGroup>>,
    pub mode: Mode,
    /// Relasi tags dan kontrak ikut dimuat (Laravel: tidak dimuat pada `per_page=-1` tanpa summary).
    pub tags_loaded: bool,
    pub kontrak_loaded: bool,
}

impl Loaded {
    /// Relasi kosong, untuk tes unit.
    pub fn empty(mode: Mode) -> Self {
        Self {
            kecamatan: HashMap::new(),
            desa: HashMap::new(),
            kegiatan: HashMap::new(),
            pengawas: HashMap::new(),
            tags: HashMap::new(),
            progress: HashMap::new(),
            estimasi: None,
            counts: HashMap::new(),
            kontrak: HashMap::new(),
            sources: HashMap::new(),
            output_loaded: mode.summary && !mode.unbounded,
            outputs: HashMap::new(),
            foto_groups: HashMap::new(),
            mode,
            tags_loaded: !mode.unbounded || mode.summary,
            kontrak_loaded: !mode.unbounded || mode.summary,
        }
    }
}

/// Mode permintaan: `summary` (`$request->boolean('summary')`) dan `unbounded` (`per_page=-1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub summary: bool,
    pub unbounded: bool,
}

pub async fn load(
    pool: &MySqlPool,
    rows: &[PekerjaanRow],
    mode: Mode,
    viewer: &crate::pekerjaan_rel::Viewer,
) -> Result<Loaded, sqlx::Error> {
    let mut kec_ids: Vec<i64> = rows.iter().filter_map(|r| r.kecamatan_id).collect();
    let mut desa_ids: Vec<i64> = rows.iter().filter_map(|r| r.desa_id).collect();
    let mut keg_ids: Vec<i64> = rows.iter().filter_map(|r| r.kegiatan_id).collect();
    let mut peng_ids: Vec<u64> = rows
        .iter()
        .flat_map(|r| [r.pengawas_id, r.pendamping_id])
        .flatten()
        .collect();
    let pekerjaan_ids: Vec<u64> = rows.iter().map(|r| r.id).collect();
    for v in [&mut kec_ids, &mut desa_ids, &mut keg_ids] {
        v.sort_unstable();
        v.dedup();
    }
    peng_ids.sort_unstable();
    peng_ids.dedup();

    let mut out = Loaded {
        kecamatan: HashMap::new(),
        desa: HashMap::new(),
        kegiatan: HashMap::new(),
        pengawas: HashMap::new(),
        tags: HashMap::new(),
        progress: if mode.unbounded {
            HashMap::new()
        } else {
            progress_for(pool, &pekerjaan_ids).await?
        },
        estimasi: if mode.summary {
            Some(estimasi_for(pool, rows).await?)
        } else {
            None
        },
        counts: HashMap::new(),
        kontrak: HashMap::new(),
        sources: HashMap::new(),
        output_loaded: mode.summary && !mode.unbounded,
        outputs: HashMap::new(),
        foto_groups: HashMap::new(),
        mode,
        tags_loaded: !mode.unbounded || mode.summary,
        kontrak_loaded: !mode.unbounded || mode.summary,
    };

    for id in &kec_ids {
        if let Some(k) = kecamatan::find(pool, *id as u64).await? {
            out.kecamatan.insert(*id, kecamatan::to_resource(&k));
        }
    }
    for id in &desa_ids {
        if let Some(d) = desa::find(pool, *id as u64).await? {
            // Relasi desa dimuat tanpa kecamatan di index Pekerjaan.
            out.desa.insert(*id, desa::base_resource(&d));
        }
    }
    for id in &keg_ids {
        if let Some(k) = kegiatan::find(pool, *id as u64).await? {
            out.kegiatan.insert(*id, kegiatan::to_resource(&k));
        }
    }
    for id in &peng_ids {
        if let Some(p) = pengawas_resource(pool, *id).await? {
            out.pengawas.insert(*id, p);
        }
    }
    for pid in pekerjaan_ids
        .iter()
        .filter(|_| !mode.unbounded || mode.summary)
    {
        let rows = sqlx::query(
            "SELECT t.id, t.name, t.slug, t.color, t.created_at, t.updated_at FROM pekerjaan_tag pt \
             JOIN tbl_tags t ON t.id = pt.tag_id WHERE pt.pekerjaan_id = ? ORDER BY t.name",
        )
        .bind(pid)
        .fetch_all(pool)
        .await?;
        let mut tags = Vec::with_capacity(rows.len());
        for r in &rows {
            let tag = crate::lookup::TagRow {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                slug: r.try_get("slug")?,
                color: r.try_get("color")?,
                created_at: r.try_get("created_at")?,
                updated_at: r.try_get("updated_at")?,
            };
            tags.push(crate::lookup::tag_resource(&tag));
        }
        out.tags.insert(*pid, tags);
    }
    for p in rows {
        out.counts
            .insert(p.id, crate::pekerjaan_rel::counts_for(pool, p.id).await?);
        if out.kontrak_loaded {
            out.kontrak.insert(
                p.id,
                crate::pekerjaan_rel::kontrak_items(pool, p.id, !mode.unbounded, mode.summary)
                    .await?,
            );
        }
        if out.output_loaded {
            out.outputs
                .insert(p.id, crate::pekerjaan_rel::outputs_for(pool, p.id).await?);
            out.foto_groups.insert(
                p.id,
                crate::pekerjaan_rel::foto_groups_for(pool, p.id).await?,
            );
        }
        let pengawas_nip = p
            .pengawas_id
            .and_then(|id| out.pengawas.get(&id))
            .and_then(|v| v["nip"].as_str().map(str::to_string));
        let pendamping_nip = p
            .pendamping_id
            .and_then(|id| out.pengawas.get(&id))
            .and_then(|v| v["nip"].as_str().map(str::to_string));
        let src = crate::pekerjaan_rel::assignment_sources(
            pool,
            viewer,
            p.id,
            p.kegiatan_id,
            pengawas_nip.as_deref(),
            pendamping_nip.as_deref(),
        )
        .await?;
        out.sources.insert(p.id, src);
    }
    Ok(out)
}

/// Estimasi per pekerjaan untuk tahun anggaran kegiatannya (atau tahun sekarang bila kosong).
pub async fn estimasi_for(
    pool: &MySqlPool,
    rows: &[PekerjaanRow],
) -> Result<HashMap<u64, crate::progress_estimasi::Summary>, sqlx::Error> {
    use crate::progress_estimasi::{summarize, HistoryRow};
    let mut out = HashMap::new();
    for p in rows {
        let tahun_raw: Option<String> = match p.kegiatan_id {
            Some(kid) => sqlx::query_scalar("SELECT tahun_anggaran FROM tbl_kegiatan WHERE id = ?")
                .bind(kid as u64)
                .fetch_optional(pool)
                .await?
                .flatten(),
            None => None,
        };
        // (int) di PHP: angka awal, selain itu 0. Tanpa kegiatan: tahun sekarang.
        let tahun = match tahun_raw {
            Some(t) => t.trim().parse::<i64>().unwrap_or(0),
            None => chrono::Datelike::year(&chrono::Utc::now()) as i64,
        };
        let hist = sqlx::query(
            "SELECT id, CAST(tahun_anggaran AS SIGNED) AS tahun_anggaran, jenis, tipe, tanggal, CAST(persen AS DOUBLE) AS persen, \
             CAST(nilai AS DOUBLE) AS nilai FROM pekerjaan_progress_estimasi_history WHERE pekerjaan_id = ?",
        )
        .bind(p.id)
        .fetch_all(pool)
        .await?;
        let mut items = Vec::with_capacity(hist.len());
        for r in &hist {
            items.push(HistoryRow {
                id: r.try_get("id")?,
                tahun_anggaran: r.try_get::<i64, _>("tahun_anggaran")?,
                jenis: r.try_get("jenis")?,
                tipe: r.try_get("tipe")?,
                tanggal: r.try_get("tanggal")?,
                persen: r.try_get("persen")?,
                nilai: r.try_get("nilai")?,
            });
        }
        out.insert(p.id, summarize(&items, tahun));
    }
    Ok(out)
}

/// Metrik progres per pekerjaan dari baris `tbl_progress` pertama (urut id).
pub async fn progress_for(
    pool: &MySqlPool,
    ids: &[u64],
) -> Result<HashMap<u64, crate::progress_metrics::Metrics>, sqlx::Error> {
    let mut out = HashMap::new();
    for id in ids {
        let row = sqlx::query(
            "SELECT CAST(content AS CHAR) AS content FROM tbl_progress WHERE pekerjaan_id = ? ORDER BY id LIMIT 1",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        if let Some(r) = row {
            let raw: Option<String> = r.try_get("content")?;
            let content = raw.and_then(|s| serde_json::from_str::<Value>(&s).ok());
            out.insert(*id, crate::progress_metrics::summarize(content.as_ref()));
        }
    }
    Ok(out)
}

/// `PengawasResource`: `jumlah_lokasi` dan `total_pagu` dihitung dari pekerjaan dengan `pengawas_id` ini.
pub async fn pengawas_resource(pool: &MySqlPool, id: u64) -> Result<Option<Value>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT p.id, p.nama, p.nip, p.jabatan, p.telepon, p.created_at, p.updated_at, \
         (SELECT COUNT(*) FROM tbl_pekerjaan x WHERE x.pengawas_id = p.id) AS jumlah_lokasi, \
         (SELECT COALESCE(SUM(x.pagu), 0) FROM tbl_pekerjaan x WHERE x.pengawas_id = p.id) AS total_pagu \
         FROM pengawas p WHERE p.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else { return Ok(None) };
    let created: Option<DateTime<Utc>> = r.try_get("created_at")?;
    let updated: Option<DateTime<Utc>> = r.try_get("updated_at")?;
    let total: f64 = r.try_get::<f64, _>("total_pagu").unwrap_or(0.0);
    Ok(Some(json!({
        "id": r.try_get::<u64, _>("id")?,
        "nama": r.try_get::<Option<String>, _>("nama")?,
        "nip": r.try_get::<Option<String>, _>("nip")?,
        "jabatan": r.try_get::<Option<String>, _>("jabatan")?,
        "telepon": r.try_get::<Option<String>, _>("telepon")?,
        "jumlah_lokasi": r.try_get::<i64, _>("jumlah_lokasi")?,
        "total_pagu": number_like_php(total),
        "created_at": carbon_json(created),
        "updated_at": carbon_json(updated),
    })))
}

/// `PekerjaanResource` untuk index dan show. Bagian yang belum dipindah dikirim sebagai null/kosong.
pub fn to_resource(p: &PekerjaanRow, rel: &Loaded) -> Value {
    let kec_key = p
        .kecamatan_id
        .and_then(|id| rel.kecamatan.get(&id).cloned());
    let desa_key = p.desa_id.and_then(|id| rel.desa.get(&id).cloned());
    let keg_key = p.kegiatan_id.and_then(|id| rel.kegiatan.get(&id).cloned());
    let progress = rel.progress.get(&p.id);
    let counts = rel.counts.get(&p.id).copied().unwrap_or_default();
    // Cabang lengkap bila relasi output dan foto dimuat; selain itu cabang parsial.
    let foto_full = rel.output_loaded.then(|| {
        crate::pekerjaan_rel::foto_full(
            rel.outputs.get(&p.id).map_or(&[], Vec::as_slice),
            rel.foto_groups.get(&p.id).map_or(&[], Vec::as_slice),
            counts.foto,
        )
    });
    let estimasi = match &rel.estimasi {
        Some(map) => map
            .get(&p.id)
            .map_or_else(empty_estimasi, |s| s.resource_fields()),
        None => empty_estimasi(),
    };
    let peng = p.pengawas_id.and_then(|id| rel.pengawas.get(&id).cloned());
    let pend = p
        .pendamping_id
        .and_then(|id| rel.pengawas.get(&id).cloned());
    let mut v = json!({
        "id": p.id,
        "kode_rekening": p.kode_rekening,
        "nama_paket": p.nama_paket,
        "pagu": p.pagu.map(number_like_php),
        "is_konsultan": p.is_konsultan,
        // `$this->status ?: 'active'`: string kosong dan "0" dianggap falsy.
        "status": p.status.clone().filter(|s| !s.is_empty() && s != "0").unwrap_or_else(|| "active".to_string()),
        "catatan": p.catatan,
        // BELUM DIPINDAH: null, bukan nilai yang salah.
        "has_kontrak": counts.has_kontrak(),
        "kontrak_count": counts.kontrak_count(),
        "progress_total": number_like_php(progress.map_or(0.0, |m| m.progress_total)),
        "deviasi": number_like_php(progress.map_or(0.0, |m| m.deviasi)),
        "progress_estimasi_fisik": estimasi["progress_estimasi_fisik"],
        "progress_estimasi_keuangan": estimasi["progress_estimasi_keuangan"],
        "progress_estimasi_keuangan_nilai": estimasi["progress_estimasi_keuangan_nilai"],
        "deviasi_estimasi_fisik": estimasi["deviasi_estimasi_fisik"],
        "deviasi_estimasi_keuangan": estimasi["deviasi_estimasi_keuangan"],
        "foto_count": counts.foto,
        "foto_required_count": foto_full.map_or(Value::Null, |f| json!(f.required)),
        "foto_status": foto_full.map_or_else(|| json!(counts.foto_status()), |f| json!(f.status)),
        "kecamatan_id": p.kecamatan_id,
        "desa_id": p.desa_id,
        "kegiatan_id": p.kegiatan_id,
        "pengawas_id": p.pengawas_id,
        "pendamping_id": p.pendamping_id,
        "assignment_sources": rel.sources.get(&p.id).cloned().unwrap_or_default(),
        "kecamatan": kec_key,
        "desa": desa_key,
        "kegiatan": keg_key,
        "pengawas": peng,
        "pendamping": pend,
        "tags": rel.tags.get(&p.id).cloned().unwrap_or_default(),
        "kontrak": rel.kontrak.get(&p.id).cloned().unwrap_or_default(),
        "penerima_count": counts.penerima,
        "sipd_links_count": counts.sipd_links,
        "created_at": iso8601_utc(p.created_at),
        "updated_at": iso8601_utc(p.updated_at),
    });
    if !rel.tags_loaded {
        v.as_object_mut().map(|m| m.remove("tags"));
    }
    if !rel.kontrak_loaded {
        v.as_object_mut().map(|m| m.remove("kontrak"));
    }
    if rel.output_loaded {
        let outputs = rel.outputs.get(&p.id).map_or_else(Vec::new, |o| {
            o.iter()
                .map(crate::pekerjaan_rel::OutputRow::to_resource)
                .collect()
        });
        v["output"] = Value::Array(outputs);
    }
    v
}

/// Estimasi belum dimuat (summary tidak aktif): semua key null.
fn empty_estimasi() -> Value {
    json!({
        "progress_estimasi_fisik": null,
        "progress_estimasi_keuangan": null,
        "progress_estimasi_keuangan_nilai": null,
        "deviasi_estimasi_fisik": null,
        "deviasi_estimasi_keuangan": null,
    })
}

/// `summary` di Laravel: `$request->boolean('summary')`.
pub fn summary_requested(query: &HashMap<String, String>) -> bool {
    query.get("summary").is_some_and(|v| php_bool(v))
}

/// Pasangan query seperti `parse_str` PHP: kunci dan nilai di-decode, urutan kunci pertama dipertahankan,
/// nilai terakhir menang. Kunci dengan `[` (array PHP) belum didukung.
pub fn laravel_query_pairs(raw: Option<&str>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for kv in raw.unwrap_or("").split('&').filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        // PHP mengganti spasi dan titik di nama kunci dengan `_`.
        let k = url_decode(k).replace([' ', '.'], "_");
        if k.is_empty() {
            continue;
        }
        let v = url_decode(v);
        match out.iter_mut().find(|(ek, _)| *ek == k) {
            Some(slot) => slot.1 = v,
            None => out.push((k, v)),
        }
    }
    out
}

/// Url halaman `n` seperti `Paginator::url()`: `array_merge($query, [page => n])` dengan `page` di posisi
/// aslinya (ditambahkan di akhir bila belum ada), lalu `http_build_query(.., RFC3986)`.
pub fn laravel_page_url(base: &str, pairs: &[(String, String)], page: u64) -> String {
    let mut parts = pairs.to_vec();
    match parts.iter_mut().find(|(k, _)| k == "page") {
        Some(slot) => slot.1 = page.to_string(),
        None => parts.push(("page".to_string(), page.to_string())),
    }
    let query = parts
        .iter()
        .map(|(k, v)| format!("{}={}", rfc3986(k), rfc3986(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{query}")
}

/// `urldecode()` PHP: `+` menjadi spasi, `%XX` didecode, sisanya dibiarkan.
fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Query string tanpa `page`, untuk `appends($request->query())` pada rute lain (foto, penerima).
pub fn query_without_page(raw: Option<&str>) -> String {
    raw.unwrap_or("")
        .split('&')
        .filter(|kv| !kv.is_empty() && !kv.starts_with("page="))
        .collect::<Vec<_>>()
        .join("&")
}

/// `rawurlencode()` (RFC 3986): huruf, angka, `-_.~` dibiarkan; selain itu `%XX` huruf besar.
fn rfc3986(s: &str) -> String {
    s.bytes()
        .map(|c| match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (c as char).to_string()
            }
            _ => format!("%{c:02X}"),
        })
        .collect()
}

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles_full = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let scope = crate::access::restriction(user.user_id, &roles_full, "p");
    let viewer = crate::pekerjaan_rel::viewer(&state.pool, user.user_id, &roles_full)
        .await
        .map_err(internal)?;

    let filter = PekerjaanFilter::from_query(&query);

    // `orderBy()` Laravel melempar exception (500) bila kolom diizinkan tetapi arahnya bukan asc/desc.
    let sort_allowed =
        SORTABLE.contains(&filter.sort_by.as_str()) || filter.sort_by == "penerima_count";
    let sort_dir = query
        .get("sort_direction")
        .map_or("desc".to_string(), |s| s.to_lowercase());
    if sort_allowed && sort_dir != "asc" && sort_dir != "desc" {
        return Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Server Error".to_string(),
        ));
    }

    // `(int) per_page === -1`. Laravel memakai cap 500 bila `summary`, selain itu 80.
    if query.get("per_page").is_some_and(|v| php_int(v) == -1) {
        let cap = if summary_requested(&query) { 500 } else { 80 };
        let (rows, _) = list(&state.pool, &filter, &scope, None, Some(cap))
            .await
            .map_err(internal)?;
        let rel = load(
            &state.pool,
            &rows,
            Mode {
                summary: summary_requested(&query),
                unbounded: true,
            },
            &viewer,
        )
        .await
        .map_err(internal)?;
        let data: Vec<Value> = rows
            .iter()
            .map(|p| {
                let mut v = to_resource(p, &rel);
                // Laravel tidak memuat `pendamping` pada per_page=-1: kuncinya tidak ada (bukan null).
                if let Some(m) = v.as_object_mut() {
                    m.remove("pendamping");
                }
                v
            })
            .collect();
        return Ok(Json(json!({ "data": data })).into_response());
    }

    // `(int) $request->get('per_page', 20)`; di bawah 1 jadi 20, di atas 100 jadi 100.
    let pp = query.get("per_page").map_or(20, |v| php_int(v));
    let per_page = if pp < 1 { 20 } else { pp.min(100) as u64 };
    // Paginator::resolveCurrentPage: FILTER_VALIDATE_INT dan >= 1, selain itu halaman 1.
    let page = query
        .get("page")
        .and_then(|v| php_filter_int(v))
        .filter(|v| *v >= 1)
        .unwrap_or(1) as u64;
    let params = PageParams { page, per_page };
    let offset = (page - 1).saturating_mul(per_page);
    let (rows, total) = list(&state.pool, &filter, &scope, Some((per_page, offset)), None)
        .await
        .map_err(internal)?;
    let rel = load(
        &state.pool,
        &rows,
        Mode {
            summary: summary_requested(&query),
            unbounded: false,
        },
        &viewer,
    )
    .await
    .map_err(internal)?;
    let data: Vec<Value> = rows.iter().map(|p| to_resource(p, &rel)).collect();
    let base = format!("{}/api/pekerjaan", state.app_url.trim_end_matches('/'));
    let pairs = laravel_query_pairs(raw.as_deref());
    let body = pagination::paginate_laravel(data, total, params, &base, &|p| {
        laravel_page_url(&base, &pairs, p)
    });
    Ok(Json(body).into_response())
}

pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles_full = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let row = match id.parse::<u64>() {
        Ok(id) => find(&state.pool, id).await.map_err(internal)?,
        Err(_) => None,
    };
    let row = row.ok_or_else(ApiError::not_found)?;
    // `Pekerjaan::userCanAccess` di Laravel: pekerjaan di luar scope mendapat 403.
    if !crate::access::user_can_access(&state.pool, user.user_id, &roles_full, row.id)
        .await
        .map_err(internal)?
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Anda tidak memiliki akses untuk pekerjaan ini",
        ));
    }
    let data =
        crate::pekerjaan_detail::build(&state, &headers, &row, &roles_full, user.user_id, &query)
            .await?;
    Ok(Json(json!({ "data": data })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_full_access_roles_pass_the_gate() {
        assert!(has_full_access(&["operator".to_string()]));
        assert!(has_full_access(&[
            "pengawas".to_string(),
            "admin".to_string()
        ]));
        assert!(!has_full_access(&["pengawas".to_string()]));
        assert!(!has_full_access(&[]));
    }

    #[test]
    fn sort_uses_whitelist_and_defaults_to_created_at_desc() {
        let mut q = HashMap::new();
        q.insert("sort_by".to_string(), "pagu; DROP TABLE x".to_string());
        let f = PekerjaanFilter::from_query(&q);
        assert_eq!(f.order_sql(), " ORDER BY p.created_at DESC");

        q.insert("sort_by".to_string(), "pagu".to_string());
        q.insert("sort_direction".to_string(), "asc".to_string());
        assert_eq!(
            PekerjaanFilter::from_query(&q).order_sql(),
            " ORDER BY p.pagu ASC"
        );
    }

    #[test]
    fn page_links_keep_query_order_and_rfc3986_encoding() {
        let base = "http://x/api/pekerjaan";
        let pairs = laravel_query_pairs(Some("per_page=5&page=3&search=a+b%21"));
        assert_eq!(
            laravel_page_url(base, &pairs, 4),
            "http://x/api/pekerjaan?per_page=5&page=4&search=a%20b%21"
        );
        let none = laravel_query_pairs(None);
        assert_eq!(
            laravel_page_url(base, &none, 2),
            "http://x/api/pekerjaan?page=2"
        );
        // Nilai terakhir menang, posisi kunci pertama dipertahankan (parse_str).
        let dup = laravel_query_pairs(Some("a=1&b=2&a=3"));
        assert_eq!(
            laravel_page_url(base, &dup, 1),
            "http://x/api/pekerjaan?a=3&b=2&page=1"
        );
    }

    #[test]
    fn php_scalar_rules_match_laravel() {
        // (int) PHP: awalan numerik.
        assert_eq!(php_int("2abc"), 2);
        assert_eq!(php_int(" 1e1"), 10);
        assert_eq!(php_int("1.9"), 1);
        assert_eq!(php_int("-1"), -1);
        assert_eq!(php_int("-1.5"), -1);
        assert_eq!(php_int("abc"), 0);
        assert_eq!(php_int(""), 0);
        // FILTER_VALIDATE_INT: tanpa nol di depan, spasi dipangkas.
        assert_eq!(php_filter_int(" 2 "), Some(2));
        assert_eq!(php_filter_int("+3"), Some(3));
        assert_eq!(php_filter_int("02"), None);
        assert_eq!(php_filter_int("1.0"), None);
        assert_eq!(php_filter_int(""), None);
        // filter_var(FILTER_VALIDATE_BOOLEAN) dan empty().
        assert!(php_bool("TRUE"));
        assert!(php_bool(" yes "));
        assert!(!php_bool("0"));
        assert!(!php_bool(""));
        assert!(!php_not_empty("0"));
        assert!(php_not_empty("00"));
    }

    #[test]
    fn zero_and_blank_filters_are_ignored_like_laravel() {
        let mut q = HashMap::new();
        q.insert("tahun".to_string(), "0".to_string());
        q.insert("sub_bidang".to_string(), "   ".to_string());
        q.insert("status".to_string(), "all".to_string());
        q.insert("is_konsultan".to_string(), String::new());
        let f = PekerjaanFilter::from_query(&q);
        assert!(f.tahun.is_none());
        assert!(f.sub_bidang.is_none());
        assert!(f.status.is_none());
        assert_eq!(f.is_konsultan, Some(false));
    }

    #[test]
    fn not_yet_ported_fields_are_null_not_zero() {
        let p = PekerjaanRow {
            id: 1,
            kode_rekening: Some("0".into()),
            nama_paket: Some("Paket".into()),
            pagu: Some(1000.0),
            is_konsultan: false,
            status: None,
            catatan: None,
            kecamatan_id: None,
            desa_id: None,
            kegiatan_id: None,
            pengawas_id: None,
            pendamping_id: None,
            created_at: None,
            updated_at: None,
        };
        let rel = Loaded::empty(Mode {
            summary: false,
            unbounded: false,
        });
        let v = to_resource(&p, &rel);
        assert_eq!(v["progress_total"], 0);
        assert_eq!(v["deviasi"], 0);
        assert_eq!(v["assignment_sources"], json!([]));
        assert_eq!(v["status"], "active");
        assert_eq!(v["pagu"], 1000);
    }
}
