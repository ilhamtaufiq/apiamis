//! `GET` dan `PUT /api/pekerjaan/{id}/progress-estimasi`, setara `PekerjaanProgressEstimasiController`.
//!
//! Riwayat rencana dan realisasi fisik serta keuangan per tahun anggaran. Ringkasan (`latest_*`, `deviasi`)
//! dihitung di sini dari riwayat, bukan dari `progress_estimasi.rs`, karena respon ini memuat seluruh riwayat.
//!
//! Berbeda dari Laravel:
//! - Pekerjaan di luar scope `byUserRole()` mendapat 404 (seperti `findOrFail`).
//! - Snapshot dan sinkronisasi Puspen sudah dihapus dari sistem dan tidak dipindah.
//! - Validasi `date` hanya menerima awalan `YYYY-MM-DD` (format lain ditolak).

use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row};

use crate::{
    access, desa::internal, format::number_like_php, lookup::carbon_json, pekerjaan,
    progress_metrics::round2, require_auth, AppState,
};

type Errors = BTreeMap<String, Vec<String>>;

const TAHUN_MIN: i64 = 2000;
const TAHUN_MAX: i64 = 2100;

/// Satu baris riwayat.
#[derive(Debug, Clone)]
struct Entry {
    id: u64,
    tanggal: NaiveDate,
    persen: f64,
    nilai: Option<f64>,
    nomor_sp2d: Option<String>,
    tanggal_pembuatan: Option<NaiveDate>,
    tanggal_pencairan: Option<NaiveDate>,
}

/// Input `PUT` yang sudah lolos validasi, per (jenis, tipe).
struct Input {
    tahun: i64,
    sections: Vec<(&'static str, &'static str, Vec<Entry>)>,
}

fn push(errors: &mut Errors, key: &str, msg: String) {
    errors.entry(key.to_string()).or_default().push(msg);
}

