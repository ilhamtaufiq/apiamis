//! Dokumen kontrak: SPK (`GET /api/kontrak/{id}/export`), cover, ZIP cover per tahun, konteks BAP, dan BAP.
//! Port `DocumentExportService`, `KontrakBapContextService`, dan metode `KontrakController` yang memakainya.
//!
//! Catatan: di Laravel, `GET kontrak/{id}/export` didaftarkan sebelum `apiResource`, sehingga metode
//! `export` (dengan `format` dan fallback ID pekerjaan) yang berjalan. `exportDoc` tidak pernah terpanggil
//! dan tidak dipindah terpisah.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::{Cursor, Write},
    path::PathBuf,
};

use axum::{
    extract::{Path, Query, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{NaiveDate, Utc};
use regex::Regex;
use serde_json::{json, Value};
use shared::ApiError;
use sqlx::{MySqlPool, Row};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::{
    docx_template, kontrak,
    kontrak::{KontrakRow, SELECT_ADDENDUM},
    kontrak_document_data::{
        self as data, DocCtx, DocSettings, KegiatanDoc, Overrides, PekerjaanDoc, PenyediaDoc,
        RawDocSettings, RegisterDoc,
    },
    kontrak_register_gap, mailer, media, onlyoffice, require_auth, AppState,
};

const APP_SETTING_MODEL: &str = "App\\Models\\AppSetting";
const TEMPLATE_COLLECTION: &str = "app-settings";
const DOCX_MIME: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const SPK_KEY: &str = "kontrak_template_spk";
const BAP_KEY: &str = "kontrak_template_bap";
const COVER_AM_KEY: &str = "kontrak_template_cover_am";
const COVER_SAN_KEY: &str = "kontrak_template_cover_san";
const STATUS_DISETUJUI: &str = "disetujui";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Nama berkas template default (`KontrakTemplateService::TEMPLATES`).
fn template_default(key: &str) -> Option<&'static str> {
    match key {
        SPK_KEY => Some("SPK_Template.docx"),
        BAP_KEY => Some("bap_template.docx"),
        COVER_AM_KEY => Some("cover_kontrak_am.docx"),
        COVER_SAN_KEY => Some("cover_kontrak_san.docx"),
        _ => None,
    }
}

/// Direktori template default. Dapat diganti dengan `KONTRAK_TEMPLATE_DIR`.
fn template_dir() -> PathBuf {
    std::env::var_os("KONTRAK_TEMPLATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../storage/app/templates"
            ))
        })
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn timestamp() -> String {
    Utc::now().format("%Y%m%d%H%M%S").to_string()
}

