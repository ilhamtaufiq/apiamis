//! Penomoran surat terpusat: satu deret nomor per tahun untuk semua surat, dengan atau tanpa pekerjaan.
//! Tabel `surat` (lihat migrasi 0005). Nomor urut memakai deret yang sama dengan Register Dokumen dan
//! berita acara: baris `tbl_document_sequences` dengan `type = 'berita-acara'`. Jadi satu tahun hanya
//! punya satu nomor urut, apa pun modulnya. Format nomor mengikuti template jenis dokumen
//! (`tbl_document_types.format_template`), dengan token yang sama seperti di frontend Register Dokumen.
//!
//! Rute:
//! - `GET /api/surat?tahun=&search=&pekerjaan_id=&page=&per_page=`: daftar surat satu tahun.
//! - `GET /api/surat/pratinjau?tahun=&type_id=&tanggal=`: nomor yang akan dipakai, tanpa mengambil nomor urut.
//! - `GET /api/surat/{id}`, `POST /api/surat`, `PUT /api/surat/{id}`, `DELETE /api/surat/{id}`.
//!
//! Nomor urut diambil di dalam transaksi dengan `FOR UPDATE` pada baris deret tahun itu, seperti register.
//! Nomor yang diisi manual tidak mengambil nomor urut, dan dijaga unik per tahun oleh indeks di tabel.

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Datelike, NaiveDate};
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};

use crate::{
    media::internal,
    require_auth,
    skp_jurnal::{as_int, gagal, normalize, VALIDATION_MESSAGE},
    AppState,
};

const TEMPLATE_DEFAULT: &str = "{sequence}/{code}-AMIS/{month}/{year}";
const PER_PAGE_DEFAULT: u32 = 20;
const PER_PAGE_MAX: u32 = 100;
const MAX_PERIHAL: usize = 500;
const MAX_TUJUAN: usize = 255;
const MAX_NOMOR: usize = 255;
const MAX_KETERANGAN: usize = 5000;
/// Tipe deret nomor urut bersama. Sama dengan `SEQ_TYPE` di `document_registers` dan `berita_acara`.
const SEQ_TYPE: &str = "berita-acara";

/// Format nomor dengan token: `{sequence}` (3 digit), `{nomor_urut_surat}`, `{code}`, `{year}`/`{tahun}`,
/// `{month}` (romawi), `{day}`.
pub fn format_nomor(template: &str, code: &str, tanggal: NaiveDate, seq: u64) -> String {
    const ROMAWI: [&str; 12] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII"];
    let tahun = tanggal.year().to_string();
    template
        .replace("{sequence}", &format!("{seq:03}"))
        .replace("{nomor_urut_surat}", &seq.to_string())
        .replace("{code}", code)
        .replace("{year}", &tahun)
        .replace("{tahun}", &tahun)
        .replace("{month}", ROMAWI[tanggal.month0() as usize])
        .replace("{day}", &format!("{:02}", tanggal.day()))
}

/// Isi surat yang bisa diubah pengguna. `nomor` `None` hanya saat membuat dan nomor dibuat otomatis.
#[derive(Debug, Clone, PartialEq)]
pub struct Masukan {
    pub type_id: i64,
    pub tanggal: NaiveDate,
    pub perihal: String,
    pub tujuan: Option<String>,
    pub keterangan: Option<String>,
    pub pekerjaan_id: Option<i64>,
    pub nomor: Option<String>,
}

