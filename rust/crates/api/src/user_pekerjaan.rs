//! Penugasan pekerjaan ke user, tabel `user_pekerjaan`. Setara `UserPekerjaanController` (Laravel):
//!
//! - `GET    /api/user-pekerjaan`                              index (daftar semua, terbaru dulu)
//! - `POST   /api/user-pekerjaan`                              store (assign, `syncWithoutDetaching`)
//! - `DELETE /api/user-pekerjaan/{id}`                         destroy
//! - `GET    /api/user-pekerjaan/user/{userId}`                byUser
//! - `GET    /api/user-pekerjaan/pekerjaan/{pekerjaanId}`      byPekerjaan
//! - `GET    /api/user-pekerjaan/available-users`              availableUsers (bukan admin)
//! - `GET    /api/user-pekerjaan/completeness-gaps`            analisis di `user_pekerjaan_gaps`
//!
//! Semua rute hanya untuk admin (`role:admin`), diperiksa di setiap handler dengan
//! `notifications::require_admin`. Belum dipindah: `POST /api/user-pekerjaan/broadcast-reminders`
//! (butuh render template email `MailTemplateService` dan `MailContentService`).
//!
//! Berbeda dari Laravel:
//! - store menjalankan sinkron pivot, pemberian role `pengawas`, dan notifikasi dalam satu transaksi.
//!   Laravel tanpa transaksi, jadi bila gagal di tengah, pivot bisa tersisa;
//! - `byUser` dan `byPekerjaan` mengurutkan relasi ke id pekerjaan atau id user, karena Laravel tidak
//!   memberi urutan pada `belongsToMany`;
//! - `byUser` dan `byPekerjaan` membalas 404 dengan pesan `No query results for model [...]`
//!   (bentuk `ModelNotFoundException`).

use std::collections::BTreeMap;

use axum::{
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;
use sqlx::{MySql, MySqlPool, Row, Transaction};

use crate::{
    format::number_like_php,
    lookup::carbon_json,
    notifications::{self, NOTIFIABLE_TYPE},
    notify,
    pekerjaan::{self, PekerjaanRow},
    php, require_auth, user_pekerjaan_gaps, AppState,
};

const USER_MODEL: &str = r"App\Models\User";
const PEKERJAAN_MODEL: &str = r"App\Models\Pekerjaan";

type Errors = BTreeMap<String, Vec<String>>;

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!("user-pekerjaan: {e}");
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "Server Error")
}

/// Tambah satu error validasi pada `field`.
fn add(errs: &mut Errors, field: &str, message: impl Into<String>) {
    errs.entry(field.to_string())
        .or_default()
        .push(message.into());
}

fn not_found_model(model: &str, id: &str) -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        format!(r"No query results for model [{model}] {id}"),
    )
}

/// Akses admin. Balas 401 tanpa token dan 403 untuk non-admin.
async fn admin(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let user = require_auth(state, headers).await?;
    notifications::require_admin(&state.pool, user.user_id).await
}

/// Waktu mentah untuk `DB::table` / pivot: `Y-m-d H:i:s`, tanpa cast Carbon.
fn raw_ts(ts: Option<DateTime<Utc>>) -> Value {
    match ts {
        Some(t) => Value::String(t.format("%Y-%m-%d %H:%M:%S").to_string()),
        None => Value::Null,
    }
}

/// Baris `tbl_pekerjaan` seperti `toArray()` model Pekerjaan (casts: pagu float, is_konsultan bool).
fn pekerjaan_json(p: &PekerjaanRow) -> Value {
    json!({
        "id": p.id,
        "kode_rekening": p.kode_rekening,
        "nama_paket": p.nama_paket,
        "pagu": p.pagu.map(number_like_php),
        "is_konsultan": p.is_konsultan,
        "status": p.status,
        "catatan": p.catatan,
        "kecamatan_id": p.kecamatan_id,
        "desa_id": p.desa_id,
        "kegiatan_id": p.kegiatan_id,
        "pengawas_id": p.pengawas_id,
        "pendamping_id": p.pendamping_id,
        "created_at": carbon_json(p.created_at),
        "updated_at": carbon_json(p.updated_at),
    })
}

