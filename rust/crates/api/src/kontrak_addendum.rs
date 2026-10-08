//! Addendum kontrak: `kontrak-addendums` dan rute addendum di bawah `kontrak/{id}`.
//!
//! Mengikuti `KontrakAddendumController` di Laravel. Yang belum dipindah: `register-gaps`
//! (`GET /kontrak-addendums/register-gaps`, `POST .../notify-pengawas`, dan
//! `GET /kontrak/{id}/addendum-register-gaps`) karena bergantung pada service gap register.

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Datelike, NaiveDate};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row, Transaction};

use crate::{
    access, audit, format::number_like_php, foto, kontrak, lookup::carbon_json, media, pagination,
    require_auth, AppState,
};

const MODEL: &str = "App\\Models\\KontrakAddendum";
const COLLECTION: &str = "kontrak/addendum";
const PER_PAGE: u64 = 20;
const UPLOAD_EXT: [&str; 8] = ["pdf", "doc", "docx", "xls", "xlsx", "jpg", "jpeg", "png"];
const UPLOAD_MAX_KB: usize = 10_240;
const JENIS: [&str; 5] = ["teknis", "biaya", "waktu", "teknis_biaya", "lainnya"];

/// Lampiran wajib dan labelnya (urutan sama dengan Laravel).
const ATTACHMENT_TYPES: [(&str, &str); 9] = [
    ("cco", "CCO"),
    ("dokumen_nego_addendum", "Dokumen Nego Addendum"),
    (
        "surat_permohonan_pembahasan",
        "Surat Permohonan Pembahasan Adendum (Penyedia)",
    ),
    (
        "surat_undangan_pembahasan",
        "Surat Undangan Pembahasan (PPK)",
    ),
    (
        "berita_acara_negosiasi_harga",
        "Berita Acara Negosiasi Harga Item Pekerjaan Baru",
    ),
    (
        "risalah_rapat_pembahasan",
        "Risalah Rapat Pembahasan Adendum",
    ),
    ("berita_acara_penelitian", "Berita Acara Penelitian"),
    ("ba_cco_addendum", "BA CCO & Adendum Kontrak"),
    (
        "surat_perintah_pelaksanaan",
        "Surat Perintah Pelaksanaan (PPK)",
    ),
];

fn label_of(key: &str) -> Option<&'static str> {
    ATTACHMENT_TYPES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, l)| *l)
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn forbidden(msg: &str) -> ApiError {
    ApiError::new(StatusCode::FORBIDDEN, msg)
}

fn unprocessable(msg: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, msg)
}

fn fmt_date(d: Option<NaiveDate>) -> Value {
    d.map_or(Value::Null, |d| json!(d.format("%Y-%m-%d").to_string()))
}

/// Nama atribut untuk pesan validasi: `addendum_ke` menjadi `addendum ke`.
fn attr_name(key: &str) -> String {
    key.replace('_', " ")
}

// ---------------------------------------------------------------------------
// Otorisasi
// ---------------------------------------------------------------------------

pub(crate) struct Actor {
    pub(crate) user_id: u64,
    pub(crate) roles: Vec<(u64, String)>,
}

pub(crate) async fn actor(state: &AppState, headers: &HeaderMap) -> Result<Actor, ApiError> {
    let user = require_auth(state, headers).await?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    Ok(Actor {
        user_id: user.user_id,
        roles,
    })
}

impl Actor {
    pub(crate) fn is_admin(&self) -> bool {
        self.roles.iter().any(|(_, n)| n == "admin")
    }

    fn is_pengawas(&self) -> bool {
        self.roles
            .iter()
            .any(|(_, n)| n.to_lowercase() == "pengawas")
    }
}

pub(crate) fn authorize_admin(a: &Actor) -> Result<(), ApiError> {
    if a.is_admin() {
        Ok(())
    } else {
        Err(forbidden("Hanya admin yang boleh melakukan aksi ini"))
    }
}

/// `Pekerjaan::byUserRole()->whereKey($id)->exists()`.
async fn can_access_pekerjaan(
    state: &AppState,
    a: &Actor,
    pekerjaan_id: Option<i64>,
) -> Result<bool, ApiError> {
    match pekerjaan_id {
        None => Ok(false),
        Some(p) => access::user_can_access(&state.pool, a.user_id, &a.roles, p as u64)
            .await
            .map_err(internal),
    }
}

pub(crate) async fn authorize_view_kontrak(
    state: &AppState,
    a: &Actor,
    kontrak: &kontrak::KontrakRow,
) -> Result<(), ApiError> {
    if a.is_admin() {
        return Ok(());
    }
    if can_access_pekerjaan(state, a, kontrak.id_pekerjaan).await? {
        Ok(())
    } else {
        Err(forbidden("Anda tidak memiliki akses ke kontrak ini"))
    }
}

/// `authorizeCreate`: admin, atau pengawas untuk pekerjaan yang diassign.
async fn authorize_create(
    state: &AppState,
    a: &Actor,
    kontrak: &kontrak::KontrakRow,
) -> Result<(), ApiError> {
    if a.is_admin() {
        return Ok(());
    }
    if !a.is_pengawas() {
        return Err(forbidden(
            "Hanya admin atau Pengawas yang boleh membuat pengajuan addendum",
        ));
    }
    if can_access_pekerjaan(state, a, kontrak.id_pekerjaan).await? {
        Ok(())
    } else {
        Err(forbidden(
            "Pengawas hanya boleh mengajukan addendum untuk pekerjaan yang diassign",
        ))
    }
}

/// `authorizeSubmit`: sama dengan create, dengan pesan untuk pengajuan.
async fn authorize_submit(
    state: &AppState,
    a: &Actor,
    kontrak: &kontrak::KontrakRow,
) -> Result<(), ApiError> {
    if a.is_admin() {
        return Ok(());
    }
    if !a.is_pengawas() {
        return Err(forbidden(
            "Hanya admin atau Pengawas yang boleh mengajukan addendum",
        ));
    }
    if can_access_pekerjaan(state, a, kontrak.id_pekerjaan).await? {
        Ok(())
    } else {
        Err(forbidden(
            "Pengawas hanya boleh mengajukan addendum untuk pekerjaan yang diassign",
        ))
    }
}

