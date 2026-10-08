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
    http::{HeaderMap, HeaderValue, StatusCode},
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

pub const PARTIAL_HEADER: &str = "pekerjaan: progres, foto, kontrak, assignment_sources, search kontrak.penyedia, sort penerima_count belum dipindah";

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

#[derive(Debug, Default, Clone)]
pub struct PekerjaanFilter {
    pub tahun: Option<String>,
    pub kecamatan_id: Option<String>,
    pub desa_id: Option<String>,
    pub kegiatan_id: Option<String>,
    pub nama_sub_kegiatan: Option<String>,
    pub sub_bidang: Option<String>,
    pub search: Option<String>,
    pub sort_by: String,
    pub sort_desc: bool,
}

impl PekerjaanFilter {
    pub fn from_query(q: &HashMap<String, String>) -> Self {
        let nonempty = |k: &str| q.get(k).filter(|v| !v.is_empty()).cloned();
        let sort_by = q.get("sort_by").cloned().unwrap_or_default();
        let sort_dir = q.get("sort_direction").map(|s| s.to_lowercase());
        Self {
            tahun: nonempty("tahun"),
            kecamatan_id: nonempty("kecamatan_id"),
            desa_id: nonempty("desa_id"),
            kegiatan_id: nonempty("kegiatan_id"),
            nama_sub_kegiatan: nonempty("nama_sub_kegiatan"),
            sub_bidang: nonempty("sub_bidang"),
            search: nonempty("search")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
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
        if let Some(s) = &self.search {
            let like = format!("%{s}%");
            sql.push_str(
                " AND (p.nama_paket LIKE ? OR p.kode_rekening LIKE ? \
                 OR p.desa_id IN (SELECT id FROM tbl_desa WHERE n_desa LIKE ?) \
                 OR p.kecamatan_id IN (SELECT id FROM tbl_kecamatan WHERE n_kec LIKE ?) \
                 OR p.pengawas_id IN (SELECT id FROM pengawas WHERE nama LIKE ?))",
            );
            for _ in 0..5 {
                b.push(like.clone());
            }
        }
        (sql, b)
    }

    fn order_sql(&self) -> String {
        let dir = if self.sort_desc { "DESC" } else { "ASC" };
        if SORTABLE.contains(&self.sort_by.as_str()) {
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
    page: Option<(u64, u64)>,
    cap: Option<u64>,
) -> Result<(Vec<PekerjaanRow>, u64), sqlx::Error> {
    let (where_sql, binds) = f.where_clause();
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
}

async fn load(pool: &MySqlPool, rows: &[PekerjaanRow]) -> Result<Loaded, sqlx::Error> {
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
    for pid in &pekerjaan_ids {
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
    let peng = p.pengawas_id.and_then(|id| rel.pengawas.get(&id).cloned());
    let pend = p
        .pendamping_id
        .and_then(|id| rel.pengawas.get(&id).cloned());
    json!({
        "id": p.id,
        "kode_rekening": p.kode_rekening,
        "nama_paket": p.nama_paket,
        "pagu": p.pagu.map(number_like_php),
        "is_konsultan": p.is_konsultan,
        "status": p.status.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "active".to_string()),
        "catatan": p.catatan,
        // BELUM DIPINDAH: null, bukan nilai yang salah.
        "has_kontrak": Value::Null,
        "kontrak_count": Value::Null,
        "progress_total": Value::Null,
        "deviasi": Value::Null,
        "progress_estimasi_fisik": Value::Null,
        "progress_estimasi_keuangan": Value::Null,
        "progress_estimasi_keuangan_nilai": Value::Null,
        "deviasi_estimasi_fisik": Value::Null,
        "deviasi_estimasi_keuangan": Value::Null,
        "foto_count": Value::Null,
        "foto_required_count": Value::Null,
        "foto_status": Value::Null,
        "kecamatan_id": p.kecamatan_id,
        "desa_id": p.desa_id,
        "kegiatan_id": p.kegiatan_id,
        "pengawas_id": p.pengawas_id,
        "pendamping_id": p.pendamping_id,
        "assignment_sources": Value::Null,
        "kecamatan": kec_key,
        "desa": desa_key,
        "kegiatan": keg_key,
        "pengawas": peng,
        "pendamping": pend,
        "tags": rel.tags.get(&p.id).cloned().unwrap_or_default(),
        "kontrak": Value::Null,
        "created_at": iso8601_utc(p.created_at),
        "updated_at": iso8601_utc(p.updated_at),
    })
}

fn partial_header(mut resp: Response) -> Response {
    resp.headers_mut().insert(
        "x-partial-response",
        HeaderValue::from_static(PARTIAL_HEADER),
    );
    resp
}

fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "message": "Daftar pekerjaan untuk role ini belum tersedia di Rust (scope pengawas belum dipindah)."
        })),
    )
        .into_response()
}