fn tanggal_dari(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

/// Membaca body. Saat mengubah (`dasar` terisi), kunci yang tidak dikirim mempertahankan nilai lama.
pub fn parse(body: &Value, dasar: Option<&Masukan>) -> Result<Masukan, ApiError> {
    let mut errors = BTreeMap::new();
    let ada = |k: &str| body.get(k).map(|v| normalize(v.clone()));

    let type_id = match ada("type_id") {
        Some(v) => as_int(&v),
        None => dasar.map(|d| d.type_id),
    };
    if type_id.is_none() {
        gagal(&mut errors, "type_id", "Jenis dokumen wajib dipilih.");
    }

    let tanggal = match ada("tanggal") {
        Some(Value::String(s)) => tanggal_dari(&s),
        Some(_) => None,
        None => dasar.map(|d| d.tanggal),
    };
    if tanggal.is_none() {
        gagal(&mut errors, "tanggal", "Tanggal wajib diisi dengan format YYYY-MM-DD.");
    }

    let perihal = match ada("perihal") {
        Some(Value::String(s)) if s.chars().count() <= MAX_PERIHAL => Some(s),
        Some(Value::String(_)) => {
            gagal(&mut errors, "perihal", "Perihal maksimal 500 karakter.");
            None
        }
        Some(_) => None,
        None => dasar.map(|d| d.perihal.clone()),
    };
    if perihal.as_deref().map_or(true, str::is_empty) {
        gagal(&mut errors, "perihal", "Perihal wajib diisi.");
    }

    let tujuan = texto(&ada("tujuan"), dasar.map(|d| d.tujuan.clone()), MAX_TUJUAN, "tujuan", "Tujuan", &mut errors);
    let keterangan = texto(
        &ada("keterangan"),
        dasar.map(|d| d.keterangan.clone()),
        MAX_KETERANGAN,
        "keterangan",
        "Keterangan",
        &mut errors,
    );

    // `null` berarti pekerjaan dilepas. Kunci yang tidak dikirim mempertahankan nilai lama.
    let pekerjaan_id = match ada("pekerjaan_id") {
        Some(Value::Null) => None,
        Some(v) => match as_int(&v) {
            Some(i) => Some(i),
            None => {
                gagal(&mut errors, "pekerjaan_id", "Pekerjaan tidak valid.");
                None
            }
        },
        None => dasar.and_then(|d| d.pekerjaan_id),
    };

    let nomor = match ada("nomor") {
        Some(Value::String(s)) if s.chars().count() > MAX_NOMOR => {
            gagal(&mut errors, "nomor", "Nomor maksimal 255 karakter.");
            None
        }
        Some(Value::String(s)) => Some(s),
        Some(_) | None => dasar.and_then(|d| d.nomor.clone()),
    };

    if !errors.is_empty() {
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }
    Ok(Masukan {
        type_id: type_id.unwrap_or_default(),
        tanggal: tanggal.unwrap_or_default(),
        perihal: perihal.unwrap_or_default(),
        tujuan,
        keterangan,
        pekerjaan_id,
        nomor,
    })
}

/// Teks opsional: kosong atau `null` berarti tidak diisi, melebihi `maks` ditolak.
fn texto(
    kiriman: &Option<Value>,
    lama: Option<Option<String>>,
    maks: usize,
    kunci: &str,
    label: &str,
    errors: &mut BTreeMap<String, Vec<String>>,
) -> Option<String> {
    match kiriman {
        Some(Value::String(s)) if s.chars().count() > maks => {
            gagal(errors, kunci, &format!("{label} maksimal {maks} karakter."));
            None
        }
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Null) => None,
        Some(_) => {
            gagal(errors, kunci, &format!("{label} harus berupa teks."));
            None
        }
        None => lama.flatten(),
    }
}

/// Nomor berikutnya dari template, tanpa mengambil nomor urut. Dipakai untuk pratinjau.
pub fn pratinjau(template: Option<&str>, code: &str, tanggal: NaiveDate, terakhir: u64) -> String {
    let template = template.filter(|t| !t.trim().is_empty()).unwrap_or(TEMPLATE_DEFAULT);
    format_nomor(template, code, tanggal, terakhir + 1)
}

fn duplikat(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.code().as_deref() == Some("1062"))
}

fn nomor_dipakai(tahun: i64, nomor: &str) -> ApiError {
    let mut errors = BTreeMap::new();
    gagal(&mut errors, "nomor", &format!("Nomor {nomor} sudah dipakai pada tahun {tahun}."));
    ApiError::validation(VALIDATION_MESSAGE, errors)
}

/// Baris surat lengkap dengan jenis dan pekerjaan induk.
pub struct Baris {
    pub id: i64,
    pub tahun: i64,
    pub sequence_number: i64,
    pub masukan: Masukan,
    pub jenis_nama: String,
    pub jenis_code: String,
    pub pekerjaan_nama: Option<String>,
}

