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

use crate::{desa::internal, lookup::carbon_json, pekerjaan, require_auth, AppState};

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
            errors.entry(key.to_string()).or_default().push(format!(
                "The selected {} is invalid.",
                key.replace('_', " ")
            ));
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

/// Notifikasi database untuk admin, kecuali pelaku. `broadcast` belum dipindah (K3).
async fn notify_admins(
    tx: &mut sqlx::Transaction<'_, MySql>,
    actor: u64,
    id: u64,
) -> Result<(), sqlx::Error> {
    let name = crate::notify::actor_name(tx, actor).await?;
    notify_admins_action(tx, actor, id, "diperbarui", &name).await
}

/// `notifyAdmins` untuk `dibuat`, `diperbarui`, atau `dihapus`: judul dan pesan sama dengan trait Laravel.
async fn notify_admins_action(
    tx: &mut sqlx::Transaction<'_, MySql>,
    actor: u64,
    id: u64,
    action: &str,
    actor_name: &str,
) -> Result<(), sqlx::Error> {
    let message = crate::notify::change_message("Pekerjaan", id, action, actor_name, true);
    let url = format!("/pekerjaan/{id}");
    crate::notify::admins(
        tx,
        actor,
        &format!("Data Pekerjaan {action}"),
        &message,
        Some(&url),
    )
    .await
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
    // T31: scope `byUserRole()` diperiksa di sini. Laravel hanya mengandalkan route permission.
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    if !crate::access::user_can_access(&state.pool, user.user_id, &roles, id)
        .await
        .map_err(internal)?
    {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "Anda tidak memiliki akses untuk pekerjaan ini",
        ));
    }

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
    // Laravel mengembalikan PekerjaanDetailResource setelah update.
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let data = crate::pekerjaan_detail::build(
        &state,
        &headers,
        &row,
        &roles,
        user.user_id,
        &std::collections::HashMap::new(),
    )
    .await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// Input `store` yang sudah lolos validasi sintaksis. `p` dipakai untuk cek `exists:` dan tag.
struct NewPekerjaan {
    p: Parsed,
    kode_rekening: Option<Option<String>>,
    nama_paket: String,
    catatan: Option<String>,
    is_konsultan: bool,
    status: String,
    pagu: f64,
}

/// `string` yang dipangkas (`TrimStrings`); kosong dianggap null (`ConvertEmptyStringsToNull`).
/// Nilai `required` yang kosong atau tidak ada menghasilkan error `required`.
fn store_text(
    obj: &Map<String, Value>,
    errors: &mut Errors,
    key: &str,
    max: usize,
    required: bool,
) -> Option<String> {
    let attr = key.replace('_', " ");
    match obj.get(key) {
        None | Some(Value::Null) => {
            if required {
                errors
                    .entry(key.to_string())
                    .or_default()
                    .push(format!("The {attr} field is required."));
            }
            None
        }
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                if required {
                    errors
                        .entry(key.to_string())
                        .or_default()
                        .push(format!("The {attr} field is required."));
                }
                None
            } else if t.chars().count() > max {
                errors.entry(key.to_string()).or_default().push(format!(
                    "The {attr} field must not be greater than {max} characters."
                ));
                None
            } else {
                Some(t.to_string())
            }
        }
        Some(_) => {
            errors
                .entry(key.to_string())
                .or_default()
                .push(format!("The {attr} field must be a string."));
            None
        }
    }
}