/// `tbl_kecamatan` seperti `toArray()` model Kecamatan.
async fn kecamatan_json(pool: &MySqlPool, id: i64) -> Result<Value, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, n_kec, created_at, updated_at FROM tbl_kecamatan WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    Ok(match row {
        None => Value::Null,
        Some(r) => json!({
            "id": r.try_get::<i64, _>("id").map_err(internal)?,
            "n_kec": r.try_get::<String, _>("n_kec").map_err(internal)?,
            "created_at": carbon_json(r.try_get("created_at").map_err(internal)?),
            "updated_at": carbon_json(r.try_get("updated_at").map_err(internal)?),
        }),
    })
}

/// `tbl_desa` seperti `toArray()` model Desa (casts: luas double, angka integer).
async fn desa_json(pool: &MySqlPool, id: i64) -> Result<Value, ApiError> {
    let row = sqlx::query(
        "SELECT CAST(id AS SIGNED) AS id, n_desa, luas, jumlah_penduduk, CAST(jumlah_kk AS SIGNED) AS jumlah_kk, \
         target, bjp_master, kecamatan_id, created_at, updated_at FROM tbl_desa WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    Ok(match row {
        None => Value::Null,
        Some(r) => {
            let luas: Option<f64> = r.try_get("luas").map_err(internal)?;
            json!({
                "id": r.try_get::<i64, _>("id").map_err(internal)?,
                "n_desa": r.try_get::<Option<String>, _>("n_desa").map_err(internal)?,
                "luas": luas.map(number_like_php),
                "jumlah_penduduk": r.try_get::<Option<i64>, _>("jumlah_penduduk").map_err(internal)?,
                "jumlah_kk": r.try_get::<Option<i64>, _>("jumlah_kk").map_err(internal)?,
                "target": r.try_get::<i64, _>("target").map_err(internal)?,
                "bjp_master": r.try_get::<i64, _>("bjp_master").map_err(internal)?,
                "kecamatan_id": r.try_get::<Option<i64>, _>("kecamatan_id").map_err(internal)?,
                "created_at": carbon_json(r.try_get("created_at").map_err(internal)?),
                "updated_at": carbon_json(r.try_get("updated_at").map_err(internal)?),
            })
        }
    })
}