/// `Str::slug`: huruf dan angka ASCII dalam huruf kecil, selain itu dipisah `-`.
fn slug(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn query_pairs(raw: Option<String>) -> Vec<(String, String)> {
    let Ok(url) = reqwest::Url::parse(&format!("http://localhost/?{}", raw.unwrap_or_default()))
    else {
        return Vec::new();
    };
    url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn str_col(row: &sqlx::mysql::MySqlRow, name: &str) -> Result<Option<String>, ApiError> {
    row.try_get::<Option<String>, _>(name).map_err(internal)
}

// ---------------------------------------------------------------------------
// Konteks dari database
// ---------------------------------------------------------------------------

async fn load_pekerjaan(
    pool: &MySqlPool,
    id: i64,
) -> Result<Option<(PekerjaanDoc, Option<i64>)>, ApiError> {
    let Some(row) = sqlx::query(
        "SELECT CAST(p.id AS SIGNED) AS id, CAST(p.nama_paket AS CHAR) AS nama_paket, \
         CAST(p.pagu AS DOUBLE) AS pagu, CAST(p.kode_rekening AS CHAR) AS kode_rekening, \
         CAST(p.kegiatan_id AS SIGNED) AS kegiatan_id, CAST(k.n_kec AS CHAR) AS nama_kecamatan, \
         CAST(d.n_desa AS CHAR) AS nama_desa \
         FROM tbl_pekerjaan p \
         LEFT JOIN tbl_kecamatan k ON k.id = p.kecamatan_id \
         LEFT JOIN tbl_desa d ON d.id = p.desa_id \
         WHERE p.id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?
    else {
        return Ok(None);
    };
    let doc = PekerjaanDoc {
        id: row.try_get("id").map_err(internal)?,
        nama_paket: str_col(&row, "nama_paket")?,
        pagu: row.try_get("pagu").map_err(internal)?,
        kode_rekening: str_col(&row, "kode_rekening")?,
        nama_kecamatan: str_col(&row, "nama_kecamatan")?,
        nama_desa: str_col(&row, "nama_desa")?,
    };
    let kegiatan_id: Option<i64> = row.try_get("kegiatan_id").map_err(internal)?;
    Ok(Some((doc, kegiatan_id)))
}

async fn load_kegiatan(pool: &MySqlPool, id: i64) -> Result<Option<KegiatanDoc>, ApiError> {
    let Some(row) = sqlx::query(
        "SELECT CAST(nama_program AS CHAR) AS nama_program, CAST(nama_kegiatan AS CHAR) AS nama_kegiatan, \
         CAST(nama_sub_kegiatan AS CHAR) AS nama_sub_kegiatan, CAST(sub_bidang AS CHAR) AS sub_bidang, \
         CAST(tahun_anggaran AS SIGNED) AS tahun_anggaran, CAST(sumber_dana AS CHAR) AS sumber_dana, \
         CAST(nama_pptk AS CHAR) AS nama_pptk, CAST(nip_pptk AS CHAR) AS nip_pptk \
         FROM tbl_kegiatan WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?
    else {
        return Ok(None);
    };
    Ok(Some(KegiatanDoc {
        nama_program: str_col(&row, "nama_program")?,
        nama_kegiatan: str_col(&row, "nama_kegiatan")?,
        nama_sub_kegiatan: str_col(&row, "nama_sub_kegiatan")?,
        sub_bidang: str_col(&row, "sub_bidang")?,
        tahun_anggaran: row.try_get("tahun_anggaran").map_err(internal)?,
        sumber_dana: str_col(&row, "sumber_dana")?,
        nama_pptk: str_col(&row, "nama_pptk")?,
        nip_pptk: str_col(&row, "nip_pptk")?,
    }))
}

async fn load_penyedia(pool: &MySqlPool, id: i64) -> Result<Option<PenyediaDoc>, ApiError> {
    let Some(row) = sqlx::query(
        "SELECT CAST(nama AS CHAR) AS nama, CAST(direktur AS CHAR) AS direktur, \
         CAST(alamat AS CHAR) AS alamat, CAST(bank AS CHAR) AS bank, CAST(norek AS CHAR) AS norek, \
         CAST(npwp AS CHAR) AS npwp, CAST(no_akta AS CHAR) AS no_akta, CAST(notaris AS CHAR) AS notaris, \
         CAST(tanggal_akta AS DATE) AS tanggal_akta FROM tbl_penyedia WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?
    else {
        return Ok(None);
    };
    Ok(Some(PenyediaDoc {
        nama: str_col(&row, "nama")?,
        direktur: str_col(&row, "direktur")?,
        alamat: str_col(&row, "alamat")?,
        bank: str_col(&row, "bank")?,
        norek: str_col(&row, "norek")?,
        npwp: str_col(&row, "npwp")?,
        no_akta: str_col(&row, "no_akta")?,
        notaris: str_col(&row, "notaris")?,
        tanggal_akta: row.try_get("tanggal_akta").map_err(internal)?,
    }))
}

async fn load_registers(pool: &MySqlPool, kontrak_id: i64) -> Result<Vec<RegisterDoc>, ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(r.id AS SIGNED) AS id, CAST(t.code AS CHAR) AS code, CAST(t.name AS CHAR) AS name, \
         CAST(r.nomor AS CHAR) AS nomor, CAST(r.tanggal AS DATE) AS tanggal, CAST(r.nilai AS DOUBLE) AS nilai \
         FROM tbl_document_registers r LEFT JOIN tbl_document_types t ON t.id = r.type_id \
         WHERE r.kontrak_id = ? ORDER BY r.id",
    )
    .bind(kontrak_id)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    rows.iter()
        .map(|r| -> Result<RegisterDoc, sqlx::Error> {
            Ok(RegisterDoc {
                id: r.try_get("id")?,
                code: r.try_get("code")?,
                name: r.try_get("name")?,
                nomor: r.try_get("nomor")?,
                tanggal: r.try_get("tanggal")?,
                nilai: r.try_get("nilai")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()
        .map_err(internal)
}

/// Konteks lengkap satu kontrak, sama dengan relasi yang dimuat Laravel sebelum membangun dokumen.
pub(crate) async fn load_ctx(pool: &MySqlPool, kontrak: KontrakRow) -> Result<DocCtx, ApiError> {
    // `pekerjaans->first()`: pivot `kontrak_pekerjaan` diurutkan menurut id pekerjaan.
    let pekerjaan_id: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(pekerjaan_id AS SIGNED) FROM kontrak_pekerjaan WHERE kontrak_id = ? ORDER BY pekerjaan_id LIMIT 1",
    )
    .bind(kontrak.id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;

    let (pekerjaan, kegiatan) = match pekerjaan_id {
        Some(id) => match load_pekerjaan(pool, id).await? {
            Some((doc, kegiatan_id)) => {
                let kegiatan = match kegiatan_id {
                    Some(kid) => load_kegiatan(pool, kid).await?,
                    None => None,
                };
                (Some(doc), kegiatan)
            }
            None => (None, None),
        },
        None => (None, None),
    };

    let kontrak_sub_bidang = match kontrak.id_kegiatan {
        Some(kid) => sqlx::query_scalar::<_, Option<String>>(
            "SELECT CAST(sub_bidang AS CHAR) FROM tbl_kegiatan WHERE id = ?",
        )
        .bind(kid)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .flatten(),
        None => None,
    };

    let penyedia = match kontrak.id_penyedia {
        Some(pid) => load_penyedia(pool, pid).await?,
        None => None,
    };

    let registers = load_registers(pool, kontrak.id).await?;

    let addendum_sql = format!("{SELECT_ADDENDUM} WHERE kontrak_id = ? ORDER BY addendum_ke, id");
    let addendums = sqlx::query(&addendum_sql)
        .bind(kontrak.id)
        .fetch_all(pool)
        .await
        .map_err(internal)?
        .iter()
        .map(kontrak::map_addendum)
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?;

    Ok(DocCtx {
        kontrak,
        pekerjaan,
        kegiatan,
        kontrak_sub_bidang,
        penyedia,
        registers,
        addendums,
    })
}

/// `GetOrCreate` pengaturan dokumen: nilai `app_settings` lalu default SPSE dari environment.
async fn load_settings(pool: &MySqlPool) -> Result<DocSettings, ApiError> {
    let read =
        |key: &'static str| async move { mailer::setting(pool, key).await.map_err(internal) };
    let raw = RawDocSettings {
        nama_ppk: read("kontrak_nama_ppk").await?,
        nip_ppk: read("kontrak_nip_ppk").await?,
        nama_pptk: read("kontrak_nama_pptk").await?,
        nip_pptk: read("kontrak_nip_pptk").await?,
        skpd: read("kontrak_skpd").await?,
        nomor_dpa: read("kontrak_nomor_dpa").await?,
        tanggal_dpa: read("kontrak_tanggal_dpa").await?,
        masa_pemeliharaan_hari: read("kontrak_masa_pemeliharaan_hari").await?,
        cara_pembayaran: read("kontrak_cara_pembayaran").await?,
    };
    let default_ppk = (
        env_or("SPSE_PPK_NAMA", "AGUNG DELI SAHPUTRA, ST"),
        env_or("SPSE_PPK_NIP", "197711212006041010"),
    );
    let default_cara = env_or("SPSE_CARA_PEMBAYARAN", "Sekaligus");
    Ok(DocSettings::from_raw(raw, default_ppk, &default_cara))
}

/// Template dari media `app-settings` bila diunggah, selain itu berkas default (`resolvePath`).
async fn resolve_template(pool: &MySqlPool, key: &str) -> Result<PathBuf, ApiError> {
    let default = template_default(key).ok_or_else(|| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Unknown kontrak template key: {key}"),
        )
    })?;
    let setting_id: Option<u64> = sqlx::query_scalar(
        "SELECT CAST(id AS UNSIGNED) FROM app_settings WHERE `key` = ? ORDER BY id LIMIT 1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    if let Some(id) = setting_id {
        if let Some(info) = media::first_media(pool, APP_SETTING_MODEL, id, TEMPLATE_COLLECTION)
            .await
            .map_err(internal)?
        {
            let path = media::media_dir(info.id).join(&info.file_name);
            if path.exists() {
                return Ok(path);
            }
        }
    }
    let path = template_dir().join(default);
    if !path.exists() {
        return Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Template tidak ditemukan: {default}"),
        ));
    }
    Ok(path)
}

// ---------------------------------------------------------------------------
// Render dan respons
// ---------------------------------------------------------------------------

/// Mengisi template `key` dengan data kontrak. Mengembalikan isi `.docx` dan nama unduhannya.
async fn render_docx(
    pool: &MySqlPool,
    ctx: &DocCtx,
    key: &str,
    overrides: &Overrides,
) -> Result<(Vec<u8>, String), ApiError> {
    let settings = load_settings(pool).await?;
    let template = resolve_template(pool, key).await?;
    let template_bytes = tokio::fs::read(&template).await.map_err(internal)?;
    let values = data::build(ctx, &settings, overrides)
        .map_err(|m| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, m))?;
    let filled = docx_template::fill(&template_bytes, &values).map_err(internal)?;
    let nama_paket = ctx
        .pekerjaan
        .as_ref()
        .and_then(|p| p.nama_paket.as_deref())
        .unwrap_or("");
    let name = format!("Kontrak_{}_{}.docx", slug(nama_paket), timestamp());
    Ok((filled, name))
}

/// Pemilihan template cover dari sub bidang (`exportCover`).
fn cover_key(sub_bidang: &str) -> Result<&'static str, ApiError> {
    let lower = sub_bidang.to_lowercase();
    if lower.contains("air minum") {
        Ok(COVER_AM_KEY)
    } else if lower.contains("sanitasi") {
        Ok(COVER_SAN_KEY)
    } else {
        Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Template cover kontrak untuk sub bidang ini belum tersedia.",
        ))
    }
}