fn baris_json(b: &Baris) -> Value {
    let m = &b.masukan;
    json!({
        "id": b.id,
        "tahun": b.tahun,
        "sequence_number": b.sequence_number,
        "nomor": m.nomor,
        "tanggal": m.tanggal.format("%Y-%m-%d").to_string(),
        "perihal": m.perihal,
        "tujuan": m.tujuan,
        "keterangan": m.keterangan,
        "type_id": m.type_id,
        "jenis": { "id": m.type_id, "name": b.jenis_nama, "code": b.jenis_code },
        "pekerjaan_id": m.pekerjaan_id,
        "pekerjaan": m.pekerjaan_id.map(|id| json!({ "id": id, "nama_paket": b.pekerjaan_nama })),
    })
}

const SELECT_BARIS: &str = "SELECT CAST(s.id AS SIGNED) AS id, CAST(s.tahun AS SIGNED) AS tahun, \
    CAST(s.sequence_number AS SIGNED) AS sequence_number, CAST(s.type_id AS SIGNED) AS type_id, s.nomor, \
    DATE_FORMAT(s.tanggal, '%Y-%m-%d') AS tanggal, s.perihal, s.tujuan, s.keterangan, \
    CAST(s.pekerjaan_id AS SIGNED) AS pekerjaan_id, t.name AS jenis_nama, t.code AS jenis_code, \
    p.nama_paket AS pekerjaan_nama \
    FROM surat s JOIN tbl_document_types t ON t.id = s.type_id \
    LEFT JOIN tbl_pekerjaan p ON p.id = s.pekerjaan_id";

fn baca_baris(row: &sqlx::mysql::MySqlRow) -> Result<Baris, ApiError> {
    let tanggal: String = row.try_get("tanggal").map_err(internal)?;
    let tanggal = NaiveDate::parse_from_str(&tanggal, "%Y-%m-%d").map_err(internal)?;
    Ok(Baris {
        id: row.try_get("id").map_err(internal)?,
        tahun: row.try_get("tahun").map_err(internal)?,
        sequence_number: row.try_get("sequence_number").map_err(internal)?,
        masukan: Masukan {
            type_id: row.try_get("type_id").map_err(internal)?,
            tanggal,
            perihal: row.try_get("perihal").map_err(internal)?,
            tujuan: row.try_get("tujuan").map_err(internal)?,
            keterangan: row.try_get("keterangan").map_err(internal)?,
            pekerjaan_id: row.try_get("pekerjaan_id").map_err(internal)?,
            nomor: Some(row.try_get("nomor").map_err(internal)?),
        },
        jenis_nama: row.try_get("jenis_nama").map_err(internal)?,
        jenis_code: row.try_get("jenis_code").map_err(internal)?,
        pekerjaan_nama: row.try_get("pekerjaan_nama").map_err(internal)?,
    })
}