/// Pekerjaan dengan relasi `kecamatan`, `desa`, `kegiatan` (dan `pivot` bila diminta), seperti Laravel.
async fn pekerjaan_with_relations(
    pool: &MySqlPool,
    p: &PekerjaanRow,
) -> Result<Map<String, Value>, ApiError> {
    let mut m = match pekerjaan_json(p) {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    m.insert(
        "kecamatan".into(),
        match p.kecamatan_id {
            Some(id) => kecamatan_json(pool, id).await?,
            None => Value::Null,
        },
    );
    m.insert(
        "desa".into(),
        match p.desa_id {
            Some(id) => desa_json(pool, id).await?,
            None => Value::Null,
        },
    );
    m.insert(
        "kegiatan".into(),
        match p.kegiatan_id {
            Some(id) => crate::kegiatan_write::attributes(pool, id as u64)
                .await?
                .map(Value::Object)
                .unwrap_or(Value::Null),
            None => Value::Null,
        },
    );
    Ok(m)
}

/// `GET /api/user-pekerjaan`: semua assignment dengan nama user dan pekerjaan, `created_at` terbaru dulu.
pub async fn index(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    admin(&state, &headers).await?;
    let rows = sqlx::query(
        "SELECT up.id, up.user_id, up.pekerjaan_id, u.name AS user_name, u.email AS user_email, \
         p.nama_paket AS pekerjaan_nama, p.pagu AS pekerjaan_pagu, up.created_at \
         FROM user_pekerjaan up \
         JOIN users u ON u.id = up.user_id \
         JOIN tbl_pekerjaan p ON p.id = up.pekerjaan_id \
         ORDER BY up.created_at DESC, up.id DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;
    let mut data = Vec::with_capacity(rows.len());
    for r in &rows {
        let created: Option<DateTime<Utc>> = r.try_get("created_at").map_err(internal)?;
        let pagu: Option<f64> = r.try_get("pekerjaan_pagu").map_err(internal)?;
        data.push(json!({
            "id": r.try_get::<u64, _>("id").map_err(internal)?,
            "user_id": r.try_get::<u64, _>("user_id").map_err(internal)?,
            "pekerjaan_id": r.try_get::<u64, _>("pekerjaan_id").map_err(internal)?,
            "user_name": r.try_get::<String, _>("user_name").map_err(internal)?,
            "user_email": r.try_get::<String, _>("user_email").map_err(internal)?,
            "pekerjaan_nama": r.try_get::<Option<String>, _>("pekerjaan_nama").map_err(internal)?,
            "pekerjaan_pagu": pagu.map(number_like_php),
            "created_at": raw_ts(created),
        }));
    }
    Ok(Json(json!({ "status": "success", "data": data })).into_response())
}

/// `POST /api/user-pekerjaan`: `user_id` wajib dan ada; `pekerjaan_ids` wajib, berupa array, setiap
/// elemen harus ada di `tbl_pekerjaan`.
pub async fn store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    admin(&state, &headers).await?;
    let mut errs: Errors = BTreeMap::new();

    // user_id: required, lalu exists:users,id.
    let mut user_id: Option<u64> = None;
    match body.get("user_id").filter(|v| !is_blank(v)) {
        None => add(&mut errs, "user_id", "The user id field is required."),
        Some(v) => {
            let id = int_of(v);
            let found = match id {
                Some(id) => exists(&state.pool, "users", id).await?,
                None => false,
            };
            match id {
                Some(id) if found => user_id = Some(id as u64),
                _ => add(&mut errs, "user_id", "The selected user id is invalid."),
            }
        }
    }

    // pekerjaan_ids: required, array, dan setiap elemen exists:tbl_pekerjaan,id.
    let ids_raw = body.get("pekerjaan_ids").filter(|v| !is_blank(v));
    let mut pekerjaan_ids: Vec<u64> = Vec::new();
    let mut jumlah_input = 0usize;
    match ids_raw {
        None => add(
            &mut errs,
            "pekerjaan_ids",
            "The pekerjaan ids field is required.",
        ),
        Some(Value::Array(items)) => {
            jumlah_input = items.len();
            for (i, item) in items.iter().enumerate() {
                let ok = match int_of(item) {
                    Some(id) => {
                        let found = exists(&state.pool, "tbl_pekerjaan", id).await?;
                        if found {
                            pekerjaan_ids.push(id as u64);
                        }
                        found
                    }
                    None => false,
                };
                if !ok {
                    add(
                        &mut errs,
                        &format!("pekerjaan_ids.{i}"),
                        format!("The selected pekerjaan ids.{i} is invalid."),
                    );
                }
            }
        }
        Some(_) => add(
            &mut errs,
            "pekerjaan_ids",
            "The pekerjaan ids field must be an array.",
        ),
    }
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    let Some(user_id) = user_id else {
        return Err(internal("validasi user-pekerjaan: user_id tidak terisi"));
    };
    let mut unique = pekerjaan_ids;
    unique.sort_unstable();
    unique.dedup();

    let mut tx: Transaction<'_, MySql> = state.pool.begin().await.map_err(internal)?;

    // syncWithoutDetaching: tambah yang belum ada, pertahankan timestamp yang sudah ada.
    for pid in &unique {
        sqlx::query(
            "INSERT IGNORE INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())",
        )
        .bind(user_id)
        .bind(pid)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    }

    // grantPengawasRoleIfEligible: tambah role `pengawas` bila user belum punya peran lapangan.
    let has_role: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM model_has_roles mr JOIN roles r ON r.id = mr.role_id \
         WHERE mr.model_type = ? AND mr.model_id = ? AND r.name IN ('pengawas', 'konsultan_pengawas', 'tfl')",
    )
    .bind(USER_MODEL)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    if has_role == 0 {
        let role_id: u64 = sqlx::query_scalar(
            "SELECT id FROM roles WHERE name = 'pengawas' AND guard_name = 'web' LIMIT 1",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(internal)?;
        sqlx::query(
            "INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, ?, ?)",
        )
        .bind(role_id)
        .bind(USER_MODEL)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    }

    // Notifikasi ke user: nama pekerjaan diurutkan id, jumlah dari input mentah (seperti count()).
    let placeholders = vec!["?"; unique.len()].join(",");
    let sql =
        format!("SELECT nama_paket FROM tbl_pekerjaan WHERE id IN ({placeholders}) ORDER BY id");
    let mut q = sqlx::query_scalar::<_, String>(&sql);
    for pid in &unique {
        q = q.bind(pid);
    }
    let nama: Vec<String> = q.fetch_all(&mut *tx).await.map_err(internal)?;
    let message = format!(
        "Anda telah di-assign ke {jumlah_input} pekerjaan baru: {}",
        nama.join(", ")
    );
    notify::to_users(
        &mut tx,
        &[user_id],
        "Penugasan Pekerjaan Baru",
        &message,
        Some("/pekerjaan"),
        "info",
    )
    .await
    .map_err(internal)?;

    tx.commit().await.map_err(internal)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "status": "success",
            "message": "Pekerjaan berhasil di-assign ke user",
        })),
    )
        .into_response())
}

