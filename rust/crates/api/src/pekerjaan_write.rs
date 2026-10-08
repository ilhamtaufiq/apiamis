//! `PUT/PATCH /api/pekerjaan/{id}`, setara `PekerjaanController@update`.
//!
//! Efek samping yang ikut dipindah, seperti trait Laravel:
//! - audit log `tbl_audit_logs` (`Auditable`), hanya jika ada perubahan;
//! - notifikasi database untuk setiap admin kecuali pelaku (`NotifiesAdminsOnChanges`).
//!
//! Belum dipindah: broadcast realtime `PekerjaanUpdated` (`BroadcastsPekerjaanRealtime`),
//! menunggu keputusan K3. Respon memakai bentuk daftar, bukan `PekerjaanDetailResource` (T25).

use std::collections::BTreeMap;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, QueryBuilder};

use crate::{desa::internal, pekerjaan, pekerjaan_rel, require_auth, AppState};

type Errors = BTreeMap<String, Vec<String>>;

/// Nilai kolom. `Null` = NULL di DB.
#[derive(Debug, Clone, PartialEq)]
enum Val {
    Null,
    Text(String),
    Int(i64),
    Flt(f64),
    Bool(bool),
}

impl Val {
    fn to_json(&self) -> Value {
        match self {
            Val::Null => Value::Null,
            Val::Text(s) => json!(s),
            Val::Int(i) => json!(i),
            Val::Flt(f) => json!(f),
            Val::Bool(b) => json!(b),
        }
    }
}

/// Satu kolom yang benar-benar berubah.
struct Change {
    col: &'static str,
    old: Val,
    new: Val,
}

/// Hasil validasi: hanya kunci yang ada di body (seperti `$request->validate` + `sometimes`).
#[derive(Default)]
struct Parsed {
    text: BTreeMap<&'static str, Option<String>>,
    is_konsultan: Option<bool>,
    fk: BTreeMap<&'static str, Option<i64>>,
    pagu: Option<Option<f64>>,
    tag_ids: Option<Vec<u64>>,
}

/// Tabel tujuan untuk pengecekan `exists:` pada kolom foreign key.
const FK_TABLES: &[(&str, &str)] = &[
    ("kecamatan_id", "tbl_kecamatan"),
    ("desa_id", "tbl_desa"),
    ("kegiatan_id", "tbl_kegiatan"),
    ("pengawas_id", "pengawas"),
    ("pendamping_id", "pengawas"),
];

/// `boolean` di Laravel: menerima true/false, 1/0, "1"/"0", "on"/"off", "yes"/"no", "true"/"false".
fn bool_input(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_i64()? {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        },
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "1" | "true" | "on" | "yes" => Some(true),
            "0" | "false" | "off" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// `integer`: angka JSON atau string angka.
fn int_input(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `numeric`: angka JSON atau string angka.
fn num_input(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok().filter(|f: &f64| f.is_finite()),
        _ => None,
    }
}

/// Validasi sintaksis (tanpa cek database). Aturan mengikuti `PekerjaanController@update`.
fn parse(body: &Value) -> Result<Parsed, Errors> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut p = Parsed::default();
    let mut errors = Errors::new();
    let mut err = |field: &str, msg: &str| {
        errors
            .entry(field.to_string())
            .or_default()
            .push(msg.to_string());
    };

    for (key, max) in [
        ("kode_rekening", 225usize),
        ("nama_paket", 225),
        ("catatan", 5000),
    ] {
        if let Some(v) = obj.get(key) {
            match v {
                Value::Null => {
                    p.text.insert(key_static(key), None);
                }
                Value::String(s) if s.chars().count() <= max => {
                    p.text.insert(key_static(key), Some(s.clone()));
                }
                _ => err(
                    key,
                    &format!("The {key} field must be a string with at most {max} characters."),
                ),
            }
        }
    }

    if let Some(v) = obj.get("status") {
        match v {
            Value::String(s) if s == "active" || s == "canceled" => {
                p.text.insert("status", Some(s.clone()));
            }
            _ => err("status", "The selected status is invalid."),
        }
    }

    if let Some(v) = obj.get("is_konsultan") {
        match bool_input(v) {
            Some(b) => p.is_konsultan = Some(b),
            None => err(
                "is_konsultan",
                "The is konsultan field must be true or false.",
            ),
        }
    }

    for (key, _) in FK_TABLES {
        let key: &'static str = key;
        if let Some(v) = obj.get(key) {
            match v {
                Value::Null => {
                    p.fk.insert(key, None);
                }
                _ => match int_input(v) {
                    Some(i) => {
                        p.fk.insert(key, Some(i));
                    }
                    None => err(key, &format!("The {key} field must be an integer.")),
                },
            }
        }
    }

    if let Some(v) = obj.get("pagu") {
        match v {
            Value::Null => p.pagu = Some(None),
            _ => match num_input(v) {
                Some(f) if f >= 0.0 => p.pagu = Some(Some(f)),
                Some(_) => err("pagu", "The pagu field must be at least 0."),
                None => err("pagu", "The pagu field must be a number."),
            },
        }
    }

    if let Some(v) = obj.get("tag_ids") {
        match v {
            Value::Null => p.tag_ids = Some(Vec::new()),
            Value::Array(items) => {
                let mut ids = Vec::with_capacity(items.len());
                for item in items {
                    match int_input(item).and_then(|i| u64::try_from(i).ok()) {
                        Some(i) => ids.push(i),
                        None => err("tag_ids.*", "The tag_ids.* field must be an integer."),
                    }
                }
                p.tag_ids = Some(ids);
            }
            _ => err("tag_ids", "The tag ids field must be an array."),
        }
    }

    if errors.is_empty() {
        Ok(p)
    } else {
        Err(errors)
    }
}