fn ensure_editable(row: &kontrak::AddendumRow) -> Result<(), ApiError> {
    if row.status == "disetujui" {
        Err(unprocessable(
            "Addendum yang sudah disetujui tidak bisa diubah",
        ))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Muat data dan resource
// ---------------------------------------------------------------------------

async fn find_addendum(pool: &MySqlPool, id: i64) -> Result<kontrak::AddendumRow, ApiError> {
    let sql = format!("{} WHERE id = ?", kontrak::SELECT_ADDENDUM);
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    kontrak::map_addendum(&row).map_err(internal)
}

/// Kontrak induk dari addendum. Baris yang hilang dianggap tidak ditemukan.
pub(crate) async fn find_kontrak(
    pool: &MySqlPool,
    id: i64,
) -> Result<kontrak::KontrakRow, ApiError> {
    kontrak::find_row(pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)
}

/// Bagian resource yang ikut dimuat, sesuai `with()` pada tiap aksi di Laravel.
#[derive(Clone, Copy, Default)]
struct Include {
    items: bool,
    people: bool,
    kontrak: bool,
}

async fn user_summary(pool: &MySqlPool, id: Option<i64>) -> Result<Value, ApiError> {
    let Some(id) = id else {
        return Ok(Value::Null);
    };
    let row = sqlx::query("SELECT CAST(id AS SIGNED), name FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(internal)?;
    Ok(match row {
        Some(r) => json!({
            "id": r.try_get::<i64, _>(0).map_err(internal)?,
            "name": r.try_get::<String, _>(1).map_err(internal)?,
        }),
        None => Value::Null,
    })
}

async fn pekerjaan_summary(pool: &MySqlPool, id: Option<i64>) -> Result<Value, ApiError> {
    let Some(id) = id else {
        return Ok(Value::Null);
    };
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED), nama_paket, kode_rekening FROM tbl_pekerjaan WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    Ok(match row {
        Some(r) => json!({
            "id": r.try_get::<i64, _>(0).map_err(internal)?,
            "nama_paket": r.try_get::<Option<String>, _>(1).map_err(internal)?,
            "kode_rekening": r.try_get::<Option<String>, _>(2).map_err(internal)?,
        }),
        None => Value::Null,
    })
}

/// Ringkasan kontrak untuk `KontrakAddendumResource` (`kontrak` pada index `all` dan `show`).
async fn kontrak_summary(pool: &MySqlPool, kontrak_id: i64) -> Result<Value, ApiError> {
    let Some(k) = kontrak::find_row(pool, kontrak_id)
        .await
        .map_err(internal)?
    else {
        return Ok(Value::Null);
    };
    let pekerjaans = sqlx::query(
        "SELECT CAST(p.id AS SIGNED), p.nama_paket, p.kode_rekening FROM kontrak_pekerjaan kp \
         JOIN tbl_pekerjaan p ON p.id = kp.pekerjaan_id WHERE kp.kontrak_id = ? ORDER BY p.id",
    )
    .bind(kontrak_id)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    let mut list = Vec::with_capacity(pekerjaans.len());
    for r in &pekerjaans {
        list.push(json!({
            "id": r.try_get::<i64, _>(0).map_err(internal)?,
            "nama_paket": r.try_get::<Option<String>, _>(1).map_err(internal)?,
            "kode_rekening": r.try_get::<Option<String>, _>(2).map_err(internal)?,
        }));
    }
    let penyedia = match k.id_penyedia {
        None => Value::Null,
        Some(pid) => {
            let row = sqlx::query("SELECT CAST(id AS SIGNED), nama FROM tbl_penyedia WHERE id = ?")
                .bind(pid)
                .fetch_optional(pool)
                .await
                .map_err(internal)?;
            match row {
                Some(r) => json!({
                    "id": r.try_get::<i64, _>(0).map_err(internal)?,
                    "nama": r.try_get::<Option<String>, _>(1).map_err(internal)?,
                }),
                None => Value::Null,
            }
        }
    };
    Ok(json!({
        "id": k.id,
        "spk": k.spk,
        "kode_paket": k.kode_paket,
        "nilai_kontrak": k.nilai_kontrak.map_or(Value::Null, number_like_php),
        "tgl_selesai": fmt_date(k.tgl_selesai),
        "pekerjaan": pekerjaan_summary(pool, k.id_pekerjaan).await?,
        "pekerjaans": list,
        "penyedia": penyedia,
    }))
}

/// `KontrakAddendumResource` dengan relasi yang dimuat sesuai aksi.
async fn resource(
    state: &AppState,
    row: &kontrak::AddendumRow,
    inc: Include,
) -> Result<Value, ApiError> {
    let mut out = kontrak::addendum_json(state, row, inc.items).await?;
    let obj = out
        .as_object_mut()
        .ok_or_else(|| internal("resource addendum bukan objek"))?;
    obj.insert(
        "attachments".into(),
        Value::Array(kontrak::attachments(state, row.id).await?),
    );
    if inc.people {
        obj.insert(
            "creator".into(),
            user_summary(&state.pool, row.created_by).await?,
        );
        obj.insert(
            "approver".into(),
            user_summary(&state.pool, row.approved_by).await?,
        );
    }
    if inc.kontrak {
        obj.insert(
            "kontrak".into(),
            kontrak_summary(&state.pool, row.kontrak_id).await?,
        );
    }
    Ok(out)
}

/// Atribut untuk audit: kolom tabel, JSON `attachment_nomors` diurai.
pub(crate) fn attributes(row: &kontrak::AddendumRow) -> Map<String, Value> {
    let attachment: Value = row
        .attachment_nomors
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null);
    let mut m = Map::new();
    m.insert("id".into(), json!(row.id));
    m.insert("kontrak_id".into(), json!(row.kontrak_id));
    m.insert("addendum_ke".into(), json!(row.addendum_ke));
    m.insert("nomor_addendum".into(), json!(row.nomor_addendum));
    m.insert("attachment_nomors".into(), attachment);
    m.insert("tanggal_addendum".into(), fmt_date(row.tanggal_addendum));
    m.insert("jenis_addendum".into(), json!(row.jenis_addendum));
    m.insert("alasan".into(), json!(row.alasan));
    m.insert("deskripsi_perubahan".into(), json!(row.deskripsi_perubahan));
    m.insert(
        "nilai_kontrak_sebelum".into(),
        row.nilai_kontrak_sebelum
            .map_or(Value::Null, number_like_php),
    );
    m.insert(
        "nilai_kontrak_sesudah".into(),
        row.nilai_kontrak_sesudah
            .map_or(Value::Null, number_like_php),
    );
    m.insert(
        "tgl_selesai_sebelum".into(),
        fmt_date(row.tgl_selesai_sebelum),
    );
    m.insert(
        "tgl_selesai_sesudah".into(),
        fmt_date(row.tgl_selesai_sesudah),
    );
    m.insert("status".into(), json!(row.status));
    m.insert(
        "kelengkapan_override".into(),
        json!(row.kelengkapan_override),
    );
    m.insert("created_by".into(), json!(row.created_by));
    m.insert("approved_by".into(), json!(row.approved_by));
    m.insert("approved_at".into(), carbon_json(row.approved_at));
    m.insert("created_at".into(), carbon_json(row.created_at));
    m.insert("updated_at".into(), carbon_json(row.updated_at));
    m
}

/// Hanya kolom yang berubah (`getDirty`). `updated_at` ikut bila ada perubahan.
fn dirty(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Option<(Map<String, Value>, Map<String, Value>)> {
    let mut old = Map::new();
    let mut new = Map::new();
    for (k, v) in after {
        if k == "updated_at" {
            continue;
        }
        if before.get(k) != Some(v) {
            old.insert(k.clone(), before.get(k).cloned().unwrap_or(Value::Null));
            new.insert(k.clone(), v.clone());
        }
    }
    if new.is_empty() {
        return None;
    }
    old.insert(
        "updated_at".into(),
        before.get("updated_at").cloned().unwrap_or(Value::Null),
    );
    new.insert(
        "updated_at".into(),
        after.get("updated_at").cloned().unwrap_or(Value::Null),
    );
    Some((old, new))
}

#[allow(clippy::too_many_arguments)]
async fn audit(
    tx: &mut Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    event: &str,
    id: i64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    url: &str,
) -> Result<(), ApiError> {
    audit::write(
        tx,
        audit::Entry {
            actor,
            event,
            auditable_type: MODEL,
            auditable_id: id as u64,
            old,
            new,
            url,
        },
        headers,
    )
    .await
    .map_err(internal)
}

// ---------------------------------------------------------------------------
// Input dan validasi
// ---------------------------------------------------------------------------

/// Field teks kosong dianggap null (`ConvertEmptyStringsToNull`).
fn text_value(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "1".into() } else { "0".into() }),
        _ => None,
    }
}

/// Tanggal dengan aturan `date`. Format yang diterima mirip `strtotime` untuk umum.
fn parse_date_str(raw: &str) -> Option<NaiveDate> {
    let s = raw.trim();
    let date_part = s.split(['T', ' ']).next()?;
    for fmt in ["%Y-%m-%d", "%Y/%m/%d", "%m/%d/%Y", "%d-%m-%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(date_part, fmt) {
            return Some(d);
        }
    }
    None
}

fn numeric(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}

#[derive(Debug, Clone, Default)]
struct ItemInput {
    nama_item: Option<String>,
    spesifikasi_sebelum: Option<String>,
    spesifikasi_sesudah: Option<String>,
    volume_sebelum: Option<f64>,
    volume_sesudah: Option<f64>,
    harga_sebelum: Option<f64>,
    harga_sesudah: Option<f64>,
    subtotal_sebelum: Option<f64>,
    subtotal_sesudah: Option<f64>,
}

/// Input addendum yang sudah divalidasi. `None` = field tidak dikirim (tidak diubah).
#[derive(Debug, Default)]
struct AddendumInput {
    addendum_ke: i64,
    nomor_addendum: Option<Option<String>>,
    tanggal_addendum: NaiveDate,
    jenis_addendum: String,
    alasan: Option<Option<String>>,
    deskripsi_perubahan: Option<Option<String>>,
    nilai_kontrak_sebelum: Option<Option<f64>>,
    nilai_kontrak_sesudah: Option<Option<f64>>,
    tgl_selesai_sebelum: Option<Option<NaiveDate>>,
    tgl_selesai_sesudah: Option<Option<NaiveDate>>,
    items: Option<Vec<ItemInput>>,
}