/// `integer` Laravel: angka JSON atau string angka bulat.
fn int_value(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `numeric` Laravel: angka JSON atau string angka.
fn num_value(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}

/// `date` Laravel, dibatasi ke awalan `YYYY-MM-DD`.
fn date_value(v: &Value) -> Option<NaiveDate> {
    let s = v.as_str()?;
    if s.len() < 10 {
        return None;
    }
    NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d").ok()
}

/// Aturan persen (closure di Laravel): 0 sampai 100, maksimal dua angka di belakang koma.
fn percent_value(key: &str, v: Option<&Value>, errors: &mut Errors) -> Option<f64> {
    let raw = match v {
        None => {
            push(errors, key, format!("The {key} field is required."));
            return None;
        }
        Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        // Array atau objek tidak pernah lolos format angka.
        Some(_) => "?".to_string(),
    };
    if raw.is_empty() {
        push(errors, key, format!("The {key} field is required."));
        push(errors, key, format!("Field {key} wajib diisi."));
        return None;
    }
    if !is_percent_format(&raw) {
        push(
            errors,
            key,
            format!(
                "Field {key} harus berupa angka desimal dengan maksimal 2 angka di belakang koma."
            ),
        );
        return None;
    }
    let normalized = raw.replace(',', ".");
    let value: f64 = normalized.parse().unwrap_or(-1.0);
    if !(0.0..=100.0).contains(&value) {
        push(
            errors,
            key,
            format!("Field {key} harus berada antara 0 dan 100."),
        );
        return None;
    }
    Some(round2(value))
}

/// `^\d+([.,]\d{1,2})?$`.
fn is_percent_format(s: &str) -> bool {
    let (int, frac) = match s.find(['.', ',']) {
        Some(pos) => (&s[..pos], Some(&s[pos + 1..])),
        None => (s, None),
    };
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    if !digits(int) {
        return false;
    }
    match frac {
        None => true,
        Some(f) => digits(f) && f.len() <= 2,
    }
}

/// Validasi satu bagian (`fisik` atau `keuangan`) beserta tipe `rencana` dan `realisasi`.
fn parse_section(
    body: &Map<String, Value>,
    jenis: &'static str,
    errors: &mut Errors,
    out: &mut Vec<(&'static str, &'static str, Vec<Entry>)>,
) {
    let section = match body.get(jenis) {
        None | Some(Value::Null) => None,
        Some(Value::Object(m)) => Some(m),
        Some(_) => {
            push(
                errors,
                jenis,
                format!("The {jenis} field must be an array."),
            );
            None
        }
    };
    for tipe in ["rencana", "realisasi"] {
        let key = format!("{jenis}.{tipe}");
        let items = section.and_then(|m| m.get(tipe));
        let list: Vec<Value> = match items {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) => a.clone(),
            Some(_) => {
                push(errors, &key, format!("The {key} field must be an array."));
                Vec::new()
            }
        };
        let mut entries = Vec::new();
        for (i, item) in list.iter().enumerate() {
            let base = format!("{key}.{i}");
            let fields = item.as_object();
            let get = |k: &str| fields.and_then(|f| f.get(k));
            let tanggal_key = format!("{base}.tanggal");
            let tanggal = match get("tanggal") {
                None | Some(Value::Null) => {
                    push(
                        errors,
                        &tanggal_key,
                        format!("The {} field is required.", tanggal_key),
                    );
                    None
                }
                Some(v) => match date_value(v) {
                    Some(d) => Some(d),
                    None => {
                        push(
                            errors,
                            &tanggal_key,
                            format!("The {} field must be a valid date.", tanggal_key),
                        );
                        None
                    }
                },
            };
            let persen_key = format!("{base}.persen");
            let persen = percent_value(&persen_key, get("persen"), errors);

            let mut entry = Entry {
                id: 0,
                tanggal: NaiveDate::default(),
                persen: 0.0,
                nilai: None,
                nomor_sp2d: None,
                tanggal_pembuatan: None,
                tanggal_pencairan: None,
            };
            if jenis == "keuangan" && tipe == "realisasi" {
                let nilai_key = format!("{base}.nilai");
                match get("nilai") {
                    None | Some(Value::Null) => {}
                    Some(Value::String(s)) if s.trim().is_empty() => {}
                    Some(v) => match num_value(v) {
                        Some(n) => entry.nilai = Some(n),
                        None => push(
                            errors,
                            &nilai_key,
                            format!("The {} field must be a number.", nilai_key),
                        ),
                    },
                }
                let sp2d_key = format!("{base}.nomor_sp2d");
                match get("nomor_sp2d") {
                    None | Some(Value::Null) => {}
                    Some(Value::String(s)) if s.trim().is_empty() => {}
                    Some(Value::String(s)) if s.chars().count() <= 255 => {
                        entry.nomor_sp2d = Some(s.clone())
                    }
                    Some(Value::String(_)) => push(
                        errors,
                        &sp2d_key,
                        format!(
                            "The {} field must not be greater than 255 characters.",
                            sp2d_key
                        ),
                    ),
                    Some(_) => push(
                        errors,
                        &sp2d_key,
                        format!("The {} field must be a string.", sp2d_key),
                    ),
                }
                for (k, slot) in [
                    ("tanggal_pembuatan", &mut entry.tanggal_pembuatan),
                    ("tanggal_pencairan", &mut entry.tanggal_pencairan),
                ] {
                    let field_key = format!("{base}.{k}");
                    match get(k) {
                        None | Some(Value::Null) => {}
                        Some(Value::String(s)) if s.trim().is_empty() => {}
                        Some(v) => match date_value(v) {
                            Some(d) => *slot = Some(d),
                            None => push(
                                errors,
                                &field_key,
                                format!("The {} field must be a valid date.", field_key),
                            ),
                        },
                    }
                }
            }
            if let (Some(t), Some(p)) = (tanggal, persen) {
                entry.tanggal = t;
                entry.persen = p;
                entries.push(entry);
            }
        }
        out.push((
            jenis,
            if tipe == "rencana" {
                "rencana"
            } else {
                "realisasi"
            },
            entries,
        ));
    }
}