fn file_response(bytes: Vec<u8>, name: &str, content_type: &str) -> Response {
    let safe: String = name
        .chars()
        .filter(|c| !matches!(c, '"' | '\r' | '\n'))
        .collect();
    (
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{safe}\""),
            ),
        ],
        bytes,
    )
        .into_response()
}

fn parse_id(raw: &str) -> Result<i64, ApiError> {
    raw.parse().map_err(|_| ApiError::not_found())
}

async fn find_kontrak(pool: &MySqlPool, id: i64) -> Result<KontrakRow, ApiError> {
    kontrak::find_row(pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)
}

/// `Kontrak::whereHas('pekerjaans', whereKey(id))->latest()`: kontrak terbaru untuk satu pekerjaan.
async fn kontrak_of_pekerjaan(
    pool: &MySqlPool,
    pekerjaan_id: i64,
) -> Result<Option<KontrakRow>, ApiError> {
    let kontrak_id: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(kp.kontrak_id AS SIGNED) FROM kontrak_pekerjaan kp \
         JOIN tbl_kontrak k ON k.id = kp.kontrak_id \
         WHERE kp.pekerjaan_id = ? ORDER BY k.created_at DESC, k.id DESC LIMIT 1",
    )
    .bind(pekerjaan_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    match kontrak_id {
        Some(id) => kontrak::find_row(pool, id).await.map_err(internal),
        None => Ok(None),
    }
}