async fn ambil_baris(pool: &MySqlPool, id: i64) -> Result<Option<Baris>, ApiError> {
    let row = sqlx::query(&format!("{SELECT_BARIS} WHERE s.id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    row.as_ref().map(baca_baris).transpose()
}

fn tahun_dari_query(query: &HashMap<String, String>) -> Result<i64, ApiError> {
    query
        .get("tahun")
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|t| (2000..=2100).contains(t))
        .ok_or_else(|| {
            let mut errors = BTreeMap::new();
            gagal(&mut errors, "tahun", "Tahun harus antara 2000 dan 2100.");
            ApiError::validation(VALIDATION_MESSAGE, errors)
        })
}

/// `GET /api/surat`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let tahun = match query.get("tahun") {
        Some(_) => tahun_dari_query(&query)?,
        None => chrono::Utc::now().year() as i64,
    };
    let page = query.get("page").and_then(|s| s.parse::<u32>().ok()).unwrap_or(1).max(1);
    let per_page = query
        .get("per_page")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(PER_PAGE_DEFAULT)
        .clamp(1, PER_PAGE_MAX);

    let mut where_sql = String::from("WHERE s.tahun = ?");
    let mut args: Vec<Arg> = vec![Arg::Int(tahun)];
    if let Some(cari) = query.get("search").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        where_sql.push_str(" AND (s.nomor LIKE ? OR s.perihal LIKE ? OR s.tujuan LIKE ?)");
        let pola = format!("%{cari}%");
        args.extend([Arg::Text(pola.clone()), Arg::Text(pola.clone()), Arg::Text(pola)]);
    }
    if let Some(p) = query.get("pekerjaan_id").and_then(|s| s.parse::<i64>().ok()) {
        where_sql.push_str(" AND s.pekerjaan_id = ?");
        args.push(Arg::Int(p));
    }

    let sql_hitung = format!("SELECT COUNT(*) FROM surat s {where_sql}");
    let mut hitung = sqlx::query_scalar::<_, i64>(&sql_hitung);
    for a in &args {
        hitung = match a {
            Arg::Int(i) => hitung.bind(*i),
            Arg::Text(s) => hitung.bind(s.clone()),
        };
    }
    let total = hitung.fetch_one(&state.pool).await.map_err(internal)?;

    let sql = format!(
        "{SELECT_BARIS} {where_sql} ORDER BY s.tanggal DESC, s.sequence_number DESC LIMIT ? OFFSET ?"
    );
    let mut ambil = sqlx::query(&sql);
    for a in &args {
        ambil = match a {
            Arg::Int(i) => ambil.bind(*i),
            Arg::Text(s) => ambil.bind(s.clone()),
        };
    }
    let rows = ambil
        .bind(per_page as i64)
        .bind(((page - 1) * per_page) as i64)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let data = rows
        .iter()
        .map(|r| baca_baris(r).map(|b| baris_json(&b)))
        .collect::<Result<Vec<_>, _>>()?;
    let last_page = ((total as u64).div_ceil(per_page as u64)).max(1);

    Ok(Json(json!({
        "data": data,
        "meta": { "tahun": tahun, "page": page, "per_page": per_page, "total": total, "last_page": last_page },
    })))
}

enum Arg {
    Int(i64),
    Text(String),
}

/// `GET /api/surat/pratinjau?tahun=&type_id=&tanggal=`: nomor yang akan dipakai bila surat disimpan sekarang.
pub async fn pratinjau_nomor(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let mut errors = BTreeMap::new();
    let type_id = query.get("type_id").and_then(|s| s.parse::<i64>().ok());
    let tanggal = query.get("tanggal").and_then(|s| tanggal_dari(s));
    if type_id.is_none() {
        gagal(&mut errors, "type_id", "Jenis dokumen wajib dipilih.");
    }
    if tanggal.is_none() {
        gagal(&mut errors, "tanggal", "Tanggal wajib diisi dengan format YYYY-MM-DD.");
    }
    if !errors.is_empty() {
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }
    let (type_id, tanggal) = (type_id.unwrap_or_default(), tanggal.unwrap_or_default());
    let jenis = jenis_dari_id(&state.pool, type_id).await?;
    let tahun = tanggal.year() as i64;
    let terakhir: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(last_number AS SIGNED) FROM tbl_document_sequences WHERE year = ? AND type = ?",
    )
    .bind(tahun)
    .bind(SEQ_TYPE)
    .fetch_optional(&state.pool)
        .await
        .map_err(internal)?;
    let nomor = pratinjau(jenis.1.as_deref(), &jenis.0, tanggal, terakhir.unwrap_or(0) as u64);
    Ok(Json(json!({ "data": { "nomor": nomor, "sequence_number": terakhir.unwrap_or(0) + 1 } })))
}

/// Kode dan template jenis dokumen. Jenis yang tidak ada menjadi error validasi di `type_id`.
async fn jenis_dari_id(pool: &MySqlPool, type_id: i64) -> Result<(String, Option<String>), ApiError> {
    let row = sqlx::query("SELECT code, format_template FROM tbl_document_types WHERE id = ?")
        .bind(type_id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    match row {
        Some(r) => Ok((
            r.try_get("code").map_err(internal)?,
            r.try_get("format_template").map_err(internal)?,
        )),
        None => {
            let mut errors = BTreeMap::new();
            gagal(&mut errors, "type_id", "Jenis dokumen tidak ditemukan.");
            Err(ApiError::validation(VALIDATION_MESSAGE, errors))
        }
    }
}

/// `GET /api/surat/{id}`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let baris = ambil_baris(&state.pool, id).await?.ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": baris_json(&baris) })))
}