/// Validasi body `PUT`, termasuk `tahun` wajib dan rentang 2000 sampai 2100.
fn parse_update(body: &Value) -> Result<Input, Errors> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut errors = Errors::new();
    let tahun = match obj.get("tahun") {
        None | Some(Value::Null) => {
            push(&mut errors, "tahun", "The tahun field is required.".into());
            0
        }
        Some(v) => match int_value(v) {
            None => {
                push(
                    &mut errors,
                    "tahun",
                    "The tahun field must be an integer.".into(),
                );
                0
            }
            Some(t) if t < TAHUN_MIN => {
                push(
                    &mut errors,
                    "tahun",
                    format!("The tahun field must be at least {TAHUN_MIN}."),
                );
                0
            }
            Some(t) if t > TAHUN_MAX => {
                push(
                    &mut errors,
                    "tahun",
                    format!("The tahun field must not be greater than {TAHUN_MAX}."),
                );
                0
            }
            Some(t) => t,
        },
    };
    let mut sections = Vec::new();
    parse_section(&obj, "fisik", &mut errors, &mut sections);
    parse_section(&obj, "keuangan", &mut errors, &mut sections);
    if errors.is_empty() {
        Ok(Input { tahun, sections })
    } else {
        Err(errors)
    }
}

/// `tahun` pada `GET`: nullable|integer|min:2000|max:2100. Kosong dianggap tidak ada.
fn query_tahun(raw: Option<&String>) -> Result<i64, ApiError> {
    let Some(raw) = raw.map(|s| s.trim()).filter(|s| !s.is_empty()) else {
        return Ok(chrono::Datelike::year(&Utc::now()) as i64);
    };
    let mut errors = Errors::new();
    let value = match raw.parse::<i64>() {
        Err(_) => {
            push(
                &mut errors,
                "tahun",
                "The tahun field must be an integer.".into(),
            );
            0
        }
        Ok(t) if t < TAHUN_MIN => {
            push(
                &mut errors,
                "tahun",
                format!("The tahun field must be at least {TAHUN_MIN}."),
            );
            0
        }
        Ok(t) if t > TAHUN_MAX => {
            push(
                &mut errors,
                "tahun",
                format!("The tahun field must not be greater than {TAHUN_MAX}."),
            );
            0
        }
        Ok(t) => t,
    };
    if errors.is_empty() {
        Ok(value)
    } else {
        Err(ApiError::validation("The given data was invalid.", errors))
    }
}

/// Baris riwayat dari DB untuk satu pekerjaan dan tahun, urut tanggal lalu id.
async fn load_rows(
    pool: &MySqlPool,
    pekerjaan_id: u64,
    tahun: i64,
) -> Result<(Vec<(String, String, Entry)>, Option<DateTime<Utc>>), ApiError> {
    let rows = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, jenis, tipe, tanggal, CAST(persen AS DOUBLE) AS persen, \
         CAST(nilai AS DOUBLE) AS nilai, nomor_sp2d, tanggal_pembuatan, tanggal_pencairan, updated_at \
         FROM pekerjaan_progress_estimasi_history \
         WHERE pekerjaan_id = ? AND tahun_anggaran = ? ORDER BY tanggal, id",
    )
    .bind(pekerjaan_id)
    .bind(tahun)
    .fetch_all(pool)
    .await
    .map_err(internal)?;
    let mut out = Vec::with_capacity(rows.len());
    let mut latest: Option<DateTime<Utc>> = None;
    for r in &rows {
        let updated: Option<DateTime<Utc>> = r.try_get("updated_at").map_err(internal)?;
        if let Some(u) = updated {
            latest = Some(latest.map_or(u, |cur| cur.max(u)));
        }
        let entry = Entry {
            id: r.try_get::<i64, _>("id").map_err(internal)? as u64,
            tanggal: r.try_get("tanggal").map_err(internal)?,
            persen: r.try_get("persen").map_err(internal)?,
            nilai: r.try_get("nilai").map_err(internal)?,
            nomor_sp2d: r.try_get("nomor_sp2d").map_err(internal)?,
            tanggal_pembuatan: r.try_get("tanggal_pembuatan").map_err(internal)?,
            tanggal_pencairan: r.try_get("tanggal_pencairan").map_err(internal)?,
        };
        out.push((
            r.try_get("jenis").map_err(internal)?,
            r.try_get("tipe").map_err(internal)?,
            entry,
        ));
    }
    Ok((out, latest))
}