/// Mengubah string dinamis ke `&'static str` dari daftar kolom yang dikenal.
fn key_static(key: &str) -> &'static str {
    match key {
        "kode_rekening" => "kode_rekening",
        "nama_paket" => "nama_paket",
        _ => "catatan",
    }
}

/// Cek `exists:` untuk setiap foreign key dan tag. Mengumpulkan error seperti Laravel.
async fn check_exists(pool: &MySqlPool, p: &Parsed) -> Result<(), ApiError> {
    let mut errors = Errors::new();
    for (key, id) in &p.fk {
        let Some(id) = id else { continue };
        let table = FK_TABLES
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, t)| *t)
            .unwrap_or("");
        let found: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE id = ?"))
            .bind(*id)
            .fetch_one(pool)
            .await
            .map_err(internal)?;
        if found == 0 {
            errors
                .entry(key.to_string())
                .or_default()
                .push(format!("The selected {key} is invalid."));
        }
    }
    if let Some(tags) = &p.tag_ids {
        for tag in tags {
            let found: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tbl_tags WHERE id = ?")
                .bind(*tag)
                .fetch_one(pool)
                .await
                .map_err(internal)?;
            if found == 0 {
                errors
                    .entry("tag_ids.*".to_string())
                    .or_default()
                    .push("The selected tag_ids.* is invalid.".to_string());
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ApiError::validation("The given data was invalid.", errors))
    }
}