/// `GET /api/kontrak/{id}/export?format=docx|pdf`: SPK kontrak. Id pekerjaan dipakai bila bukan id kontrak.
pub async fn export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let id = parse_id(&id)?;
    let kontrak = match kontrak::find_row(&state.pool, id).await.map_err(internal)? {
        Some(k) => k,
        None => kontrak_of_pekerjaan(&state.pool, id)
            .await?
            .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "Kontrak not found"))?,
    };
    let format = query.get("format").map(String::as_str).unwrap_or("docx");
    let ctx = load_ctx(&state.pool, kontrak).await?;

    let exported = async {
        let (docx, name) = render_docx(&state.pool, &ctx, SPK_KEY, &Overrides::default()).await?;
        if format != "pdf" {
            return Ok::<_, ApiError>((docx, name, DOCX_MIME));
        }
        let pdf = onlyoffice::convert_docx_to_pdf(&state.app_url, &docx, &name)
            .await
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Gagal konversi ke PDF: ONLYOFFICE tidak mengembalikan hasil.",
                )
            })?;
        let pdf_name = format!("{}.pdf", name.trim_end_matches(".docx"));
        Ok((pdf, pdf_name, "application/pdf"))
    }
    .await;

    match exported {
        Ok((bytes, name, mime)) => Ok(file_response(bytes, &name, mime)),
        Err(e) => Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Export failed: {}", e.message),
        )),
    }
}