/// `POST /api/surat`: membuat surat dan mengambil nomor urut berikutnya dalam satu transaksi.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let masukan = parse(&body, None)?;
    let tahun = masukan.tanggal.year() as i64;
    let (code, template) = jenis_dari_id(&state.pool, masukan.type_id).await?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    // Baris deret tahun ini dikunci sampai commit, sama seperti register: tidak ada dua nomor yang sama.
    let terakhir: Option<Option<i64>> = sqlx::query_scalar(
        "SELECT CAST(last_number AS SIGNED) FROM tbl_document_sequences WHERE year = ? AND type = ? FOR UPDATE",
    )
    .bind(tahun)
    .bind(SEQ_TYPE)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let seq = terakhir.flatten().map_or(1, |l| l + 1);
    sqlx::query(
        "INSERT INTO tbl_document_sequences (year, type, last_number) VALUES (?, ?, ?) \
         ON DUPLICATE KEY UPDATE last_number = VALUES(last_number)",
    )
    .bind(tahun)
    .bind(SEQ_TYPE)
    .bind(seq)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let seq = seq as u64;

    let nomor = match masukan.nomor.clone().filter(|n| !n.trim().is_empty()) {
        Some(n) => n.trim().to_string(),
        None => pratinjau_nomor_dari(template.as_deref(), &code, masukan.tanggal, seq),
    };
    let dipakai: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM surat WHERE tahun = ? AND nomor = ?")
        .bind(tahun)
        .bind(&nomor)
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
    if dipakai > 0 {
        return Err(nomor_dipakai(tahun, &nomor));
    }

    let hasil = sqlx::query(
        "INSERT INTO surat (tahun, sequence_number, type_id, nomor, tanggal, perihal, tujuan, keterangan, \
         pekerjaan_id, created_by, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(tahun)
    .bind(seq)
    .bind(masukan.type_id)
    .bind(&nomor)
    .bind(masukan.tanggal)
    .bind(&masukan.perihal)
    .bind(&masukan.tujuan)
    .bind(&masukan.keterangan)
    .bind(masukan.pekerjaan_id)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await;
    let id = match hasil {
        Ok(r) => r.last_insert_id() as i64,
        Err(e) if duplikat(&e) => return Err(nomor_dipakai(tahun, &nomor)),
        Err(e) => return Err(internal(e)),
    };
    tx.commit().await.map_err(internal)?;

    let baris = ambil_baris(&state.pool, id).await?.ok_or_else(|| internal("surat hilang setelah simpan"))?;
    Ok((StatusCode::CREATED, Json(json!({ "data": baris_json(&baris) }))).into_response())
}

fn pratinjau_nomor_dari(template: Option<&str>, code: &str, tanggal: NaiveDate, seq: u64) -> String {
    let template = template.filter(|t| !t.trim().is_empty()).unwrap_or(TEMPLATE_DEFAULT);
    format_nomor(template, code, tanggal, seq)
}