/// Aturan `validateAddendum`. `current` dipakai untuk mengecualikan baris sendiri pada `unique`.
async fn validate_addendum(
    pool: &MySqlPool,
    body: &Value,
    kontrak_id: i64,
    current: Option<&kontrak::AddendumRow>,
) -> Result<AddendumInput, ApiError> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut input = AddendumInput::default();

    // addendum_ke: wajib, integer >= 1, unik per kontrak.
    match obj.get("addendum_ke").filter(|v| !v.is_null()) {
        None => foto::add(
            &mut errs,
            "addendum_ke",
            format!("The {} field is required.", attr_name("addendum_ke")),
        ),
        Some(v) => match v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        {
            Some(n) if n >= 1 => {
                let taken: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM tbl_kontrak_addendums WHERE kontrak_id = ? AND addendum_ke = ? AND id <> ?",
                )
                .bind(kontrak_id)
                .bind(n)
                .bind(current.map_or(0, |c| c.id))
                .fetch_one(pool)
                .await
                .map_err(internal)?;
                if taken > 0 {
                    foto::add(
                        &mut errs,
                        "addendum_ke",
                        format!("The {} has already been taken.", attr_name("addendum_ke")),
                    );
                }
                input.addendum_ke = n;
            }
            _ => foto::add(
                &mut errs,
                "addendum_ke",
                format!("The {} field must be at least 1.", attr_name("addendum_ke")),
            ),
        },
    }

    // nomor_addendum: nullable, max 100, unik di addendum dan di register dokumen.
    match obj.get("nomor_addendum").and_then(text_value) {
        None => {
            if obj.contains_key("nomor_addendum") {
                input.nomor_addendum = Some(None);
            }
        }
        Some(nomor) => {
            if nomor.chars().count() > 100 {
                foto::add(
                    &mut errs,
                    "nomor_addendum",
                    "The nomor addendum field must not be greater than 100 characters.".into(),
                );
            } else {
                let taken: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM tbl_kontrak_addendums WHERE nomor_addendum = ? AND id <> ?",
                )
                .bind(&nomor)
                .bind(current.map_or(0, |c| c.id))
                .fetch_one(pool)
                .await
                .map_err(internal)?;
                // Nomor lama milik baris ini tidak dihitung (ignore pada register).
                let ignore_reg = current.and_then(|c| c.nomor_addendum.clone());
                let reg_taken: i64 = match ignore_reg {
                    Some(old) => sqlx::query_scalar("SELECT COUNT(*) FROM tbl_document_registers WHERE nomor = ? AND nomor <> ?")
                        .bind(&nomor)
                        .bind(old)
                        .fetch_one(pool)
                        .await
                        .map_err(internal)?,
                    None => sqlx::query_scalar("SELECT COUNT(*) FROM tbl_document_registers WHERE nomor = ?")
                        .bind(&nomor)
                        .fetch_one(pool)
                        .await
                        .map_err(internal)?,
                };
                if taken > 0 || reg_taken > 0 {
                    foto::add(
                        &mut errs,
                        "nomor_addendum",
                        "The nomor addendum has already been taken.".into(),
                    );
                }
                input.nomor_addendum = Some(Some(nomor));
            }
        }
    }

    // tanggal_addendum: wajib, date.
    match obj.get("tanggal_addendum").and_then(text_value) {
        None => foto::add(
            &mut errs,
            "tanggal_addendum",
            format!("The {} field is required.", attr_name("tanggal_addendum")),
        ),
        Some(s) => match parse_date_str(&s) {
            Some(d) => input.tanggal_addendum = d,
            None => foto::add(
                &mut errs,
                "tanggal_addendum",
                format!(
                    "The {} field must be a valid date.",
                    attr_name("tanggal_addendum")
                ),
            ),
        },
    }

    // jenis_addendum: wajib, salah satu dari daftar.
    match obj.get("jenis_addendum").and_then(text_value) {
        None => foto::add(
            &mut errs,
            "jenis_addendum",
            format!("The {} field is required.", attr_name("jenis_addendum")),
        ),
        Some(j) if JENIS.contains(&j.as_str()) => input.jenis_addendum = j,
        Some(_) => foto::add(
            &mut errs,
            "jenis_addendum",
            "The selected jenis addendum is invalid.".into(),
        ),
    }

    input.alasan = optional_text(&obj, "alasan", &mut errs);
    input.deskripsi_perubahan = optional_text(&obj, "deskripsi_perubahan", &mut errs);
    input.nilai_kontrak_sebelum = optional_money(&obj, "nilai_kontrak_sebelum", &mut errs);
    input.nilai_kontrak_sesudah = optional_money(&obj, "nilai_kontrak_sesudah", &mut errs);
    input.tgl_selesai_sebelum = optional_date(&obj, "tgl_selesai_sebelum", &mut errs);
    input.tgl_selesai_sesudah = optional_date(&obj, "tgl_selesai_sesudah", &mut errs);

    match obj.get("items") {
        None => {}
        Some(Value::Null) => input.items = None,
        Some(Value::Array(list)) => {
            let mut items = Vec::with_capacity(list.len());
            for (i, raw) in list.iter().enumerate() {
                let Some(item) = raw.as_object() else {
                    foto::add(
                        &mut errs,
                        &format!("items.{i}"),
                        format!("The items.{i} field must be an array."),
                    );
                    continue;
                };
                items.push(parse_item(item, i, &mut errs));
            }
            input.items = Some(items);
        }
        Some(_) => foto::add(
            &mut errs,
            "items",
            "The items field must be an array.".into(),
        ),
    }

    if errs.is_empty() {
        Ok(input)
    } else {
        Err(ApiError::validation("The given data was invalid.", errs))
    }
}