/// Validasi `PekerjaanController@store`, termasuk `required_unless:is_konsultan,true,1` untuk kecamatan dan desa.
fn parse_store(body: &Value) -> Result<NewPekerjaan, Errors> {
    let obj = body.as_object().cloned().unwrap_or_default();
    let mut errors = Errors::new();
    let mut p = Parsed::default();

    let kode_rekening = match obj.get("kode_rekening") {
        None => None,
        Some(_) => Some(store_text(&obj, &mut errors, "kode_rekening", 225, false)),
    };
    let nama_paket = store_text(&obj, &mut errors, "nama_paket", 225, true).unwrap_or_default();
    let catatan = store_text(&obj, &mut errors, "catatan", 5000, false);

    let is_konsultan = match obj.get("is_konsultan") {
        None => false,
        Some(v) => match bool_input(v) {
            Some(b) => b,
            None => {
                errors
                    .entry("is_konsultan".into())
                    .or_default()
                    .push("The is konsultan field must be true or false.".into());
                false
            }
        },
    };
    p.is_konsultan = Some(is_konsultan);

    let status = match obj.get("status") {
        None => "active".to_string(),
        Some(Value::String(s)) if s == "active" || s == "canceled" => s.clone(),
        Some(_) => {
            errors
                .entry("status".into())
                .or_default()
                .push("The selected status is invalid.".into());
            "active".to_string()
        }
    };

    // kecamatan_id dan desa_id: wajib (kecuali konsultan). `null` eksplisit lolos, seperti `nullable`.
    for key in ["kecamatan_id", "desa_id"] {
        let attr = key.replace('_', " ");
        match obj.get(key) {
            None if !is_konsultan => {
                errors
                    .entry(key.into())
                    .or_default()
                    .push(format!("The {attr} field is required."));
            }
            None | Some(Value::Null) => {
                p.fk.insert(key_static_fk(key), None);
            }
            Some(v) => match int_input(v) {
                Some(i) => {
                    p.fk.insert(key_static_fk(key), Some(i));
                }
                None => {
                    errors
                        .entry(key.into())
                        .or_default()
                        .push(format!("The {attr} field must be an integer."));
                }
            },
        }
    }

    for key in ["kegiatan_id", "pengawas_id", "pendamping_id"] {
        let attr = key.replace('_', " ");
        match obj.get(key) {
            None | Some(Value::Null) => {
                p.fk.insert(key_static_fk(key), None);
            }
            Some(v) => match int_input(v) {
                Some(i) => {
                    p.fk.insert(key_static_fk(key), Some(i));
                }
                None => {
                    errors
                        .entry(key.into())
                        .or_default()
                        .push(format!("The {attr} field must be an integer."));
                }
            },
        }
    }

    let pagu = match obj.get("pagu") {
        None | Some(Value::Null) => {
            errors
                .entry("pagu".into())
                .or_default()
                .push("The pagu field is required.".into());
            0.0
        }
        Some(v) => match num_input(v) {
            Some(f) if f >= 0.0 => f,
            Some(_) => {
                errors
                    .entry("pagu".into())
                    .or_default()
                    .push("The pagu field must be at least 0.".into());
                0.0
            }
            None => {
                errors
                    .entry("pagu".into())
                    .or_default()
                    .push("The pagu field must be a number.".into());
                0.0
            }
        },
    };

    match obj.get("tag_ids") {
        None => {}
        Some(Value::Null) => p.tag_ids = Some(Vec::new()),
        Some(Value::Array(items)) => {
            let mut ids = Vec::with_capacity(items.len());
            for item in items {
                match int_input(item).and_then(|i| u64::try_from(i).ok()) {
                    Some(i) => ids.push(i),
                    None => errors
                        .entry("tag_ids.*".into())
                        .or_default()
                        .push("The tag ids.* field must be an integer.".into()),
                }
            }
            p.tag_ids = Some(ids);
        }
        Some(_) => errors
            .entry("tag_ids".into())
            .or_default()
            .push("The tag ids field must be an array.".into()),
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(NewPekerjaan {
        p,
        kode_rekening,
        nama_paket,
        catatan,
        is_konsultan,
        status,
        pagu,
    })
}

/// Nama kolom FK yang `'static` untuk `Parsed::fk`.
fn key_static_fk(key: &str) -> &'static str {
    FK_TABLES
        .iter()
        .map(|(k, _)| *k)
        .find(|k| *k == key)
        .unwrap_or("kecamatan_id")
}

/// Atribut pekerjaan seperti `getAttributes()` untuk audit `created` dan `deleted`.
fn row_attributes(p: &pekerjaan::PekerjaanRow) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(p.id));
    m.insert("kode_rekening".into(), json!(p.kode_rekening));
    m.insert("nama_paket".into(), json!(p.nama_paket));
    m.insert("kecamatan_id".into(), json!(p.kecamatan_id));
    m.insert("desa_id".into(), json!(p.desa_id));
    m.insert("kegiatan_id".into(), json!(p.kegiatan_id));
    m.insert("pagu".into(), json!(p.pagu));
    m.insert("is_konsultan".into(), json!(p.is_konsultan));
    m.insert("status".into(), json!(p.status));
    m.insert("catatan".into(), json!(p.catatan));
    m.insert("pengawas_id".into(), json!(p.pengawas_id));
    m.insert("pendamping_id".into(), json!(p.pendamping_id));
    m.insert("created_at".into(), carbon_json(p.created_at));
    m.insert("updated_at".into(), carbon_json(p.updated_at));
    m
}