/// `DELETE /api/user-pekerjaan/{id}`. Id yang tidak ada, atau bukan angka, membalas 404.
pub async fn destroy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    admin(&state, &headers).await?;
    let not_found = || {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "error", "message": "Assignment tidak ditemukan" })),
        )
            .into_response()
    };
    let Ok(id) = id.parse::<u64>() else {
        return Ok(not_found());
    };
    let res = sqlx::query("DELETE FROM user_pekerjaan WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(internal)?;
    if res.rows_affected() == 0 {
        return Ok(not_found());
    }
    Ok(
        Json(json!({ "status": "success", "message": "Assignment berhasil dihapus" }))
            .into_response(),
    )
}

/// `GET /api/user-pekerjaan/user/{userId}`: data user dan pekerjaan yang di-assign, dengan pivot.
pub async fn by_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Response, ApiError> {
    admin(&state, &headers).await?;
    let not_found = || not_found_model(USER_MODEL, &user_id);
    let uid: u64 = user_id.parse().map_err(|_| not_found())?;
    let user = sqlx::query("SELECT id, name, email FROM users WHERE id = ?")
        .bind(uid)
        .fetch_optional(&state.pool)
        .await
        .map_err(internal)?
        .ok_or_else(not_found)?;

    let links = sqlx::query(
        "SELECT pekerjaan_id, user_id, created_at, updated_at FROM user_pekerjaan WHERE user_id = ? ORDER BY pekerjaan_id",
    )
    .bind(uid)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    let mut pekerjaan = Vec::with_capacity(links.len());
    for l in &links {
        let pid: u64 = l.try_get("pekerjaan_id").map_err(internal)?;
        let Some(p) = pekerjaan::find(&state.pool, pid).await.map_err(internal)? else {
            continue;
        };
        let mut m = pekerjaan_with_relations(&state.pool, &p).await?;
        m.insert(
            "pivot".into(),
            json!({
                "user_id": uid,
                "pekerjaan_id": pid,
                "created_at": raw_ts(l.try_get("created_at").map_err(internal)?),
                "updated_at": raw_ts(l.try_get("updated_at").map_err(internal)?),
            }),
        );
        pekerjaan.push(Value::Object(m));
    }

    Ok(Json(json!({
        "status": "success",
        "data": {
            "user": {
                "id": user.try_get::<u64, _>("id").map_err(internal)?,
                "name": user.try_get::<String, _>("name").map_err(internal)?,
                "email": user.try_get::<String, _>("email").map_err(internal)?,
            },
            "pekerjaan": pekerjaan,
        }
    }))
    .into_response())
}