fn optional_text(
    obj: &Map<String, Value>,
    key: &str,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<Option<String>> {
    let v = obj.get(key)?;
    match text_value(v) {
        None => Some(None),
        Some(s) if v.is_string() || v.is_number() => Some(Some(s)),
        Some(_) => {
            foto::add(
                errs,
                key,
                format!("The {} field must be a string.", attr_name(key)),
            );
            None
        }
    }
}

fn optional_money(
    obj: &Map<String, Value>,
    key: &str,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<Option<f64>> {
    let v = obj.get(key)?;
    if v.is_null() || text_value(v).is_none() {
        return Some(None);
    }
    match numeric(v) {
        Some(n) if n >= 0.0 => Some(Some(n)),
        Some(_) => {
            foto::add(
                errs,
                key,
                format!("The {} field must be at least 0.", attr_name(key)),
            );
            None
        }
        None => {
            foto::add(
                errs,
                key,
                format!("The {} field must be a number.", attr_name(key)),
            );
            None
        }
    }
}

fn optional_date(
    obj: &Map<String, Value>,
    key: &str,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<Option<NaiveDate>> {
    let v = obj.get(key)?;
    match text_value(v) {
        None => Some(None),
        Some(s) => match parse_date_str(&s) {
            Some(d) => Some(Some(d)),
            None => {
                foto::add(
                    errs,
                    key,
                    format!("The {} field must be a valid date.", attr_name(key)),
                );
                None
            }
        },
    }
}

fn parse_item(
    item: &Map<String, Value>,
    index: usize,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> ItemInput {
    let key = |field: &str| format!("items.{index}.{field}");
    let mut out = ItemInput::default();
    if let Some(v) = item.get("nama_item").and_then(text_value) {
        if v.chars().count() > 255 {
            foto::add(
                errs,
                &key("nama_item"),
                "The nama item field must not be greater than 255 characters.".into(),
            );
        } else {
            out.nama_item = Some(v);
        }
    }
    out.spesifikasi_sebelum = item.get("spesifikasi_sebelum").and_then(text_value);
    out.spesifikasi_sesudah = item.get("spesifikasi_sesudah").and_then(text_value);
    let num = |field: &str, min0: bool, errs: &mut BTreeMap<String, Vec<String>>| -> Option<f64> {
        let v = item.get(field).filter(|v| text_value(v).is_some())?;
        match numeric(v) {
            Some(n) if !min0 || n >= 0.0 => Some(n),
            Some(_) => {
                foto::add(
                    errs,
                    &key(field),
                    format!("The {} field must be at least 0.", attr_name(field)),
                );
                None
            }
            None => {
                foto::add(
                    errs,
                    &key(field),
                    format!("The {} field must be a number.", attr_name(field)),
                );
                None
            }
        }
    };
    out.volume_sebelum = num("volume_sebelum", false, errs);
    out.volume_sesudah = num("volume_sesudah", false, errs);
    out.harga_sebelum = num("harga_sebelum", true, errs);
    out.harga_sesudah = num("harga_sesudah", true, errs);
    out.subtotal_sebelum = num("subtotal_sebelum", true, errs);
    out.subtotal_sesudah = num("subtotal_sesudah", true, errs);
    out
}

/// `buildAttachmentNomors`: `attachment_nomor` (objek tipe→nomor) dengan `attachment_tanggal`.
fn build_attachment_nomors(body: &Value) -> Option<Value> {
    let nomors = body.get("attachment_nomor")?.as_object()?;
    if nomors.is_empty() {
        return None;
    }
    let tanggals = body.get("attachment_tanggal").and_then(Value::as_object);
    let mut result = Map::new();
    for (ty, raw) in nomors {
        if label_of(ty).is_none() {
            continue;
        }
        let nomor = match raw {
            Value::String(s) => s.trim().to_string(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        if nomor.is_empty() {
            continue;
        }
        let tanggal = tanggals
            .and_then(|t| t.get(ty))
            .cloned()
            .unwrap_or(Value::Null);
        result.insert(ty.clone(), json!({ "nomor": nomor, "tanggal": tanggal }));
    }
    if result.is_empty() {
        None
    } else {
        Some(Value::Object(result))
    }
}

/// Nomor dari `attachment_nomor` untuk urutan sequence (`array_values` dan hanya string).
fn attachment_number_values(body: &Value) -> Vec<String> {
    body.get("attachment_nomor")
        .and_then(Value::as_object)
        .map(|m| {
            m.values()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Register dan sequence
// ---------------------------------------------------------------------------

/// `linkRegister`: kaitkan register dengan nomor yang sama dan belum punya addendum.
async fn link_register(
    tx: &mut Transaction<'_, MySql>,
    row: &kontrak::AddendumRow,
) -> Result<(), ApiError> {
    let Some(nomor) = row.nomor_addendum.as_deref().filter(|n| !n.is_empty()) else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE tbl_document_registers SET addendum_id = ? WHERE kontrak_id = ? AND nomor = ? AND addendum_id IS NULL",
    )
    .bind(row.id)
    .bind(row.kontrak_id)
    .bind(nomor)
    .execute(&mut **tx)
    .await
    .map_err(internal)?;
    Ok(())
}

/// Nomor urut terbesar dari sebuah nomor: `.N/YYYY` di akhir, `N.` di awal, atau `N/` di awal.
fn sequence_from_number(num: &str) -> Option<u64> {
    if let Some(slash) = num.rfind('/') {
        let (left, right) = (&num[..slash], &num[slash + 1..]);
        if !right.is_empty() && right.bytes().all(|b| b.is_ascii_digit()) {
            let digits: String = left
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if !digits.is_empty() && left[..left.len() - digits.len()].ends_with('.') {
                return digits.parse().ok();
            }
        }
    }
    let lead: String = num.chars().take_while(|c| c.is_ascii_digit()).collect();
    if lead.is_empty() {
        return None;
    }
    let rest = &num[lead.len()..];
    if let Some(after_dot) = rest.strip_prefix('.') {
        let second: String = after_dot
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !second.is_empty() {
            return second.parse().ok();
        }
    }
    if rest.starts_with('/') {
        return lead.parse().ok();
    }
    None
}

/// `updateSequenceFromNumbers`: naikkan `last_number` bila ada nomor yang lebih besar.
async fn update_sequence(
    tx: &mut Transaction<'_, MySql>,
    numbers: &[String],
    year: i32,
) -> Result<(), ApiError> {
    let max_seq = numbers
        .iter()
        .filter_map(|n| sequence_from_number(n))
        .max()
        .unwrap_or(0);
    if max_seq == 0 {
        return Ok(());
    }
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(last_number AS SIGNED) FROM tbl_document_sequences WHERE year = ? AND type = 'berita-acara' FOR UPDATE",
    )
    .bind(year)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?;
    let current = current.unwrap_or(0);
    if (max_seq as i64) > current {
        sqlx::query(
            "INSERT INTO tbl_document_sequences (year, type, last_number) VALUES (?, 'berita-acara', ?) \
             ON DUPLICATE KEY UPDATE last_number = VALUES(last_number)",
        )
        .bind(year)
        .bind(max_seq as i64)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    }
    Ok(())
}

/// Tahun dari tanggal addendum (`substr(tanggal, 0, 4)`), atau tahun sekarang bila kosong.
fn year_of(tanggal: Option<NaiveDate>) -> i32 {
    tanggal.map_or_else(|| chrono::Utc::now().year(), |d| d.year())
}

async fn insert_items(
    tx: &mut Transaction<'_, MySql>,
    addendum_id: i64,
    items: &[ItemInput],
) -> Result<(), ApiError> {
    for item in items {
        sqlx::query(
            "INSERT INTO tbl_kontrak_addendum_items (addendum_id, nama_item, spesifikasi_sebelum, spesifikasi_sesudah, \
             volume_sebelum, volume_sesudah, harga_sebelum, harga_sesudah, subtotal_sebelum, subtotal_sesudah, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
        )
        .bind(addendum_id)
        .bind(&item.nama_item)
        .bind(&item.spesifikasi_sebelum)
        .bind(&item.spesifikasi_sesudah)
        .bind(item.volume_sebelum)
        .bind(item.volume_sesudah)
        .bind(item.harga_sebelum)
        .bind(item.harga_sesudah)
        .bind(item.subtotal_sebelum)
        .bind(item.subtotal_sesudah)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handler: daftar dan detail
// ---------------------------------------------------------------------------

fn truthy_query(v: Option<&String>) -> Option<&str> {
    v.map(|s| s.as_str()).filter(|s| !s.is_empty())
}

/// `GET /api/kontrak-addendums`: admin, paginasi 20, filter status dan pencarian.
pub async fn all(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    authorize_admin(&a)?;

    let mut sql = format!("{} WHERE 1 = 1", kontrak::SELECT_ADDENDUM);
    let mut binds: Vec<String> = Vec::new();
    if let Some(status) = truthy_query(query.get("status")).filter(|s| *s != "all") {
        sql.push_str(" AND status = ?");
        binds.push(status.to_string());
    }
    if let Some(search) = truthy_query(query.get("search")) {
        let like = format!("%{search}%");
        sql.push_str(
            " AND (nomor_addendum LIKE ? OR alasan LIKE ? \
             OR kontrak_id IN (SELECT k.id FROM tbl_kontrak k WHERE k.id_pekerjaan IN (SELECT id FROM tbl_pekerjaan WHERE nama_paket LIKE ?)) \
             OR kontrak_id IN (SELECT kp.kontrak_id FROM kontrak_pekerjaan kp JOIN tbl_pekerjaan p ON p.id = kp.pekerjaan_id WHERE p.nama_paket LIKE ?) \
             OR kontrak_id IN (SELECT k.id FROM tbl_kontrak k WHERE k.id_penyedia IN (SELECT id FROM tbl_penyedia WHERE nama LIKE ?)))",
        );
        binds.extend(std::iter::repeat_n(like, 5));
    }
    let count_sql = sql.replacen(
        kontrak::SELECT_ADDENDUM,
        "SELECT COUNT(*) FROM tbl_kontrak_addendums",
        1,
    );
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b);
    }
    let total = cq.fetch_one(&state.pool).await.map_err(internal)?;

    let page = page_of(&query);
    sql.push_str(" ORDER BY tanggal_addendum DESC, created_at DESC, id DESC LIMIT ? OFFSET ?");
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q
        .bind(PER_PAGE as i64)
        .bind(((page - 1) * PER_PAGE) as i64)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let mut data = Vec::with_capacity(rows.len());
    for r in &rows {
        let row = kontrak::map_addendum(r).map_err(internal)?;
        data.push(
            resource(
                &state,
                &row,
                Include {
                    items: true,
                    people: true,
                    kontrak: true,
                },
            )
            .await?,
        );
    }
    let base = format!(
        "{}/api/kontrak-addendums",
        state.app_url.trim_end_matches('/')
    );
    Ok(Json(pagination::paginate_with_query(
        data,
        total as u64,
        pagination::PageParams {
            page,
            per_page: PER_PAGE,
        },
        &base,
        "",
    ))
    .into_response())
}

fn page_of(query: &HashMap<String, String>) -> u64 {
    query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1)
}

/// `GET /api/kontrak-addendums/{id}`: detail dengan kontrak, item, dan pembuat/penyetuju.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_addendum(&state.pool, id).await?;
    let k = find_kontrak(&state.pool, row.kontrak_id).await?;
    authorize_view_kontrak(&state, &a, &k).await?;
    Ok(Json(json!({ "data": resource(&state, &row, Include { items: true, people: true, kontrak: true }).await? })).into_response())
}

/// `GET /api/kontrak/{id}/addendums`: semua addendum kontrak, urut `addendum_ke`.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let k = find_kontrak(&state.pool, id).await?;
    authorize_view_kontrak(&state, &a, &k).await?;
    let sql = format!(
        "{} WHERE kontrak_id = ? ORDER BY addendum_ke, id",
        kontrak::SELECT_ADDENDUM
    );
    let rows = sqlx::query(&sql)
        .bind(id)
        .fetch_all(&state.pool)
        .await
        .map_err(internal)?;
    let mut data = Vec::with_capacity(rows.len());
    for r in &rows {
        let row = kontrak::map_addendum(r).map_err(internal)?;
        data.push(
            resource(
                &state,
                &row,
                Include {
                    items: true,
                    people: true,
                    kontrak: false,
                },
            )
            .await?,
        );
    }
    Ok(Json(json!({ "data": data })).into_response())
}

// ---------------------------------------------------------------------------
// Handler: nomor
// ---------------------------------------------------------------------------

/// Pengganti `str_replace` berurutan (PHP) dengan daftar pasangan.
fn replace_all(template: &str, pairs: &[(&str, String)]) -> String {
    pairs.iter().fold(template.to_string(), |acc, (from, to)| {
        acc.replace(from, to)
    })
}

/// Nomor SPK untuk prefix lampiran: `DISPERKIM-AMS.N` (tanpa peka huruf) atau `.N.N/N` di akhir.
fn spk_prefix(spk: &str, fallback: String) -> String {
    let lower = spk.to_lowercase();
    let needle = "disperkim-ams.";
    let mut from = 0;
    while let Some(pos) = lower[from..].find(needle) {
        let start = from + pos + needle.len();
        let digits: String = spk[start..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            return digits;
        }
        from = start;
    }
    if let Some(slash) = spk.rfind('/') {
        let right = &spk[slash + 1..];
        let left = &spk[..slash];
        if !right.is_empty() && right.bytes().all(|b| b.is_ascii_digit()) {
            if let Some(dot) = left.rfind('.') {
                let mid = &left[dot + 1..];
                if !mid.is_empty() && mid.bytes().all(|b| b.is_ascii_digit()) {
                    if let Some(dot2) = left[..dot].rfind('.') {
                        let first = &left[dot2 + 1..dot];
                        if !first.is_empty() && first.bytes().all(|b| b.is_ascii_digit()) {
                            return first.to_string();
                        }
                    }
                }
            }
        }
    }
    fallback
}

/// `POST /api/kontrak/{id}/addendum-numbers`: pratinjau nomor addendum dan lampiran.
pub async fn generate_numbers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let k = find_kontrak(&state.pool, id).await?;
    authorize_create(&state, &a, &k).await?;

    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let tanggal = body.get("tanggal").and_then(text_value);
    let tanggal_date = match tanggal.as_deref() {
        None => {
            foto::add(
                &mut errs,
                "tanggal",
                "The tanggal field is required.".into(),
            );
            None
        }
        Some(s) => parse_date_str(s).or_else(|| {
            foto::add(
                &mut errs,
                "tanggal",
                "The tanggal field must be a valid date.".into(),
            );
            None
        }),
    };
    let count = match body.get("count").and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    }) {
        None => {
            foto::add(&mut errs, "count", "The count field is required.".into());
            None
        }
        Some(n) if (1..=20).contains(&n) => Some(n),
        Some(_) => {
            foto::add(
                &mut errs,
                "count",
                "The count field must be between 1 and 20.".into(),
            );
            None
        }
    };
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    let (Some(date), Some(count)) = (tanggal_date, count) else {
        return Err(internal("validasi generate nomor tidak lengkap"));
    };

    let year = date.year();
    let addendum_type: Option<(String, Option<String>)> = sqlx::query(
        "SELECT code, format_template FROM tbl_document_types WHERE LOWER(code) IN ('add', 'addendum') ORDER BY id LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?
    .map(|r| -> Result<_, ApiError> { Ok((r.try_get(0).map_err(internal)?, r.try_get(1).map_err(internal)?)) })
    .transpose()?;
    let Some((code, format_template)) = addendum_type else {
        return Err(unprocessable(
            "Tipe dokumen Addendum (ADD) belum dikonfigurasi di Master Tipe Dokumen.",
        ));
    };

    let last: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(last_number AS SIGNED) FROM tbl_document_sequences WHERE year = ? AND type = 'berita-acara'",
    )
    .bind(year)
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;
    let start = last.unwrap_or(0) + 1;

    const ROMAN: [&str; 12] = [
        "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII",
    ];
    let month_roman = ROMAN
        .get(date.month() as usize - 1)
        .map_or_else(|| date.month().to_string(), |s| s.to_string());

    let id_pekerjaan = k.id_pekerjaan.map_or(String::new(), |v| v.to_string());
    let prefix = match k.spk.as_deref() {
        Some(spk) => spk_prefix(spk, id_pekerjaan.clone()),
        None => id_pekerjaan.clone(),
    };

    let template = format_template
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "{sequence}/{code}-AMIS/{month}/{year}".to_string());
    let year_s = date.format("%Y").to_string();
    let pairs: Vec<(&str, String)> = vec![
        ("{sequence}", format!("{start:03}")),
        ("{nomor_urut_surat}", start.to_string()),
        ("{code}", code),
        ("{year}", year_s.clone()),
        ("{tahun}", year_s.clone()),
        ("{month}", month_roman),
        ("{day}", date.format("%d").to_string()),
        ("{kontrak_id}", k.id.to_string()),
        ("{id_pekerjaan}", id_pekerjaan.clone()),
    ];
    let mut numbers = vec![replace_all(&template, &pairs)];
    for i in 1..count {
        let seq = format!("{:03}", start + i);
        numbers.push(format!("{id_pekerjaan}.{seq}/{prefix}/AMS/{year_s}"));
    }
    Ok(Json(json!({ "numbers": numbers })).into_response())
}