/// `PUT /api/surat/{id}`. Tahun surat tidak berubah: nomor urut hanya berlaku di tahunnya.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let lama = ambil_baris(&state.pool, id).await?.ok_or_else(ApiError::not_found)?;
    let masukan = parse(&body, Some(&lama.masukan))?;

    if masukan.tanggal.year() as i64 != lama.tahun {
        let mut errors = BTreeMap::new();
        gagal(&mut errors, "tanggal", "Tanggal tidak boleh pindah ke tahun lain. Nomor urut mengikuti tahun asal.");
        return Err(ApiError::validation(VALIDATION_MESSAGE, errors));
    }
    let nomor = masukan.nomor.clone().unwrap_or_default();
    if lama.masukan.nomor.as_deref() != Some(nomor.as_str()) {
        let dipakai: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM surat WHERE tahun = ? AND nomor = ? AND id <> ?")
            .bind(lama.tahun)
            .bind(&nomor)
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .map_err(internal)?;
        if dipakai > 0 {
            return Err(nomor_dipakai(lama.tahun, &nomor));
        }
    }

    let hasil = sqlx::query(
        "UPDATE surat SET type_id = ?, nomor = ?, tanggal = ?, perihal = ?, tujuan = ?, keterangan = ?, \
         pekerjaan_id = ?, updated_at = NOW() WHERE id = ?",
    )
    .bind(masukan.type_id)
    .bind(&nomor)
    .bind(masukan.tanggal)
    .bind(&masukan.perihal)
    .bind(&masukan.tujuan)
    .bind(&masukan.keterangan)
    .bind(masukan.pekerjaan_id)
    .bind(id)
    .execute(&state.pool)
    .await;
    if let Err(e) = &hasil {
        if duplikat(e) {
            return Err(nomor_dipakai(lama.tahun, &nomor));
        }
    }
    hasil.map_err(internal)?;

    let baris = ambil_baris(&state.pool, id).await?.ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({ "data": baris_json(&baris) })))
}

/// `DELETE /api/surat/{id}`. Nomor yang sudah terpakai tidak dipakai ulang, jadi celah di deret tetap ada.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let hasil = sqlx::query("DELETE FROM surat WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    if hasil.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tgl(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn format_nomor_mengikuti_token_frontend() {
        let t = "{sequence}/{code}-AMIS/{month}/{year}";
        assert_eq!(format_nomor(t, "SRT", tgl("2026-05-03"), 7), "007/SRT-AMIS/V/2026");
        assert_eq!(format_nomor("{nomor_urut_surat}-{day}-{tahun}", "X", tgl("2026-12-09"), 12), "12-09-2026");
    }

    #[test]
    fn pratinjau_memakai_template_bawaan_bila_kosong() {
        assert_eq!(pratinjau(None, "SRT", tgl("2026-01-10"), 41), "042/SRT-AMIS/I/2026");
        assert_eq!(pratinjau(Some("  "), "SRT", tgl("2026-01-10"), 0), "001/SRT-AMIS/I/2026");
        assert_eq!(pratinjau(Some("{code}-{sequence}"), "BAST", tgl("2026-02-01"), 9), "BAST-010");
    }

    #[test]
    fn parse_membuat_surat_dengan_isian_wajib() {
        let m = parse(
            &json!({ "type_id": 3, "tanggal": "2026-05-03", "perihal": "  Undangan rapat  ", "tujuan": "" }),
            None,
        )
        .unwrap();
        assert_eq!(m.type_id, 3);
        assert_eq!(m.perihal, "Undangan rapat");
        assert_eq!(m.tujuan, None);
        assert_eq!(m.nomor, None);
        assert_eq!(m.pekerjaan_id, None);
    }

    #[test]
    fn parse_menolak_isian_wajib_kosong() {
        let err = parse(&json!({ "perihal": "" }), None).unwrap_err();
        let errors = err.errors.expect("ada galat validasi");
        assert!(errors.contains_key("type_id"));
        assert!(errors.contains_key("tanggal"));
        assert!(errors.contains_key("perihal"));
    }

    #[test]
    fn parse_update_mempertahankan_nilai_lama_dan_bisa_melepas_pekerjaan() {
        let lama = Masukan {
            type_id: 3,
            tanggal: tgl("2026-05-03"),
            perihal: "Lama".into(),
            tujuan: Some("Dinas".into()),
            keterangan: None,
            pekerjaan_id: Some(9),
            nomor: Some("001/SRT-AMIS/V/2026".into()),
        };
        let m = parse(&json!({ "perihal": "Baru", "pekerjaan_id": null }), Some(&lama)).unwrap();
        assert_eq!(m.perihal, "Baru");
        assert_eq!(m.type_id, 3);
        assert_eq!(m.tujuan.as_deref(), Some("Dinas"));
        assert_eq!(m.pekerjaan_id, None);
        assert_eq!(m.nomor.as_deref(), Some("001/SRT-AMIS/V/2026"));
    }
}