/// Sisipkan pekerjaan baru. Kolom `kode_rekening` tidak dikirim bila tidak ada, sehingga default DB berlaku.
async fn insert_pekerjaan(
    tx: &mut sqlx::Transaction<'_, MySql>,
    n: &NewPekerjaan,
) -> Result<(u64, Map<String, Value>), sqlx::Error> {
    let fk = |key: &str| {
        if n.is_konsultan && (key == "kecamatan_id" || key == "desa_id") {
            None
        } else {
            n.p.fk.get(key).copied().flatten()
        }
    };
    let mut cols: Vec<(&'static str, Val)> = Vec::new();
    if let Some(k) = &n.kode_rekening {
        cols.push(("kode_rekening", opt_text(k.clone())));
    }
    cols.push(("nama_paket", Val::Text(n.nama_paket.clone())));
    cols.push(("pagu", Val::Flt(n.pagu)));
    cols.push(("is_konsultan", Val::Bool(n.is_konsultan)));
    cols.push(("status", Val::Text(n.status.clone())));
    cols.push(("kecamatan_id", opt_int(fk("kecamatan_id"))));
    cols.push(("desa_id", opt_int(fk("desa_id"))));
    cols.push(("kegiatan_id", opt_int(fk("kegiatan_id"))));
    cols.push(("pengawas_id", opt_int(fk("pengawas_id"))));
    cols.push(("pendamping_id", opt_int(fk("pendamping_id"))));
    cols.push(("catatan", opt_text(n.catatan.clone())));

    let names: Vec<&str> = cols.iter().map(|(c, _)| *c).collect();
    let marks = vec!["?"; names.len()].join(", ");
    let sql = format!(
        "INSERT INTO tbl_pekerjaan ({}, created_at, updated_at) VALUES ({marks}, NOW(), NOW())",
        names.join(", ")
    );
    let mut q = sqlx::query(&sql);
    for (_, v) in &cols {
        q = match v {
            Val::Null => q.bind(None::<String>),
            Val::Text(s) => q.bind(s.clone()),
            Val::Int(i) => q.bind(*i),
            Val::Flt(f) => q.bind(*f),
            Val::Bool(b) => q.bind(*b),
        };
    }
    let id = q.execute(&mut **tx).await?.last_insert_id();

    let mut attrs = Map::new();
    attrs.insert("id".into(), json!(id));
    for (c, v) in &cols {
        attrs.insert((*c).to_string(), v.to_json());
    }
    Ok((id, attrs))
}

/// Pekerjaan yang masih ditautkan ke unit SPAM atau SPM sanitasi. Sinkronisasi capaian belum dipindah.
async fn has_spam_or_sanitasi_links(
    tx: &mut sqlx::Transaction<'_, MySql>,
    id: u64,
) -> Result<bool, sqlx::Error> {
    let mut linked = false;
    for table in ["tbl_unit_spam_pekerjaan", "tbl_spm_sanitasi_pekerjaan"] {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_name = ?",
        )
        .bind(table)
        .fetch_one(&mut **tx)
        .await?;
        if exists == 0 {
            continue;
        }
        let n: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM {table} WHERE pekerjaan_id = ?"
        ))
        .bind(id)
        .fetch_one(&mut **tx)
        .await?;
        linked |= n > 0;
    }
    Ok(linked)
}

/// Deadlock InnoDB (SQLSTATE 40001). Transaksi yang kalah dijalankan ulang dari awal.
fn is_deadlock(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.code().as_deref() == Some("40001"))
}

/// Jumlah percobaan transaksi `store` dan `destroy` bila terjadi deadlock.
const TX_ATTEMPTS: usize = 5;

/// Jeda sebelum mengulang transaksi yang kalah deadlock.
async fn deadlock_backoff(attempt: usize) {
    tokio::time::sleep(std::time::Duration::from_millis(10 * attempt as u64)).await;
}

/// Satu transaksi `store`: insert, tag, audit, dan notifikasi. Mengembalikan id dan atribut untuk audit.
async fn store_tx(
    pool: &MySqlPool,
    headers: &HeaderMap,
    actor: u64,
    n: &NewPekerjaan,
    url: &str,
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let name = crate::notify::actor_name(&mut tx, actor).await?;
    let (id, attrs) = insert_pekerjaan(&mut tx, n).await?;
    if let Some(tags) = &n.p.tag_ids {
        sync_tags(&mut tx, id, tags).await?;
    }
    crate::audit::write(
        &mut tx,
        crate::audit::Entry {
            actor,
            event: "created",
            auditable_type: "App\\Models\\Pekerjaan",
            auditable_id: id,
            old: None,
            new: Some(attrs),
            url,
        },
        headers,
    )
    .await?;
    notify_admins_action(&mut tx, actor, id, "dibuat", &name).await?;
    tx.commit().await?;
    Ok(id)
}