/// `GET /api/kontrak/{id}/export-cover`: cover kontrak sesuai sub bidang.
pub async fn export_cover(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let kontrak = find_kontrak(&state.pool, parse_id(&id)?).await?;
    let ctx = load_ctx(&state.pool, kontrak).await?;
    let key = cover_key(&ctx.sub_bidang())?;
    let (bytes, name) = render_docx(&state.pool, &ctx, key, &Overrides::default()).await?;
    Ok(file_response(bytes, &name, DOCX_MIME))
}

/// `GET /api/kontrak/export-all-covers?tahun=`: ZIP berisi cover semua kontrak (per tahun anggaran bila diisi).
pub async fn export_all_covers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let pool = &state.pool;
    let tahun: Option<i64> = match query
        .get("tahun")
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        None => None,
        Some(raw) => match raw.parse::<i64>() {
            Ok(t) if (2000..=2100).contains(&t) => Some(t),
            _ => {
                let mut errors = BTreeMap::new();
                errors.insert(
                    "tahun".to_string(),
                    vec!["The tahun field must be between 2000 and 2100.".to_string()],
                );
                return Err(ApiError::validation(
                    "The tahun field must be between 2000 and 2100.",
                    errors,
                ));
            }
        },
    };

    let ids: Vec<i64> = match tahun {
        Some(t) => sqlx::query_scalar(
            "SELECT CAST(id AS SIGNED) FROM tbl_kontrak \
             WHERE id_kegiatan IN (SELECT id FROM tbl_kegiatan WHERE tahun_anggaran = ?) ORDER BY id",
        )
        .bind(t)
        .fetch_all(pool)
        .await
        .map_err(internal)?,
        None => sqlx::query_scalar("SELECT CAST(id AS SIGNED) FROM tbl_kontrak ORDER BY id")
            .fetch_all(pool)
            .await
            .map_err(internal)?,
    };
    if ids.is_empty() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "Tidak ada kontrak untuk diekspor",
        ));
    }

    let unsafe_chars = Regex::new(r"[^A-Za-z0-9_.\-]+").map_err(internal)?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let mut used: HashSet<String> = HashSet::new();
    let mut errors: Vec<String> = Vec::new();
    let mut count = 0usize;

    for id in ids {
        let Some(kontrak) = kontrak::find_row(pool, id).await.map_err(internal)? else {
            continue;
        };
        let ctx = match load_ctx(pool, kontrak).await {
            Ok(ctx) => ctx,
            Err(e) => {
                errors.push(format!("kontrak {id}: {}", e.message));
                continue;
            }
        };
        let nama = ctx
            .pekerjaan
            .as_ref()
            .and_then(|p| p.nama_paket.clone())
            .or_else(|| ctx.kontrak.kode_paket.clone())
            .unwrap_or_else(|| format!("kontrak_{id}"));

        let rendered = match cover_key(&ctx.sub_bidang()) {
            Ok(key) => render_docx(pool, &ctx, key, &Overrides::default()).await,
            Err(e) => Err(e),
        };
        let (bytes, _) = match rendered {
            Ok(file) => file,
            Err(e) => {
                errors.push(format!("{nama}: {}", e.message));
                continue;
            }
        };

        let safe = unsafe_chars.replace_all(&nama, "_");
        let mut entry = format!("cover_{safe}.docx");
        if !used.insert(entry.clone()) {
            entry = format!("cover_{safe}_{id}.docx");
            used.insert(entry.clone());
        }
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file(entry, options).map_err(internal)?;
        zip.write_all(&bytes).map_err(internal)?;
        count += 1;
    }

    if count == 0 {
        return Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Tidak ada cover yang bisa dibuat: {}", errors.join("; ")),
        ));
    }
    let archive = zip.finish().map_err(internal)?.into_inner();
    let name = format!(
        "cover_kontrak{}.zip",
        tahun.map(|t| format!("_{t}")).unwrap_or_default()
    );
    Ok(file_response(archive, &name, "application/zip"))
}

/// Serialisasi register untuk `bapContext`.
fn register_json(register: Option<&RegisterDoc>) -> Value {
    match register {
        None => Value::Null,
        Some(r) => json!({
            "register_id": r.id,
            "nomor": r.nomor,
            "tanggal": r.tanggal.map(|d| d.format("%Y-%m-%d").to_string()),
            "nilai": r.nilai,
            "type_code": r.code,
            "type_name": r.name,
        }),
    }
}