/// `GET /api/user-pekerjaan/pekerjaan/{pekerjaanId}`: pekerjaan dan user yang di-assign, dengan pivot.
pub async fn by_pekerjaan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(pekerjaan_id): Path<String>,
) -> Result<Response, ApiError> {
    admin(&state, &headers).await?;
    let not_found = || not_found_model(PEKERJAAN_MODEL, &pekerjaan_id);
    let pid: u64 = pekerjaan_id.parse().map_err(|_| not_found())?;
    let p = pekerjaan::find(&state.pool, pid)
        .await
        .map_err(internal)?
        .ok_or_else(not_found)?;

    let rows = sqlx::query(
        "SELECT u.id, u.google_id, u.name, u.email, u.avatar, u.gender, u.nip, u.jabatan, u.email_verified_at, \
         u.created_at, u.updated_at, up.created_at AS pivot_created_at, up.updated_at AS pivot_updated_at \
         FROM user_pekerjaan up JOIN users u ON u.id = up.user_id \
         WHERE up.pekerjaan_id = ? ORDER BY up.user_id",
    )
    .bind(pid)
    .fetch_all(&state.pool)
    .await
    .map_err(internal)?;

    let mut users = Vec::with_capacity(rows.len());
    for r in &rows {
        let verified: Option<DateTime<Utc>> = r.try_get("email_verified_at").map_err(internal)?;
        let created: Option<DateTime<Utc>> = r.try_get("created_at").map_err(internal)?;
        let updated: Option<DateTime<Utc>> = r.try_get("updated_at").map_err(internal)?;
        let uid: u64 = r.try_get("id").map_err(internal)?;
        users.push(json!({
            "id": uid,
            "google_id": r.try_get::<Option<String>, _>("google_id").map_err(internal)?,
            "name": r.try_get::<String, _>("name").map_err(internal)?,
            "email": r.try_get::<String, _>("email").map_err(internal)?,
            "avatar": r.try_get::<Option<String>, _>("avatar").map_err(internal)?,
            "gender": r.try_get::<Option<String>, _>("gender").map_err(internal)?,
            "nip": r.try_get::<Option<String>, _>("nip").map_err(internal)?,
            "jabatan": r.try_get::<Option<String>, _>("jabatan").map_err(internal)?,
            "email_verified_at": carbon_json(verified),
            "created_at": carbon_json(created),
            "updated_at": carbon_json(updated),
            "pivot": {
                "user_id": uid,
                "pekerjaan_id": pid,
                "created_at": raw_ts(r.try_get("pivot_created_at").map_err(internal)?),
                "updated_at": raw_ts(r.try_get("pivot_updated_at").map_err(internal)?),
            },
        }));
    }

    Ok(Json(json!({
        "status": "success",
        "data": {
            "pekerjaan": {
                "id": p.id,
                "nama_paket": p.nama_paket,
                "pagu": p.pagu.map(number_like_php),
            },
            "users": users,
        }
    }))
    .into_response())
}