fn entry_json(e: &Entry) -> Value {
    json!({
        "id": e.id,
        "tanggal": e.tanggal.format("%Y-%m-%d").to_string(),
        "persen": number_like_php(e.persen),
        "nilai": e.nilai.map_or(Value::Null, number_like_php),
        "nomor_sp2d": e.nomor_sp2d,
        "tanggal_pembuatan": e.tanggal_pembuatan.map(|d| d.format("%Y-%m-%d").to_string()),
        "tanggal_pencairan": e.tanggal_pencairan.map(|d| d.format("%Y-%m-%d").to_string()),
    })
}

/// Entri terbaru: tanggal terbaru, lalu id terbesar (`latestEntry`).
fn latest(entries: &[&Entry]) -> Option<Entry> {
    entries
        .iter()
        .copied()
        .max_by(|a, b| (a.tanggal, a.id).cmp(&(b.tanggal, b.id)))
        .cloned()
}

/// `buildSectionSummary` untuk satu jenis.
fn section_summary(rows: &[(String, String, Entry)], jenis: &str) -> Value {
    let of = |tipe: &str| -> Vec<&Entry> {
        rows.iter()
            .filter(|(j, t, _)| j == jenis && t == tipe)
            .map(|(_, _, e)| e)
            .collect()
    };
    let rencana = of("rencana");
    let realisasi = of("realisasi");
    let latest_rencana = latest(&rencana);
    let latest_realisasi = latest(&realisasi);
    let deviasi = match (&latest_rencana, &latest_realisasi) {
        (Some(p), Some(r)) => Some(round2(r.persen - p.persen)),
        _ => None,
    };
    json!({
        "rencana": rencana.iter().map(|e| entry_json(e)).collect::<Vec<_>>(),
        "realisasi": realisasi.iter().map(|e| entry_json(e)).collect::<Vec<_>>(),
        "latest_rencana": latest_rencana.map_or(Value::Null, |e| number_like_php(e.persen)),
        "latest_realisasi": latest_realisasi.as_ref().map_or(Value::Null, |e| number_like_php(e.persen)),
        "deviasi": deviasi.map_or(Value::Null, number_like_php),
    })
}

/// `buildPayload`: isi `PekerjaanProgressEstimasiResource`.
fn build_payload(
    pekerjaan_id: u64,
    tahun: i64,
    rows: &[(String, String, Entry)],
    updated_at: Option<DateTime<Utc>>,
) -> Value {
    json!({
        "pekerjaan_id": pekerjaan_id,
        "tahun_anggaran": tahun,
        "fisik": section_summary(rows, "fisik"),
        "keuangan": section_summary(rows, "keuangan"),
        "updated_at": carbon_json(updated_at),
    })
}

/// Pekerjaan harus ada dan berada dalam scope pengguna. Selain itu 404 (`findOrFail`).
async fn ensure_in_scope(state: &AppState, actor: u64, pekerjaan_id: u64) -> Result<(), ApiError> {
    pekerjaan::find(&state.pool, pekerjaan_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let roles = auth::login::roles_of(&state.pool, actor)
        .await
        .map_err(internal)?;
    if !access::user_can_access(&state.pool, actor, &roles, pekerjaan_id)
        .await
        .map_err(internal)?
    {
        return Err(ApiError::not_found());
    }
    Ok(())
}

/// `GET /api/pekerjaan/{id}/progress-estimasi?tahun=`.
pub async fn show(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let tahun = query_tahun(query.get("tahun"))?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    ensure_in_scope(&state, user.user_id, id).await?;
    let (rows, updated) = load_rows(&state.pool, id, tahun).await?;
    Ok(Json(json!({ "data": build_payload(id, tahun, &rows, updated) })).into_response())
}

/// `PUT /api/pekerjaan/{id}/progress-estimasi`: ganti seluruh riwayat untuk tahun tersebut.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let input =
        parse_update(&body).map_err(|e| ApiError::validation("The given data was invalid.", e))?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    ensure_in_scope(&state, user.user_id, id).await?;

    let mut tx = state.pool.begin().await.map_err(internal)?;
    sqlx::query("DELETE FROM pekerjaan_progress_estimasi_history WHERE pekerjaan_id = ? AND tahun_anggaran = ?")
        .bind(id)
        .bind(input.tahun)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    for (jenis, tipe, entries) in &input.sections {
        for e in entries {
            insert_entry(&mut tx, id, input.tahun, jenis, tipe, e)
                .await
                .map_err(internal)?;
        }
    }
    tx.commit().await.map_err(internal)?;

    let (rows, updated) = load_rows(&state.pool, id, input.tahun).await?;
    Ok(Json(json!({
        "message": "Riwayat progress estimasi berhasil disimpan",
        "data": build_payload(id, input.tahun, &rows, updated),
    }))
    .into_response())
}