/// `KontrakBapContextService::build`.
fn bap_context_json(ctx: &DocCtx, gaps: Vec<Value>) -> Value {
    let bastp = ctx.find_register("BASTP");
    let jaminan_um = ctx.find_register("JAMINAN_UM");
    let uang_muka = ctx.find_register("UANG_MUKA");
    let latest = ctx.latest_approved();
    let missing: Vec<&str> = if bastp.is_none() {
        vec!["bastp"]
    } else {
        Vec::new()
    };
    let date = |d: Option<NaiveDate>| d.map(|d| d.format("%Y-%m-%d").to_string());

    // `where('status', '!=', 'disetujui')` lalu `sortByDesc('addendum_ke')`.
    let pending: Vec<Value> = ctx
        .addendums
        .iter()
        .filter(|a| a.status != STATUS_DISETUJUI)
        .rev()
        .map(|a| {
            json!({
                "id": a.id,
                "addendum_ke": a.addendum_ke,
                "nomor": a.nomor_addendum,
                "tanggal": date(a.tanggal_addendum),
                "status": a.status,
                "nilai_kontrak_sesudah": a.nilai_kontrak_sesudah,
            })
        })
        .collect();

    json!({
        "can_generate": missing.is_empty(),
        "missing": missing,
        "nilai_kontrak_efektif": latest.and_then(|a| a.nilai_kontrak_sesudah).or(ctx.kontrak.nilai_kontrak),
        "nilai_kontrak_awal": ctx.kontrak.nilai_kontrak,
        "bastp": register_json(bastp),
        "addendum": latest.map(|a| json!({
            "id": a.id,
            "addendum_ke": a.addendum_ke,
            "nomor": a.nomor_addendum,
            "tanggal": date(a.tanggal_addendum),
            "nilai_kontrak_sesudah": a.nilai_kontrak_sesudah,
        })),
        "addendum_register_gaps": gaps,
        "pending_addendums": pending,
        "jaminan_uang_muka": register_json(jaminan_um),
        "uang_muka": register_json(uang_muka),
        "pekerjaan": ctx.pekerjaan.as_ref().map(|p| json!({
            "id": p.id,
            "nama_paket": p.nama_paket,
        })),
    })
}

/// `GET /api/kontrak/{id}/bap-context`.
pub async fn bap_context(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let kontrak = find_kontrak(&state.pool, parse_id(&id)?).await?;
    let kontrak_id = kontrak.id;
    let ctx = load_ctx(&state.pool, kontrak).await?;
    let gaps: Vec<Value> = kontrak_register_gap::gap_items(&state.pool)
        .await?
        .into_iter()
        .map(|(_, v)| v)
        .filter(|v| v["kontrak_id"] == json!(kontrak_id))
        .collect();
    Ok(Json(bap_context_json(&ctx, gaps)).into_response())
}

/// `GET /api/kontrak/{id}/export-bap`: BAP dengan override dari query string.
pub async fn export_bap(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiError> {
    require_auth(&state, &headers).await?;
    let kontrak = find_kontrak(&state.pool, parse_id(&id)?).await?;
    let ctx = load_ctx(&state.pool, kontrak).await?;
    if ctx.find_register("BASTP").is_none() {
        return Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "message": "Register dokumen BASTP belum tersedia untuk kontrak ini.",
                "missing": ["bastp"],
            })),
        )
            .into_response());
    }
    let overrides = Overrides::from_pairs(query_pairs(raw));
    let (bytes, name) = render_docx(&state.pool, &ctx, BAP_KEY, &overrides).await?;
    Ok(file_response(bytes, &name, DOCX_MIME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_matches_laravel_for_common_names() {
        assert_eq!(slug("Jalan & Jembatan Cianjur"), "jalan-jembatan-cianjur");
        assert_eq!(slug(""), "");
    }

    #[test]
    fn cover_key_follows_sub_bidang() {
        assert_eq!(cover_key("Air Minum").unwrap(), COVER_AM_KEY);
        assert_eq!(cover_key("Sanitasi Lingkungan").unwrap(), COVER_SAN_KEY);
        assert!(cover_key("Lainnya").is_err());
    }

    #[test]
    fn query_pairs_decode_values() {
        let pairs = query_pairs(Some("nama=Jalan%20Raya&nilai_lain=5".to_string()));
        assert_eq!(pairs[0], ("nama".to_string(), "Jalan Raya".to_string()));
        assert_eq!(pairs[1], ("nilai_lain".to_string(), "5".to_string()));
    }
}