/// `GET /api/user-pekerjaan/available-users?search=`: user yang bukan admin, diurutkan nama.
pub async fn available_users(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiError> {
    admin(&state, &headers).await?;
    let params = php::Params::parse(raw.as_deref());
    let search = params.get("search").filter(|s| php::truthy(Some(*s)));
    let mut sql = String::from(
        "SELECT CAST(u.id AS SIGNED) AS id, u.name, u.email FROM users u \
         WHERE NOT EXISTS (SELECT 1 FROM model_has_roles mr JOIN roles r ON r.id = mr.role_id \
         WHERE mr.model_type = ? AND mr.model_id = u.id AND r.name = 'admin')",
    );
    let term = search.map(|s| format!("%{s}%"));
    if term.is_some() {
        sql.push_str(" AND (u.name LIKE ? OR u.email LIKE ?)");
    }
    sql.push_str(" ORDER BY u.name, u.id");
    let mut q = sqlx::query(&sql).bind(NOTIFIABLE_TYPE);
    if let Some(t) = &term {
        q = q.bind(t).bind(t);
    }
    let rows = q.fetch_all(&state.pool).await.map_err(internal)?;
    let mut data = Vec::with_capacity(rows.len());
    for r in &rows {
        data.push(json!({
            "id": r.try_get::<i64, _>("id").map_err(internal)?,
            "name": r.try_get::<String, _>("name").map_err(internal)?,
            "email": r.try_get::<String, _>("email").map_err(internal)?,
        }));
    }
    Ok(Json(json!({ "status": "success", "data": data })).into_response())
}

/// `GET /api/user-pekerjaan/completeness-gaps?gaps[]=&tahun=`.
pub async fn completeness_gaps(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiError> {
    admin(&state, &headers).await?;
    let params = php::Params::parse(raw.as_deref());
    let gaps = gap_filters_from(&params)?;
    let tahun = tahun_from(&params)?;
    let data = user_pekerjaan_gaps::analyze(&state.pool, gaps.as_deref(), tahun)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "status": "success", "data": data })).into_response())
}

/// `gaps` harus array (`gaps[]=`), dan setiap nilai `foto`, `penerima`, atau `progress`.
/// Nilai skalar tidak kosong gagal dengan pesan `must be an array`; skalar kosong dianggap null.
fn gap_filters_from(params: &php::Params) -> Result<Option<Vec<String>>, ApiError> {
    let list = params.array("gaps");
    if list.is_empty() {
        if params.get("gaps").is_some_and(|v| !v.is_empty()) {
            let mut errs = Errors::new();
            add(&mut errs, "gaps", "The gaps field must be an array.");
            return Err(ApiError::validation("The given data was invalid.", errs));
        }
        return Ok(None);
    }
    let mut errs = Errors::new();
    for (i, g) in list.iter().enumerate() {
        if !matches!(g.as_str(), "foto" | "penerima" | "progress") {
            add(
                &mut errs,
                &format!("gaps.{i}"),
                format!("The selected gaps.{i} is invalid."),
            );
        }
    }
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    Ok(Some(list))
}

/// `tahun` opsional: bilangan bulat 2000 sampai 2100.
fn tahun_from(params: &php::Params) -> Result<Option<i64>, ApiError> {
    let Some(raw) = params.get("tahun") else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let mut errs = Errors::new();
    let parsed = raw.trim().parse::<i64>().ok();
    match parsed {
        None => add(&mut errs, "tahun", "The tahun field must be an integer."),
        Some(v) if v < 2000 => add(&mut errs, "tahun", "The tahun field must be at least 2000."),
        Some(v) if v > 2100 => add(
            &mut errs,
            "tahun",
            "The tahun field must not be greater than 2100.",
        ),
        Some(_) => {}
    }
    if !errs.is_empty() {
        return Err(ApiError::validation("The given data was invalid.", errs));
    }
    Ok(parsed)
}

/// Nilai kosong untuk `required`: null, string kosong, atau array kosong.
fn is_blank(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

/// Id integer dari JSON: angka bulat atau string angka.
fn int_of(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().filter(|x| *x > 0),
        Value::String(s) => s.trim().parse::<i64>().ok().filter(|x| *x > 0),
        _ => None,
    }
}

/// Rule `exists:<table>,id`.
async fn exists(pool: &MySqlPool, table: &str, id: i64) -> Result<bool, ApiError> {
    let sql = match table {
        "users" => "SELECT COUNT(*) FROM users WHERE id = ?",
        _ => "SELECT COUNT(*) FROM tbl_pekerjaan WHERE id = ?",
    };
    let n: i64 = sqlx::query_scalar(sql)
        .bind(id as u64)
        .fetch_one(pool)
        .await
        .map_err(internal)?;
    Ok(n > 0)
}