/// Menentukan kolom yang berubah. Aturan konsultan sama dengan Laravel.
fn plan(current: &pekerjaan::PekerjaanRow, p: &Parsed) -> Result<Vec<Change>, ApiError> {
    let mut fk = p.fk.clone();
    if let Some(v) = p.is_konsultan {
        if v {
            fk.insert("kecamatan_id", None);
            fk.insert("desa_id", None);
        }
    } else if current.is_konsultan {
        // `$request->boolean('is_konsultan', true)` bernilai true bila kunci tidak ada
        fk.remove("kecamatan_id");
        fk.remove("desa_id");
    }

    let will_konsultan = p.is_konsultan.unwrap_or(current.is_konsultan);
    if !will_konsultan {
        let kec = fk
            .get("kecamatan_id")
            .copied()
            .flatten()
            .or(current.kecamatan_id);
        let desa = fk.get("desa_id").copied().flatten().or(current.desa_id);
        if kec.is_none() || desa.is_none() {
            let mut errors = Errors::new();
            errors.insert("kecamatan_id".into(), vec!["Kecamatan wajib diisi.".into()]);
            errors.insert("desa_id".into(), vec!["Desa wajib diisi.".into()]);
            return Err(ApiError::validation(
                "Kecamatan dan desa wajib diisi untuk pekerjaan non-konsultan.",
                errors,
            ));
        }
    }

    let mut candidates: Vec<(&'static str, Val, Val)> = Vec::new();
    for (col, v) in &p.text {
        let old = match *col {
            "kode_rekening" => current.kode_rekening.clone(),
            "nama_paket" => current.nama_paket.clone(),
            "catatan" => current.catatan.clone(),
            "status" => current.status.clone(),
            _ => None,
        };
        candidates.push((col, opt_text(old), opt_text(v.clone())));
    }
    if let Some(v) = p.is_konsultan {
        candidates.push((
            "is_konsultan",
            Val::Bool(current.is_konsultan),
            Val::Bool(v),
        ));
    }
    for (col, v) in &fk {
        let old = match *col {
            "kecamatan_id" => current.kecamatan_id,
            "desa_id" => current.desa_id,
            "kegiatan_id" => current.kegiatan_id,
            "pengawas_id" => current.pengawas_id.map(|i| i as i64),
            "pendamping_id" => current.pendamping_id.map(|i| i as i64),
            _ => None,
        };
        candidates.push((col, opt_int(old), opt_int(*v)));
    }
    if let Some(v) = p.pagu {
        let old = current.pagu;
        candidates.push((
            "pagu",
            match old {
                Some(f) => Val::Flt(f),
                None => Val::Null,
            },
            match v {
                Some(f) => Val::Flt(f),
                None => Val::Null,
            },
        ));
    }

    Ok(candidates
        .into_iter()
        .filter(|(_, old, new)| old != new)
        .map(|(col, old, new)| Change { col, old, new })
        .collect())
}

fn opt_text(v: Option<String>) -> Val {
    v.map(Val::Text).unwrap_or(Val::Null)
}

fn opt_int(v: Option<i64>) -> Val {
    v.map(Val::Int).unwrap_or(Val::Null)
}

/// Menulis perubahan dalam satu UPDATE dengan parameter terikat.
async fn apply(
    tx: &mut sqlx::Transaction<'_, MySql>,
    id: u64,
    changes: &[Change],
) -> Result<(), sqlx::Error> {
    let mut qb = QueryBuilder::<MySql>::new("UPDATE tbl_pekerjaan SET ");
    for (i, c) in changes.iter().enumerate() {
        if i > 0 {
            qb.push(", ");
        }
        qb.push(c.col).push(" = ");
        match &c.new {
            Val::Null => {
                qb.push_bind(None::<String>);
            }
            Val::Text(s) => {
                qb.push_bind(s.clone());
            }
            Val::Int(v) => {
                qb.push_bind(*v);
            }
            Val::Flt(v) => {
                qb.push_bind(*v);
            }
            Val::Bool(v) => {
                qb.push_bind(*v);
            }
        }
    }
    qb.push(", updated_at = NOW() WHERE id = ").push_bind(id);
    qb.build().execute(&mut **tx).await?;
    Ok(())
}

/// Sync pivot `pekerjaan_tag` seperti `tags()->sync()`: tambah yang baru, hapus yang tidak ada.
async fn sync_tags(
    tx: &mut sqlx::Transaction<'_, MySql>,
    pekerjaan_id: u64,
    tags: &[u64],
) -> Result<(), sqlx::Error> {
    let mut wanted: Vec<u64> = tags.to_vec();
    wanted.sort_unstable();
    wanted.dedup();

    let existing: Vec<u64> =
        sqlx::query_scalar("SELECT tag_id FROM pekerjaan_tag WHERE pekerjaan_id = ?")
            .bind(pekerjaan_id)
            .fetch_all(&mut **tx)
            .await?;

    for tag in existing.iter().filter(|t| !wanted.contains(t)) {
        sqlx::query("DELETE FROM pekerjaan_tag WHERE pekerjaan_id = ? AND tag_id = ?")
            .bind(pekerjaan_id)
            .bind(tag)
            .execute(&mut **tx)
            .await?;
    }
    for tag in wanted.iter().filter(|t| !existing.contains(t)) {
        sqlx::query("INSERT INTO pekerjaan_tag (pekerjaan_id, tag_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
            .bind(pekerjaan_id)
            .bind(tag)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Audit log untuk event `updated` (`Auditable::logAudit`).
async fn write_audit(
    tx: &mut sqlx::Transaction<'_, MySql>,
    actor: u64,
    id: u64,
    changes: &[Change],
    url: &str,
    headers: &HeaderMap,
) -> Result<(), sqlx::Error> {
    let mut old = Map::new();
    let mut new = Map::new();
    for c in changes {
        old.insert(c.col.to_string(), c.old.to_json());
        new.insert(c.col.to_string(), c.new.to_json());
    }
    crate::audit::write(
        tx,
        crate::audit::Entry {
            actor,
            event: "updated",
            auditable_type: "App\\Models\\Pekerjaan",
            auditable_id: id,
            old: Some(old),
            new: Some(new),
            url,
        },
        headers,
    )
    .await
}

/// UUID v4 untuk kolom `notifications.id`.
fn new_uuid() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Notifikasi database untuk admin, kecuali pelaku. `broadcast` belum dipindah (K3).
async fn notify_admins(
    tx: &mut sqlx::Transaction<'_, MySql>,
    actor: u64,
    id: u64,
) -> Result<(), sqlx::Error> {
    let actor_name: Option<String> = sqlx::query_scalar("SELECT name FROM users WHERE id = ?")
        .bind(actor)
        .fetch_optional(&mut **tx)
        .await?
        .flatten();
    let actor_name = actor_name.unwrap_or_else(|| "System".to_string());

    let admins: Vec<u64> = sqlx::query_scalar(
        "SELECT u.id FROM users u \
         JOIN model_has_roles mr ON mr.model_id = u.id AND mr.model_type = 'App\\\\Models\\\\User' \
         JOIN roles r ON r.id = mr.role_id WHERE r.name = 'admin' ORDER BY u.id",
    )
    .fetch_all(&mut **tx)
    .await?;

    let data = json!({
        "title": "Data Pekerjaan updated",
        "message": format!(
            "Model Pekerjaan dengan ID #{id} telah updated oleh {actor_name}. Klik untuk membuka detail perubahan."
        ),
        "url": format!("/pekerjaan/{id}"),
        "type": "info",
        "is_banner": false,
        "broadcast_history_id": null,
    })
    .to_string();

    for admin in admins.into_iter().filter(|a| *a != actor) {
        sqlx::query(
            "INSERT INTO notifications (id, type, notifiable_type, notifiable_id, data, created_at, updated_at) \
             VALUES (?, 'App\\\\Notifications\\\\AppNotification', 'App\\\\Models\\\\User', ?, ?, NOW(), NOW())",
        )
        .bind(new_uuid())
        .bind(admin)
        .bind(&data)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// `PUT/PATCH /api/pekerjaan/{id}`.
pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = pekerjaan::find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;

    let parsed =
        parse(&body).map_err(|e| ApiError::validation("The given data was invalid.", e))?;
    check_exists(&state.pool, &parsed).await?;
    let changes = plan(&current, &parsed)?;

    let url = format!("{}/api/pekerjaan/{id}", state.app_url.trim_end_matches('/'));
    let mut tx = state.pool.begin().await.map_err(internal)?;
    if !changes.is_empty() {
        apply(&mut tx, id, &changes).await.map_err(internal)?;
    }
    if let Some(tags) = &parsed.tag_ids {
        sync_tags(&mut tx, id, tags).await.map_err(internal)?;
    }
    if !changes.is_empty() {
        write_audit(&mut tx, user.user_id, id, &changes, &url, &headers)
            .await
            .map_err(internal)?;
        notify_admins(&mut tx, user.user_id, id)
            .await
            .map_err(internal)?;
    }
    tx.commit().await.map_err(internal)?;

    let row = pekerjaan::find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let viewer = pekerjaan_rel::viewer(&state.pool, user.user_id, &roles)
        .await
        .map_err(internal)?;
    let rel = pekerjaan::load(
        &state.pool,
        std::slice::from_ref(&row),
        pekerjaan::Mode {
            summary: false,
            unbounded: false,
        },
        &viewer,
    )
    .await
    .map_err(internal)?;
    let mut response = Json(json!({ "data": pekerjaan::to_resource(&row, &rel) })).into_response();
    response.headers_mut().insert(
        "x-partial-response",
        axum::http::HeaderValue::from_static(pekerjaan::PARTIAL_HEADER),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn booleans_accept_laravel_spellings() {
        assert_eq!(bool_input(&json!("on")), Some(true));
        assert_eq!(bool_input(&json!(0)), Some(false));
        assert_eq!(bool_input(&json!("maybe")), None);
    }

    #[test]
    fn status_must_be_active_or_canceled() {
        assert!(parse(&json!({"status": "active"})).is_ok());
        assert!(parse(&json!({"status": "closed"})).is_err());
    }

    #[test]
    fn only_present_keys_are_parsed() {
        let p = parse(&json!({"nama_paket": "A"})).unwrap();
        assert_eq!(p.text.len(), 1);
        assert!(p.fk.is_empty() && p.pagu.is_none() && p.tag_ids.is_none());
    }

    #[test]
    fn uuid_has_v4_layout() {
        let u = new_uuid();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
    }
}