/// `POST /api/pekerjaan`, setara `PekerjaanController@store`. Respon 200 dengan `PekerjaanDetailResource`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let n =
        parse_store(&body).map_err(|e| ApiError::validation("The given data was invalid.", e))?;
    check_exists(&state.pool, &n.p).await?;
    let url = format!("{}/api/pekerjaan", state.app_url.trim_end_matches('/'));

    let mut attempt = 0;
    let id = loop {
        attempt += 1;
        match store_tx(&state.pool, &headers, user.user_id, &n, &url).await {
            Ok(id) => break id,
            Err(e) if is_deadlock(&e) && attempt < TX_ATTEMPTS => deadlock_backoff(attempt).await,
            Err(e) => return Err(internal(e)),
        }
    };

    let row = pekerjaan::find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    let data = crate::pekerjaan_detail::build(
        &state,
        &headers,
        &row,
        &roles,
        user.user_id,
        &std::collections::HashMap::new(),
    )
    .await?;
    Ok(Json(json!({ "data": data })).into_response())
}

/// Satu transaksi `destroy`. `Ok(false)` bila masih tertaut ke SPAM atau SPM sanitasi (tidak ada perubahan).
async fn destroy_tx(
    pool: &MySqlPool,
    headers: &HeaderMap,
    actor: u64,
    id: u64,
    current: &pekerjaan::PekerjaanRow,
    url: &str,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    if has_spam_or_sanitasi_links(&mut tx, id).await? {
        return Ok(false);
    }
    let name = crate::notify::actor_name(&mut tx, actor).await?;
    // Baris pekerjaan dikunci lebih dulu, lalu audit dan notifikasi, sama dengan `store` dan `update`.
    sqlx::query("DELETE FROM tbl_pekerjaan WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    crate::audit::write(
        &mut tx,
        crate::audit::Entry {
            actor,
            event: "deleted",
            auditable_type: "App\\Models\\Pekerjaan",
            auditable_id: id,
            old: Some(row_attributes(current)),
            new: None,
            url,
        },
        headers,
    )
    .await?;
    notify_admins_action(&mut tx, actor, id, "dihapus", &name).await?;
    tx.commit().await?;
    Ok(true)
}

/// `DELETE /api/pekerjaan/{id}`, setara `PekerjaanController@destroy`.
///
/// Berbeda dari Laravel: pekerjaan di luar scope `byUserRole()` mendapat 403 (seperti update), dan
/// pekerjaan yang masih ditautkan ke SPAM atau SPM sanitasi ditolak 409 karena sinkronisasi capaian belum dipindah.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let user = require_auth(&state, &headers).await?;
    let id: u64 = id.parse().map_err(|_| ApiError::not_found())?;
    let current = pekerjaan::find(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    let roles = auth::login::roles_of(&state.pool, user.user_id)
        .await
        .map_err(internal)?;
    if !crate::access::user_can_access(&state.pool, user.user_id, &roles, id)
        .await
        .map_err(internal)?
    {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "Anda tidak memiliki akses untuk pekerjaan ini",
        ));
    }

    let url = format!("{}/api/pekerjaan/{id}", state.app_url.trim_end_matches('/'));
    let mut attempt = 0;
    loop {
        attempt += 1;
        match destroy_tx(&state.pool, &headers, user.user_id, id, &current, &url).await {
            Ok(true) => break,
            Ok(false) => {
                return Err(ApiError::new(
                    axum::http::StatusCode::CONFLICT,
                    "Pekerjaan masih terhubung ke unit SPAM atau SPM sanitasi. Lepas tautan tersebut terlebih dahulu.",
                ))
            }
            Err(e) if is_deadlock(&e) && attempt < TX_ATTEMPTS => {
                deadlock_backoff(attempt).await
            }
            Err(e) => return Err(internal(e)),
        }
    }

    Ok(Json(json!({ "message": "Pekerjaan deleted successfully" })).into_response())
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
        let u = crate::notify::new_uuid();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
    }
}
