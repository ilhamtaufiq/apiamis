//! `GET /api/kegiatan`, setara `KegiatanController@index` + `KegiatanResource`.
//!
//! Filter: `tahun` (`tahun_anggaran`), `per_page`. `per_page=-1` mengembalikan
//! semua baris tanpa paginasi (`{"data": [...]}`), seperti di Laravel.

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};
use std::collections::HashMap;

use crate::{desa::internal, format::iso8601_utc, pagination, require_auth, AppState};

#[derive(Debug, Clone, PartialEq)]
pub struct KegiatanRow {
    pub id: u64,
    pub nama_program: Option<String>,
    pub sub_bidang: Option<String>,
    pub nama_kegiatan: Option<String>,
    pub nama_sub_kegiatan: Option<String>,
    pub tahun_anggaran: Option<String>,
    pub sumber_dana: Option<String>,
    /// `decimal(15,2)`: Laravel mengirim sebagai string, contoh `"0.00"`.
    pub pagu: Option<String>,
    /// Kolom JSON, di-cast `array` oleh Laravel.
    pub kode_rekening: Option<Value>,
    pub nama_pptk: Option<String>,
    pub nip_pptk: Option<String>,
    pub sipd_id_sub_bl: Option<u64>,
    pub kode_sub_giat: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Bentuk `KegiatanResource`.
pub fn to_resource(r: &KegiatanRow) -> Value {
    json!({
        "id": r.id,
        "nama_program": r.nama_program,
        "sub_bidang": r.sub_bidang,
        "nama_kegiatan": r.nama_kegiatan,
        "nama_sub_kegiatan": r.nama_sub_kegiatan,
        "tahun_anggaran": r.tahun_anggaran,
        "sumber_dana": r.sumber_dana,
        "pagu": r.pagu,
        "kode_rekening": r.kode_rekening,
        "nama_pptk": r.nama_pptk,
        "nip_pptk": r.nip_pptk,
        "sipd_id_sub_bl": r.sipd_id_sub_bl,
        "kode_sub_giat": r.kode_sub_giat,
        "created_at": iso8601_utc(r.created_at),
        "updated_at": iso8601_utc(r.updated_at),
    })
}

/// Mengambil baris; `limit = None` berarti semua baris.
pub async fn list(
    pool: &MySqlPool,
    tahun: Option<&str>,
    page: Option<(u64, u64)>,
) -> Result<(Vec<KegiatanRow>, u64), sqlx::Error> {
    let where_sql = if tahun.is_some() {
        " WHERE tahun_anggaran = ?"
    } else {
        ""
    };

    let count_sql = format!("SELECT COUNT(*) FROM tbl_kegiatan{where_sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(t) = tahun {
        cq = cq.bind(t);
    }
    let total = cq.fetch_one(pool).await? as u64;

    let mut sql = format!(
        "SELECT id, nama_program, sub_bidang, nama_kegiatan, nama_sub_kegiatan, tahun_anggaran, \
         sumber_dana, CAST(pagu AS CHAR) AS pagu, CAST(kode_rekening AS CHAR) AS kode_rekening, \
         nama_pptk, nip_pptk, sipd_id_sub_bl, kode_sub_giat, created_at, updated_at \
         FROM tbl_kegiatan{where_sql} ORDER BY id"
    );
    if page.is_some() {
        sql.push_str(" LIMIT ? OFFSET ?");
    }
    let mut q = sqlx::query(&sql);
    if let Some(t) = tahun {
        q = q.bind(t);
    }
    if let Some((limit, offset)) = page {
        q = q.bind(limit).bind(offset);
    }
    let rows = q.fetch_all(pool).await?;

    let items = rows
        .iter()
        .map(|r| -> Result<KegiatanRow, sqlx::Error> {
            let raw_kode: Option<String> = r.try_get("kode_rekening")?;
            let kode_rekening =
                raw_kode.map(|s| serde_json::from_str::<Value>(&s).unwrap_or(Value::Null));
            Ok(KegiatanRow {
                id: r.try_get("id")?,
                nama_program: r.try_get("nama_program")?,
                sub_bidang: r.try_get("sub_bidang")?,
                nama_kegiatan: r.try_get("nama_kegiatan")?,
                nama_sub_kegiatan: r.try_get("nama_sub_kegiatan")?,
                tahun_anggaran: r.try_get("tahun_anggaran")?,
                sumber_dana: r.try_get("sumber_dana")?,
                pagu: r.try_get("pagu")?,
                kode_rekening,
                nama_pptk: r.try_get("nama_pptk")?,
                nip_pptk: r.try_get("nip_pptk")?,
                sipd_id_sub_bl: r.try_get("sipd_id_sub_bl")?,
                kode_sub_giat: r.try_get("kode_sub_giat")?,
                created_at: r.try_get("created_at")?,
                updated_at: r.try_get("updated_at")?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok((items, total))
}

pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;

    let tahun = query
        .get("tahun")
        .filter(|t| !t.is_empty())
        .map(String::as_str);

    if query.get("per_page").map(String::as_str) == Some("-1") {
        let (rows, _) = list(&state.pool, tahun, None).await.map_err(internal)?;
        let data: Vec<Value> = rows.iter().map(to_resource).collect();
        return Ok(Json(json!({ "data": data })));
    }

    let params = pagination::page_params(&query);
    let offset = (params.page - 1) * params.per_page;
    let (rows, total) = list(&state.pool, tahun, Some((params.per_page, offset)))
        .await
        .map_err(internal)?;

    let data: Vec<Value> = rows.iter().map(to_resource).collect();
    let base = format!("{}/api/kegiatan", state.app_url.trim_end_matches('/'));
    Ok(Json(pagination::paginate(data, total, params, &base)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    const FIXTURE: &str = include_str!("../../../fixtures/live/kegiatan_index.json");

    fn s(v: &Value) -> Option<String> {
        v.as_str().map(str::to_string)
    }

    fn ts(v: &Value) -> Option<DateTime<Utc>> {
        v.as_str().map(|x| {
            NaiveDateTime::parse_from_str(&x[..19], "%Y-%m-%dT%H:%M:%S")
                .unwrap()
                .and_utc()
        })
    }

    /// Membangun baris dari item fixture (nilai yang sudah dibersihkan), lalu
    /// mengharapkan bentuk resource identik. Field pribadi di fixture sudah
    /// `<redacted>`, jadi ikut dibawa apa adanya.
    #[test]
    fn fixture_page_is_reproduced_exactly() {
        let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
        let body = &fixture["body"];
        let items = body["data"].as_array().unwrap();

        for item in items {
            let row = KegiatanRow {
                id: item["id"].as_u64().unwrap(),
                nama_program: s(&item["nama_program"]),
                sub_bidang: s(&item["sub_bidang"]),
                nama_kegiatan: s(&item["nama_kegiatan"]),
                nama_sub_kegiatan: s(&item["nama_sub_kegiatan"]),
                tahun_anggaran: s(&item["tahun_anggaran"]),
                sumber_dana: s(&item["sumber_dana"]),
                pagu: s(&item["pagu"]),
                kode_rekening: Some(item["kode_rekening"].clone()),
                nama_pptk: s(&item["nama_pptk"]),
                nip_pptk: s(&item["nip_pptk"]),
                sipd_id_sub_bl: item["sipd_id_sub_bl"].as_u64(),
                kode_sub_giat: s(&item["kode_sub_giat"]),
                created_at: ts(&item["created_at"]),
                updated_at: ts(&item["updated_at"]),
            };
            assert_eq!(&to_resource(&row), item, "id {}", row.id);
        }

        let params = pagination::PageParams {
            page: 1,
            per_page: 15,
        };
        let built = pagination::paginate(
            items.to_vec(),
            20,
            params,
            "http://apiamis.cianjur.space/api/kegiatan",
        );
        assert_eq!(built["meta"], body["meta"]);
        assert_eq!(built["links"], body["links"]);
    }
}