/// Query string tanpa `page`, untuk `appends($request->query())`.
fn query_without_page(raw: Option<&str>) -> String {
    raw.unwrap_or("")
        .split('&')
        .filter(|kv| !kv.is_empty() && !kv.starts_with("page="))
        .collect::<Vec<_>>()
        .join("&")
}

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::permission::user_role_names(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    if !has_full_access(&roles) {
        return Ok(forbidden());
    }

    let filter = PekerjaanFilter::from_query(&query);

    if query.get("per_page").map(String::as_str) == Some("-1") {
        let (rows, _) = list(&state.pool, &filter, None, Some(80))
            .await
            .map_err(internal)?;
        let rel = load(&state.pool, &rows).await.map_err(internal)?;
        let data: Vec<Value> = rows.iter().map(|p| to_resource(p, &rel)).collect();
        return Ok(partial_header(
            Json(json!({ "data": data })).into_response(),
        ));
    }

    let per_page = query
        .get("per_page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(20)
        .min(100);
    let page = query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1);
    let params = PageParams { page, per_page };
    let offset = (page - 1) * per_page;
    let (rows, total) = list(&state.pool, &filter, Some((per_page, offset)), None)
        .await
        .map_err(internal)?;
    let rel = load(&state.pool, &rows).await.map_err(internal)?;
    let data: Vec<Value> = rows.iter().map(|p| to_resource(p, &rel)).collect();
    let base = format!("{}/api/pekerjaan", state.app_url.trim_end_matches('/'));
    let body = pagination::paginate_with_query(
        data,
        total,
        params,
        &base,
        &query_without_page(raw.as_deref()),
    );
    Ok(partial_header(Json(body).into_response()))
}

pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let roles = auth::permission::user_role_names(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    if !has_full_access(&roles) {
        return Ok(forbidden());
    }
    let row = match id.parse::<u64>() {
        Ok(id) => find(&state.pool, id).await.map_err(internal)?,
        Err(_) => None,
    };
    let row = row.ok_or_else(ApiError::not_found)?;
    let rel = load(&state.pool, std::slice::from_ref(&row))
        .await
        .map_err(internal)?;
    Ok(partial_header(
        Json(json!({ "data": to_resource(&row, &rel) })).into_response(),
    ))
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
    fn appends_query_without_page() {
        assert_eq!(
            query_without_page(Some("per_page=5&page=3&search=a")),
            "per_page=5&search=a"
        );
        assert_eq!(query_without_page(None), "");
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
        let rel = Loaded {
            kecamatan: HashMap::new(),
            desa: HashMap::new(),
            kegiatan: HashMap::new(),
            pengawas: HashMap::new(),
            tags: HashMap::new(),
        };
        let v = to_resource(&p, &rel);
        assert_eq!(v["progress_total"], Value::Null);
        assert_eq!(v["assignment_sources"], Value::Null);
        assert_eq!(v["status"], "active");
        assert_eq!(v["pagu"], 1000);
    }
}