// ---------------------------------------------------------------------------
// Handler: tulis addendum
// ---------------------------------------------------------------------------

/// `POST /api/kontrak/{id}/addendums`: buat draft beserta item.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let k = find_kontrak(&state.pool, id).await?;
    authorize_create(&state, &a, &k).await?;
    let input = validate_addendum(&state.pool, &body, id, None).await?;

    let url = format!(
        "{}/api/kontrak-addendums",
        state.app_url.trim_end_matches('/')
    );
    let attachment = build_attachment_nomors(&body).map(|v| v.to_string());
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let res = sqlx::query(
        "INSERT INTO tbl_kontrak_addendums (kontrak_id, addendum_ke, nomor_addendum, attachment_nomors, tanggal_addendum, \
         jenis_addendum, alasan, deskripsi_perubahan, nilai_kontrak_sebelum, nilai_kontrak_sesudah, tgl_selesai_sebelum, \
         tgl_selesai_sesudah, status, kelengkapan_override, created_by, approved_by, approved_at, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'draft', 0, ?, NULL, NULL, NOW(), NOW())",
    )
    .bind(id)
    .bind(input.addendum_ke)
    .bind(input.nomor_addendum.clone().flatten())
    .bind(attachment)
    .bind(input.tanggal_addendum)
    .bind(&input.jenis_addendum)
    .bind(input.alasan.clone().flatten())
    .bind(input.deskripsi_perubahan.clone().flatten())
    .bind(input.nilai_kontrak_sebelum.flatten())
    .bind(input.nilai_kontrak_sesudah.flatten())
    .bind(input.tgl_selesai_sebelum.flatten())
    .bind(input.tgl_selesai_sesudah.flatten())
    .bind(a.user_id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let new_id = res.last_insert_id() as i64;
    let row = kontrak::map_addendum(
        &sqlx::query(&format!("{} WHERE id = ?", kontrak::SELECT_ADDENDUM))
            .bind(new_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?,
    )
    .map_err(internal)?;
    link_register(&mut tx, &row).await?;
    insert_items(&mut tx, new_id, input.items.as_deref().unwrap_or(&[])).await?;

    let mut numbers: Vec<String> = row.nomor_addendum.iter().cloned().collect();
    numbers.extend(attachment_number_values(&body));
    update_sequence(&mut tx, &numbers, year_of(Some(input.tanggal_addendum))).await?;

    audit(
        &mut tx,
        &headers,
        a.user_id,
        "created",
        new_id,
        None,
        Some(attributes(&row)),
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;

    let row = find_addendum(&state.pool, new_id).await?;
    let out = resource(
        &state,
        &row,
        Include {
            items: true,
            ..Default::default()
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!({ "data": out }))).into_response())
}

/// `PUT` dan `PATCH /api/kontrak-addendums/{id}`. Field yang tidak dikirim tidak diubah.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = find_addendum(&state.pool, id).await?;
    let k = find_kontrak(&state.pool, current.kontrak_id).await?;
    authorize_create(&state, &a, &k).await?;
    ensure_editable(&current)?;
    let input = validate_addendum(&state.pool, &body, current.kontrak_id, Some(&current)).await?;

    let url = format!(
        "{}/api/kontrak-addendums/{id}",
        state.app_url.trim_end_matches('/')
    );
    let before = attributes(&current);
    let attachment = build_attachment_nomors(&body).map(|v| v.to_string());

    let mut tx = state.pool.begin().await.map_err(internal)?;
    // Kolom yang dikirim saja; `attachment_nomors` selalu ditulis (null bila tidak ada).
    let mut sets: Vec<&str> = vec![
        "addendum_ke = ?",
        "tanggal_addendum = ?",
        "jenis_addendum = ?",
        "attachment_nomors = ?",
    ];
    if input.nomor_addendum.is_some() {
        sets.push("nomor_addendum = ?");
    }
    if input.alasan.is_some() {
        sets.push("alasan = ?");
    }
    if input.deskripsi_perubahan.is_some() {
        sets.push("deskripsi_perubahan = ?");
    }
    if input.nilai_kontrak_sebelum.is_some() {
        sets.push("nilai_kontrak_sebelum = ?");
    }
    if input.nilai_kontrak_sesudah.is_some() {
        sets.push("nilai_kontrak_sesudah = ?");
    }
    if input.tgl_selesai_sebelum.is_some() {
        sets.push("tgl_selesai_sebelum = ?");
    }
    if input.tgl_selesai_sesudah.is_some() {
        sets.push("tgl_selesai_sesudah = ?");
    }
    let sql = format!(
        "UPDATE tbl_kontrak_addendums SET {}, updated_at = NOW() WHERE id = ?",
        sets.join(", ")
    );
    let mut q = sqlx::query(&sql)
        .bind(input.addendum_ke)
        .bind(input.tanggal_addendum)
        .bind(&input.jenis_addendum)
        .bind(attachment);
    if let Some(v) = &input.nomor_addendum {
        q = q.bind(v.clone());
    }
    if let Some(v) = &input.alasan {
        q = q.bind(v.clone());
    }
    if let Some(v) = &input.deskripsi_perubahan {
        q = q.bind(v.clone());
    }
    if let Some(v) = input.nilai_kontrak_sebelum {
        q = q.bind(v);
    }
    if let Some(v) = input.nilai_kontrak_sesudah {
        q = q.bind(v);
    }
    if let Some(v) = input.tgl_selesai_sebelum {
        q = q.bind(v);
    }
    if let Some(v) = input.tgl_selesai_sesudah {
        q = q.bind(v);
    }
    q.bind(id).execute(&mut *tx).await.map_err(internal)?;

    let after = find_addendum_tx(&mut tx, id).await?;
    link_register(&mut tx, &after).await?;
    if let Some(items) = &input.items {
        sqlx::query("DELETE FROM tbl_kontrak_addendum_items WHERE addendum_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        insert_items(&mut tx, id, items).await?;
    }

    let mut numbers: Vec<String> = after.nomor_addendum.iter().cloned().collect();
    numbers.extend(attachment_number_values(&body));
    update_sequence(
        &mut tx,
        &numbers,
        year_of(Some(
            after.tanggal_addendum.unwrap_or(input.tanggal_addendum),
        )),
    )
    .await?;

    if let Some((old, new)) = dirty(&before, &attributes(&after)) {
        audit(
            &mut tx,
            &headers,
            a.user_id,
            "updated",
            id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;

    let row = find_addendum(&state.pool, id).await?;
    Ok(Json(json!({ "data": resource(&state, &row, Include { items: true, ..Default::default() }).await? })).into_response())
}

async fn find_addendum_tx(
    tx: &mut Transaction<'_, MySql>,
    id: i64,
) -> Result<kontrak::AddendumRow, ApiError> {
    let sql = format!("{} WHERE id = ?", kontrak::SELECT_ADDENDUM);
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    kontrak::map_addendum(&row).map_err(internal)
}

/// `DELETE /api/kontrak-addendums/{id}`: hapus addendum beserta item dan lampirannya.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_addendum(&state.pool, id).await?;
    let k = find_kontrak(&state.pool, row.kontrak_id).await?;
    authorize_submit(&state, &a, &k).await?;
    ensure_editable(&row)?;

    let url = format!(
        "{}/api/kontrak-addendums/{id}",
        state.app_url.trim_end_matches('/')
    );
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM tbl_kontrak_addendum_items WHERE addendum_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let dirs = media::delete_collection(&mut tx, MODEL, id as u64, COLLECTION, None).await?;
    sqlx::query("DELETE FROM tbl_kontrak_addendums WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    audit(
        &mut tx,
        &headers,
        a.user_id,
        "deleted",
        id,
        Some(attributes(&row)),
        None,
        &url,
    )
    .await?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&dirs).await;
    Ok(Json(json!({ "message": "Addendum kontrak berhasil dihapus" })).into_response())
}

/// Ubah status dan catat audit `updated`. Dipakai aksi alur persetujuan.
async fn set_status(
    state: &AppState,
    headers: &HeaderMap,
    actor_id: u64,
    row: &kontrak::AddendumRow,
    sql: &str,
    binds: Vec<Option<String>>,
    approve_stamp: bool,
) -> Result<kontrak::AddendumRow, ApiError> {
    let url = format!(
        "{}/api/kontrak-addendums/{}",
        state.app_url.trim_end_matches('/'),
        row.id
    );
    let before = attributes(row);
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(b);
    }
    if approve_stamp {
        q = q.bind(actor_id);
    }
    q.bind(row.id).execute(&mut *tx).await.map_err(internal)?;
    let after = find_addendum_tx(&mut tx, row.id).await?;
    if let Some((old, new)) = dirty(&before, &attributes(&after)) {
        audit(
            &mut tx,
            headers,
            actor_id,
            "updated",
            row.id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(after)
}

/// `POST /api/kontrak-addendums/{id}/submit`: draft atau ditolak menjadi diajukan.
pub async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_addendum(&state.pool, id).await?;
    let k = find_kontrak(&state.pool, row.kontrak_id).await?;
    authorize_submit(&state, &a, &k).await?;
    if row.status != "draft" && row.status != "ditolak" {
        return Err(unprocessable(
            "Addendum hanya bisa diajukan dari status draft atau ditolak",
        ));
    }
    let after = set_status(
        &state,
        &headers,
        a.user_id,
        &row,
        "UPDATE tbl_kontrak_addendums SET status = 'diajukan', approved_by = NULL, approved_at = NULL, updated_at = NOW() WHERE id = ?",
        vec![],
        false,
    )
    .await?;
    Ok(Json(json!({ "data": resource(&state, &after, Include { items: true, ..Default::default() }).await? })).into_response())
}

/// Nomor dan tanggal dokumen untuk register (`dokumen`).
#[derive(Debug)]
struct Dokumen {
    ty: String,
    nomor: Option<String>,
    tanggal: Option<String>,
}

/// `dokumenFromAttachments`: nomor dan tanggal dari lampiran yang sudah diupload.
async fn dokumen_from_attachments(
    pool: &MySqlPool,
    addendum_id: i64,
) -> Result<Vec<Dokumen>, ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(custom_properties AS CHAR) FROM media WHERE model_type = ? AND model_id = ? AND collection_name = ? ORDER BY order_column, id",
    )
    .bind(MODEL)
    .bind(addendum_id as u64)
    .bind(COLLECTION)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    let mut out = Vec::new();
    for r in &rows {
        let props: Value = r
            .try_get::<Option<String>, _>(0)
            .map_err(internal)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let Some(ty) = props.get("type").and_then(text_value) else {
            continue;
        };
        out.push(Dokumen {
            ty,
            nomor: props.get("nomor").and_then(text_value),
            tanggal: props.get("tanggal").and_then(text_value),
        });
    }
    Ok(out)
}