/// Satu baris riwayat. Kolom keuangan realisasi hanya diisi untuk `keuangan` + `realisasi`.
async fn insert_entry(
    tx: &mut sqlx::Transaction<'_, MySql>,
    pekerjaan_id: u64,
    tahun: i64,
    jenis: &str,
    tipe: &str,
    e: &Entry,
) -> Result<(), sqlx::Error> {
    let keuangan_realisasi = jenis == "keuangan" && tipe == "realisasi";
    sqlx::query(
        "INSERT INTO pekerjaan_progress_estimasi_history \
         (pekerjaan_id, tahun_anggaran, jenis, tipe, tanggal, persen, nilai, nomor_sp2d, tanggal_pembuatan, tanggal_pencairan, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NOW(), NOW())",
    )
    .bind(pekerjaan_id)
    .bind(tahun)
    .bind(jenis)
    .bind(tipe)
    .bind(e.tanggal)
    .bind(e.persen)
    .bind(if keuangan_realisasi { e.nilai } else { None })
    .bind(if keuangan_realisasi { e.nomor_sp2d.clone() } else { None })
    .bind(if keuangan_realisasi { e.tanggal_pembuatan } else { None })
    .bind(if keuangan_realisasi { e.tanggal_pencairan } else { None })
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_format_allows_up_to_two_decimals() {
        assert!(is_percent_format("45"));
        assert!(is_percent_format("45,5"));
        assert!(is_percent_format("45.25"));
        assert!(!is_percent_format("45.255"));
        assert!(!is_percent_format("-1"));
        assert!(!is_percent_format("."));
        assert!(!is_percent_format("1e2"));
    }

    #[test]
    fn percent_range_and_messages() {
        let mut e = Errors::new();
        assert_eq!(percent_value("p", Some(&json!("100,5")), &mut e), None);
        assert_eq!(e["p"][0], "Field p harus berada antara 0 dan 100.");
        let mut e = Errors::new();
        assert_eq!(percent_value("p", Some(&json!("12,345")), &mut e), None);
        assert_eq!(
            percent_value("p", Some(&json!("12,34")), &mut Errors::new()),
            Some(12.34)
        );
        let mut e = Errors::new();
        percent_value("p", None, &mut e);
        assert_eq!(e["p"][0], "The p field is required.");
    }

    #[test]
    fn latest_prefers_later_date_then_higher_id() {
        let mk = |id, d: u32, p| Entry {
            id,
            tanggal: NaiveDate::from_ymd_opt(2025, 1, d).unwrap(),
            persen: p,
            nilai: None,
            nomor_sp2d: None,
            tanggal_pembuatan: None,
            tanggal_pencairan: None,
        };
        let a = mk(1, 5, 10.0);
        let b = mk(2, 5, 20.0);
        let c = mk(3, 1, 30.0);
        assert_eq!(latest(&[&a, &b, &c]).unwrap().id, 2);
    }

    #[test]
    fn update_requires_tahun_and_checks_sections() {
        let err = parse_update(&json!({"fisik": "x"})).err().unwrap();
        assert_eq!(err["tahun"][0], "The tahun field is required.");
        assert_eq!(err["fisik"][0], "The fisik field must be an array.");
    }
}