/// `POST /api/kontrak-addendums/{id}/process`: admin menetapkan nomor dan mencatat dokumen ke register.
pub async fn process(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    authorize_admin(&a)?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_addendum(&state.pool, id).await?;
    if row.status != "diajukan" {
        return Err(unprocessable(
            "Hanya addendum yang sudah diajukan yang bisa diproses",
        ));
    }

    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let nomor_addendum = match body.get("nomor_addendum").and_then(text_value) {
        Some(n) if n.chars().count() <= 100 => Some(n),
        Some(_) => {
            foto::add(
                &mut errs,
                "nomor_addendum",
                "The nomor addendum field must not be greater than 100 characters.".into(),
            );
            None
        }
        None => {
            foto::add(
                &mut errs,
                "nomor_addendum",
                "The nomor addendum field is required.".into(),
            );
            None
        }
    };

    // `dokumen` nullable array: bila tidak dikirim, pakai lampiran yang diupload.
    let mut dokumen: Vec<Dokumen> = Vec::new();
    let from_body = match body.get("dokumen") {
        None | Some(Value::Null) => None,
        Some(Value::Array(list)) => {
            for (i, d) in list.iter().enumerate() {
                let ty = d.get("type").and_then(text_value);
                if ty.is_none() {
                    foto::add(
                        &mut errs,
                        &format!("dokumen.{i}.type"),
                        format!("The dokumen.{i}.type field is required."),
                    );
                }
                let nomor = d.get("nomor").and_then(text_value);
                if nomor.as_deref().is_some_and(|n| n.chars().count() > 255) {
                    foto::add(
                        &mut errs,
                        &format!("dokumen.{i}.nomor"),
                        format!(
                            "The dokumen.{i}.nomor field must not be greater than 255 characters."
                        ),
                    );
                }
                let tanggal = d.get("tanggal").and_then(text_value);
                if let Some(t) = tanggal.as_deref() {
                    if parse_date_str(t).is_none() {
                        foto::add(
                            &mut errs,
                            &format!("dokumen.{i}.tanggal"),
                            format!("The dokumen.{i}.tanggal field must be a valid date."),
                        );
                    }
                }
                if let Some(ty) = ty {
                    dokumen.push(Dokumen { ty, nomor, tanggal });
                }
            }
            Some(())
        }
        Some(_) => {
            foto::add(
                &mut errs,
                "dokumen",
                "The dokumen field must be an array.".into(),
            );
            None
        }
    };
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    if from_body.is_none() {
        dokumen = dokumen_from_attachments(&state.pool, id).await?;
    }
    let Some(nomor_addendum) = nomor_addendum else {
        return Err(internal("nomor addendum tidak tersedia setelah validasi"));
    };

    let addendum_type_id: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM tbl_document_types WHERE LOWER(code) IN ('add', 'addendum') ORDER BY id LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .map_err(internal)?;

    let url = format!(
        "{}/api/kontrak-addendums/{id}",
        state.app_url.trim_end_matches('/')
    );
    let before = attributes(&row);
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query(
        "UPDATE tbl_kontrak_addendums SET nomor_addendum = ?, status = 'disetujui', approved_by = ?, approved_at = NOW(), updated_at = NOW() WHERE id = ?",
    )
    .bind(&nomor_addendum)
    .bind(a.user_id)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let after = find_addendum_tx(&mut tx, id).await?;
    link_register(&mut tx, &after).await?;

    for dok in &dokumen {
        let Some(nomor) = dok.nomor.as_deref().filter(|n| !n.is_empty() && *n != "0") else {
            continue;
        };
        sqlx::query(
            "DELETE FROM tbl_document_registers WHERE addendum_id = ? AND attachment_type = ?",
        )
        .bind(id)
        .bind(&dok.ty)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        // `tanggal` dan `type_id` wajib di tabel; nilai kosong membuat insert gagal seperti di Laravel.
        let tanggal = dok
            .tanggal
            .as_deref()
            .and_then(parse_date_str)
            .ok_or_else(|| internal("Column 'tanggal' cannot be null"))?;
        let type_id =
            addendum_type_id.ok_or_else(|| internal("Column 'type_id' cannot be null"))?;
        sqlx::query(
            "INSERT INTO tbl_document_registers (kontrak_id, addendum_id, type_id, attachment_type, nomor, tanggal, sequence_number, year, description, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?, NOW(), NOW())",
        )
        .bind(after.kontrak_id)
        .bind(id)
        .bind(type_id)
        .bind(&dok.ty)
        .bind(nomor)
        .bind(tanggal)
        .bind(tanggal.year())
        .bind(label_of(&dok.ty))
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    }

    if let Some((old, new)) = dirty(&before, &attributes(&after)) {
        audit(
            &mut tx,
            &headers,
            a.user_id,
            "updated",
            id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;

    let row = find_addendum(&state.pool, id).await?;
    Ok(Json(json!({ "data": resource(&state, &row, Include { items: true, ..Default::default() }).await? })).into_response())
}

/// `POST /api/kontrak-addendums/{id}/approve`: admin menyetujui dari draft, diajukan, atau diproses.
pub async fn approve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    authorize_admin(&a)?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_addendum(&state.pool, id).await?;
    if !["diajukan", "draft", "diproses"].contains(&row.status.as_str()) {
        return Err(unprocessable(
            "Hanya addendum yang sudah diajukan atau draft yang bisa disetujui",
        ));
    }
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let input = body.get("nomor_addendum").and_then(text_value);
    if input.as_deref().is_some_and(|n| n.chars().count() > 100) {
        foto::add(
            &mut errs,
            "nomor_addendum",
            "The nomor addendum field must not be greater than 100 characters.".into(),
        );
    }
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    // `$validated['nomor_addendum'] ?: $existing`: kosong atau "0" memakai nomor yang ada.
    let nomor = input
        .filter(|n| n != "0")
        .or_else(|| row.nomor_addendum.clone());

    let url = format!(
        "{}/api/kontrak-addendums/{id}",
        state.app_url.trim_end_matches('/')
    );
    let before = attributes(&row);
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query(
        "UPDATE tbl_kontrak_addendums SET nomor_addendum = ?, status = 'disetujui', approved_by = ?, approved_at = NOW(), updated_at = NOW() WHERE id = ?",
    )
    .bind(nomor)
    .bind(a.user_id)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let after = find_addendum_tx(&mut tx, id).await?;
    link_register(&mut tx, &after).await?;
    if let Some((old, new)) = dirty(&before, &attributes(&after)) {
        audit(
            &mut tx,
            &headers,
            a.user_id,
            "updated",
            id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;

    let row = find_addendum(&state.pool, id).await?;
    Ok(Json(json!({ "data": resource(&state, &row, Include { items: true, ..Default::default() }).await? })).into_response())
}

/// `POST /api/kontrak-addendums/{id}/override-kelengkapan`: atur `kelengkapan_override`.
pub async fn override_kelengkapan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    authorize_admin(&a)?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_addendum(&state.pool, id).await?;
    if row.status == "disetujui" {
        return Err(unprocessable(
            "Addendum yang sudah disetujui tidak bisa diubah kelengkapannya",
        ));
    }
    let value = match body.get("kelengkapan_override") {
        None | Some(Value::Null) => {
            let mut errs = BTreeMap::new();
            foto::add(
                &mut errs,
                "kelengkapan_override",
                "The kelengkapan override field is required.".into(),
            );
            return Err(ApiError::validation("The given data was invalid.", errs));
        }
        Some(v) => match v {
            Value::Bool(b) => *b,
            Value::Number(n) if n.as_i64() == Some(0) || n.as_i64() == Some(1) => {
                n.as_i64() == Some(1)
            }
            Value::String(s) if s == "0" || s == "false" => false,
            Value::String(s) if s == "1" || s == "true" => true,
            _ => {
                let mut errs = BTreeMap::new();
                foto::add(
                    &mut errs,
                    "kelengkapan_override",
                    "The kelengkapan override field must be true or false.".into(),
                );
                return Err(ApiError::validation("The given data was invalid.", errs));
            }
        },
    };

    let url = format!(
        "{}/api/kontrak-addendums/{id}",
        state.app_url.trim_end_matches('/')
    );
    let before = attributes(&row);
    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("UPDATE tbl_kontrak_addendums SET kelengkapan_override = ?, updated_at = NOW() WHERE id = ?")
        .bind(value)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let after = find_addendum_tx(&mut tx, id).await?;
    if let Some((old, new)) = dirty(&before, &attributes(&after)) {
        audit(
            &mut tx,
            &headers,
            a.user_id,
            "updated",
            id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(Json(json!({ "data": resource(&state, &after, Include { items: true, ..Default::default() }).await? })).into_response())
}

/// `POST /api/kontrak-addendums/{id}/reject`: admin menolak dari diajukan atau diproses.
pub async fn reject(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    authorize_admin(&a)?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_addendum(&state.pool, id).await?;
    if row.status != "diajukan" && row.status != "diproses" {
        return Err(unprocessable(
            "Hanya addendum yang sudah diajukan yang bisa ditolak",
        ));
    }
    let after = set_status(
        &state,
        &headers,
        a.user_id,
        &row,
        "UPDATE tbl_kontrak_addendums SET status = 'ditolak', approved_by = ?, approved_at = NOW(), updated_at = NOW() WHERE id = ?",
        vec![],
        true,
    )
    .await?;
    Ok(Json(json!({ "data": resource(&state, &after, Include { items: true, ..Default::default() }).await? })).into_response())
}

/// Ganti `custom_properties` sebuah media dengan `props` (gabungan kunci).
async fn merge_media_props(
    tx: &mut Transaction<'_, MySql>,
    media_id: i64,
    props: Map<String, Value>,
) -> Result<(), ApiError> {
    let current: Option<String> =
        sqlx::query_scalar("SELECT CAST(custom_properties AS CHAR) FROM media WHERE id = ?")
            .bind(media_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(internal)?
            .flatten();
    let mut merged: Map<String, Value> = current
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    for (k, v) in props {
        merged.insert(k, v);
    }
    sqlx::query("UPDATE media SET custom_properties = ?, updated_at = NOW() WHERE id = ?")
        .bind(Value::Object(merged).to_string())
        .bind(media_id)
        .execute(&mut **tx)
        .await
        .map_err(internal)?;
    Ok(())
}

/// Media lampiran (`kontrak/addendum`) dengan custom property `type` tertentu.
async fn media_of_type(
    tx: &mut Transaction<'_, MySql>,
    addendum_id: i64,
    ty: &str,
) -> Result<Vec<i64>, ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(id AS SIGNED), CAST(custom_properties AS CHAR) FROM media WHERE model_type = ? AND model_id = ? AND collection_name = ? ORDER BY id",
    )
    .bind(MODEL)
    .bind(addendum_id as u64)
    .bind(COLLECTION)
    .fetch_all(&mut **tx)
    .await
    .map_err(internal)?;
    let mut ids = Vec::new();
    for r in &rows {
        let props: Value = r
            .try_get::<Option<String>, _>(1)
            .map_err(internal)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        if props.get("type").and_then(Value::as_str) == Some(ty) {
            ids.push(r.try_get(0).map_err(internal)?);
        }
    }
    Ok(ids)
}

/// `PUT /api/kontrak-addendums/{id}/attachment-numbers`: admin mengubah nomor dan tanggal lampiran.
pub async fn update_attachment_numbers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    authorize_admin(&a)?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_addendum(&state.pool, id).await?;
    ensure_editable(&row)?;

    let numbers = match body.get("numbers") {
        Some(Value::Object(m)) => m.clone(),
        _ => {
            let mut errs = BTreeMap::new();
            foto::add(
                &mut errs,
                "numbers",
                "The numbers field is required.".into(),
            );
            return Err(ApiError::validation("The given data was invalid.", errs));
        }
    };

    let url = format!(
        "{}/api/kontrak-addendums/{id}/attachment-numbers",
        state.app_url.trim_end_matches('/')
    );
    let before = attributes(&row);
    let mut existing: Map<String, Value> = row
        .attachment_nomors
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    let mut tx = state.pool.begin().await.map_err(internal)?;
    for (ty, entry) in &numbers {
        if label_of(ty).is_none() {
            continue;
        }
        let nomor = entry.get("nomor").and_then(text_value).unwrap_or_default();
        let tanggal = entry.get("tanggal").and_then(text_value);

        // Sinkron ke media yang sudah diupload untuk tipe ini.
        for media_id in media_of_type(&mut tx, id, ty).await? {
            let mut props = Map::new();
            if !nomor.is_empty() {
                props.insert("nomor".into(), json!(nomor));
            }
            if let Some(t) = &tanggal {
                props.insert("tanggal".into(), json!(t));
            }
            merge_media_props(&mut tx, media_id, props).await?;
        }

        if nomor.is_empty() && tanggal.is_none() {
            existing.remove(ty);
            continue;
        }
        let prev = existing.get(ty).cloned().unwrap_or(Value::Null);
        let mut next = Map::new();
        let nomor_value = if nomor.is_empty() {
            prev.get("nomor").cloned().unwrap_or(Value::Null)
        } else {
            json!(nomor)
        };
        let tanggal_value = match &tanggal {
            Some(t) => json!(t),
            None => prev.get("tanggal").cloned().unwrap_or(Value::Null),
        };
        if !nomor_value.is_null() {
            next.insert("nomor".into(), nomor_value);
        }
        if !tanggal_value.is_null() {
            next.insert("tanggal".into(), tanggal_value);
        }
        existing.insert(ty.clone(), Value::Object(next));
    }

    let stored = if existing.is_empty() {
        None
    } else {
        Some(Value::Object(existing).to_string())
    };
    sqlx::query(
        "UPDATE tbl_kontrak_addendums SET attachment_nomors = ?, updated_at = NOW() WHERE id = ?",
    )
    .bind(stored)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?;
    let after = find_addendum_tx(&mut tx, id).await?;
    if let Some((old, new)) = dirty(&before, &attributes(&after)) {
        audit(
            &mut tx,
            &headers,
            a.user_id,
            "updated",
            id,
            Some(old),
            Some(new),
            &url,
        )
        .await?;
    }
    tx.commit().await.map_err(internal)?;

    let row = find_addendum(&state.pool, id).await?;
    Ok(Json(json!({ "data": resource(&state, &row, Include { items: true, ..Default::default() }).await? })).into_response())
}

/// `POST /api/kontrak-addendums/{id}/upload`: unggah lampiran untuk satu tipe (mengganti yang lama).
pub async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    multipart: Multipart,
) -> Result<Response, ApiError> {
    let a = actor(&state, &headers).await?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found())?;
    let row = find_addendum(&state.pool, id).await?;
    let k = find_kontrak(&state.pool, row.kontrak_id).await?;
    authorize_submit(&state, &a, &k).await?;

    let form = foto::read_form(multipart).await?;
    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let ty = form
        .fields
        .get("type")
        .cloned()
        .flatten()
        .filter(|s| !s.is_empty());
    if ty.is_none() {
        foto::add(&mut errs, "type", "The type field is required.".into());
    }
    let file = form.file.as_ref();
    match file {
        None => foto::add(&mut errs, "file", "The file field is required.".into()),
        Some(f) => {
            let ext = f
                .original_name
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_lowercase();
            if !UPLOAD_EXT.contains(&ext.as_str()) {
                foto::add(
                    &mut errs,
                    "file",
                    format!(
                        "The file field must be a file of type: {}.",
                        UPLOAD_EXT.join(", ")
                    ),
                );
            }
            if f.bytes.len() > UPLOAD_MAX_KB * 1024 {
                foto::add(
                    &mut errs,
                    "file",
                    format!("The file field must not be greater than {UPLOAD_MAX_KB} kilobytes."),
                );
            }
        }
    }
    let nomor_in = form
        .fields
        .get("nomor")
        .cloned()
        .flatten()
        .filter(|s| !s.trim().is_empty());
    if nomor_in.as_deref().is_some_and(|s| s.chars().count() > 255) {
        foto::add(
            &mut errs,
            "nomor",
            "The nomor field must not be greater than 255 characters.".into(),
        );
    }
    let tanggal_in = form
        .fields
        .get("tanggal")
        .cloned()
        .flatten()
        .filter(|s| !s.trim().is_empty());
    if let Some(t) = tanggal_in.as_deref() {
        if parse_date_str(t).is_none() {
            foto::add(
                &mut errs,
                "tanggal",
                "The tanggal field must be a valid date.".into(),
            );
        }
    }
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    let (Some(ty), Some(file)) = (ty, file) else {
        return Err(internal("validasi upload tidak lengkap"));
    };
    let Some(label) = label_of(&ty) else {
        return Err(unprocessable("Jenis lampiran tidak valid"));
    };

    // `?:` PHP: nomor/tanggal kosong memakai nilai lama dari attachment_nomors.
    let attachments = row
        .attachment_nomors
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or(Value::Null);
    let prev = attachments.get(&ty).cloned().unwrap_or(Value::Null);
    let nomor = nomor_in.or_else(|| prev.get("nomor").and_then(text_value));
    let tanggal = tanggal_in.or_else(|| prev.get("tanggal").and_then(text_value));

    let mime = media::mime_for_name(&file.original_name);
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let mut old_dirs = Vec::new();
    for media_id in media_of_type(&mut tx, id, &ty).await? {
        sqlx::query("DELETE FROM media WHERE id = ?")
            .bind(media_id)
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        old_dirs.push(media::media_dir(media_id as u64));
    }
    // Upload media tidak mengubah atribut model, jadi tidak ada audit (sama dengan Laravel).
    let stored = media::attach(&mut tx, MODEL, id as u64, COLLECTION, file, mime, false).await?;
    let props = Map::from_iter([
        ("type".to_string(), json!(ty)),
        ("label".to_string(), json!(label)),
        ("nomor".to_string(), json!(nomor)),
        ("tanggal".to_string(), json!(tanggal)),
    ]);
    if let Err(e) = merge_media_props(&mut tx, stored.media_id as i64, props).await {
        media::remove_dirs(std::slice::from_ref(&stored.dir)).await;
        return Err(e);
    }
    if let Err(e) = tx.commit().await {
        media::remove_dirs(std::slice::from_ref(&stored.dir)).await;
        return Err(internal(e));
    }
    media::remove_dirs(&old_dirs).await;

    let row = find_addendum(&state.pool, id).await?;
    Ok(Json(json!({ "data": resource(&state, &row, Include { items: true, ..Default::default() }).await? })).into_response())
}
