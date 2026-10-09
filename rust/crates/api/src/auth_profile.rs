//! Profil, avatar, dan impersonasi dari `AuthController` Laravel.
//!
//! - `PUT /api/auth/profile` (`updateProfile`): pembaruan data sendiri, 200 dengan `UserResource`.
//! - `POST /api/auth/avatar` (`uploadAvatar`): unggah satu gambar, mengganti avatar lama.
//! - `DELETE /api/auth/avatar` (`deleteAvatar`): hapus avatar.
//! - `POST /api/auth/impersonate/{user}` (`impersonate`): hanya admin. Membuat token Sanctum
//!   `impersonation-token` untuk user target dan mencatat `impersonation_started` di audit.
//!
//! Deviasi dan catatan:
//! - Respon `UserResource` dikembalikan tanpa pembungkus `data`, sesuai `UserResource::$wrap = null`.
//!   Rute `GET /api/auth/me` di Rust memakai `data`; perlu dicek ulang terhadap produksi.
//! - Format waktu di audit (`updated_at`) memakai `carbon_json` seperti `users_write`, bukan
//!   nilai mentah dari database seperti Laravel.
//! - Impersonasi: urutan cek mengikuti Laravel. Binding `{user}` (404) lebih dulu, lalu auth (401),
//!   lalu role admin (403), lalu cek diri sendiri (422). Tidak ada cek tambahan terhadap target
//!   (admin lain atau akun yang dilindungi tetap bisa diimpersonasi, seperti Laravel).
//!   Token impersonasi memakai `login::create_token`, yaitu tanpa `expires_at`. Batas 720 menit
//!   dari `config/sanctum.php` tidak ditegakkan di kolom itu; lihat catatan di `auth::authenticate`.
//! - Avatar: `image|mimes:jpg,jpeg,png,webp,gif|max:5120`. Tipe dicek dari isi berkas (magic number),
//!   tidak dari ekstensi. Yang didukung: JPEG, PNG, GIF, dan WEBP. Audit tidak ditulis karena
//!   Laravel tidak menyimpan model user saat avatar berubah.
//! - Field multipart selain `avatar` diabaikan. Body JSON yang tidak valid diperlakukan sebagai
//!   input kosong (seperti `$request->all()` di Laravel).

use std::collections::BTreeMap;

use auth::login::{self, UserRow};
use axum::{
    body::Bytes,
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use shared::ApiError;

use crate::{audit, lookup::carbon_json, media, require_auth, users, AppState};

const MODEL_USER: &str = "App\\Models\\User";
const AVATAR_COLLECTION: &str = "avatar";
/// `max:5120` pada aturan `image`, dalam kilobyte.
const AVATAR_MAX_KB: usize = 5120;
const AVATAR_MAX_BYTES: usize = AVATAR_MAX_KB * 1024;
/// Batas body untuk rute avatar: berkas maksimal ditambah sedikit untuk overhead multipart.
pub const AVATAR_BODY_LIMIT: usize = AVATAR_MAX_BYTES + 1024 * 1024;
const GENDERS: [&str; 3] = ["male", "female", "other"];
/// Nama token impersonasi di `personal_access_tokens`.
const IMPERSONATION_TOKEN_NAME: &str = "impersonation-token";

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn validation(errs: BTreeMap<String, Vec<String>>) -> ApiError {
    ApiError::validation("The given data was invalid.", errs)
}

fn add(errs: &mut BTreeMap<String, Vec<String>>, key: &str, message: impl Into<String>) {
    errs.entry(key.to_string()).or_default().push(message.into());
}

/// Body JSON sebagai objek. Body kosong, tidak valid, atau bukan objek menjadi input kosong.
fn parse_body(bytes: &Bytes) -> Map<String, Value> {
    match serde_json::from_slice::<Value>(bytes) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Bcrypt seperti `bcrypt()` Laravel, dengan awalan `$2y$`.
fn hash_password(plain: &str) -> Result<String, ApiError> {
    let hashed = bcrypt::hash(plain, bcrypt::DEFAULT_COST).map_err(internal)?;
    Ok(hashed.replacen("$2b$", "$2y$", 1))
}

/// Aturan `email` yang disederhanakan (sama dengan `users_write`).
fn looks_like_email(s: &str) -> bool {
    let mut parts = s.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && !local.contains(' ')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains(' ')
}

/// Nilai kolom user sebelum perubahan, untuk membandingkan dengan nilai baru.
fn current(col: &str, user: &UserRow) -> Option<String> {
    match col {
        "name" => Some(user.name.clone()),
        "email" => Some(user.email.clone()),
        "avatar" => user.avatar.clone(),
        "gender" => user.gender.clone(),
        "nip" => user.nip.clone(),
        "jabatan" => user.jabatan.clone(),
        _ => None,
    }
}

/// `required|string|max:N` pada field `sometimes`. `None` bila field tidak ada atau tidak valid.
fn sometimes_required_string(
    input: &Map<String, Value>,
    key: &str,
    max: usize,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<String> {
    let value = input.get(key)?;
    let text = match value {
        Value::Null => None,
        Value::String(s) if s.trim().is_empty() => None,
        Value::String(s) => Some(s.trim().to_string()),
        _ => {
            add(errs, key, format!("The {key} field must be a string."));
            return None;
        }
    };
    match text {
        None => {
            add(errs, key, format!("The {key} field is required."));
            None
        }
        Some(t) if t.chars().count() > max => {
            add(
                errs,
                key,
                format!("The {key} field must not be greater than {max} characters."),
            );
            None
        }
        Some(t) => Some(t),
    }
}

/// `nullable|string|max:N`. Hasil luar `None` berarti field tidak ada atau tidak valid.
/// Hasil dalam `None` berarti nilai null, seperti `null` atau string kosong.
fn nullable_string(
    input: &Map<String, Value>,
    key: &str,
    max: usize,
    errs: &mut BTreeMap<String, Vec<String>>,
) -> Option<Option<String>> {
    match input.get(key)? {
        Value::Null => Some(None),
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Some(None)
            } else if t.chars().count() > max {
                add(
                    errs,
                    key,
                    format!("The {key} field must not be greater than {max} characters."),
                );
                None
            } else {
                Some(Some(t.to_string()))
            }
        }
        _ => {
            add(errs, key, format!("The {key} field must be a string."));
            None
        }
    }
}

/// `PUT /api/auth/profile`. Field yang tidak dikirim tidak diubah. Field yang dikirim null menjadi null.
/// Perubahan dicatat di audit `updated` dengan nilai lama dan baru, password tidak ikut (seperti `Auditable`).
pub async fn update_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let actor = require_auth(&state, &headers).await?;
    let input = parse_body(&body);
    let user = login::find_by_id(&state.pool, actor.user_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::unauthenticated)?;

    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut sets: Vec<(&'static str, Option<String>)> = Vec::new();

    if let Some(name) = sometimes_required_string(&input, "name", 255, &mut errs) {
        sets.push(("name", Some(name)));
    }

    // `sometimes|required|email|unique:users,email,{id}`.
    if let Some(value) = input.get("email") {
        match value {
            Value::Null => add(&mut errs, "email", "The email field is required."),
            Value::String(s) if s.trim().is_empty() => {
                add(&mut errs, "email", "The email field is required.")
            }
            Value::String(s) => {
                let email = s.trim().to_string();
                if !looks_like_email(&email) {
                    add(
                        &mut errs,
                        "email",
                        "The email field must be a valid email address.",
                    );
                } else {
                    let taken: i64 =
                        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE email = ? AND id <> ?")
                            .bind(&email)
                            .bind(actor.user_id)
                            .fetch_one(&state.pool)
                            .await
                            .map_err(internal)?;
                    if taken > 0 {
                        add(&mut errs, "email", "The email has already been taken.");
                    } else {
                        sets.push(("email", Some(email)));
                    }
                }
            }
            _ => add(
                &mut errs,
                "email",
                "The email field must be a valid email address.",
            ),
        }
    }

    // `nullable|string|min:6` untuk password. Password tidak di-trim (`TrimStrings` mengecualikannya).
    // String kosong dianggap tidak ada, seperti `!empty()` di controller.
    let mut new_password: Option<String> = None;
    match input.get("password") {
        None | Some(Value::Null) => {}
        Some(Value::String(p)) if p.is_empty() => {}
        Some(Value::String(p)) => {
            if p.chars().count() < 6 {
                add(
                    &mut errs,
                    "password",
                    "The password field must be at least 6 characters.",
                );
            } else {
                new_password = Some(p.clone());
            }
        }
        Some(_) => add(&mut errs, "password", "The password field must be a string."),
    }

    if let Some(v) = nullable_string(&input, "nip", 50, &mut errs) {
        sets.push(("nip", v));
    }
    if let Some(v) = nullable_string(&input, "jabatan", 255, &mut errs) {
        sets.push(("jabatan", v));
    }
    match input.get("gender") {
        None => {}
        Some(Value::Null) => sets.push(("gender", None)),
        Some(Value::String(g)) if g.trim().is_empty() => sets.push(("gender", None)),
        Some(Value::String(g)) => {
            let g = g.trim().to_string();
            if GENDERS.contains(&g.as_str()) {
                sets.push(("gender", Some(g)));
            } else {
                add(&mut errs, "gender", "The selected gender is invalid.");
            }
        }
        Some(_) => {
            add(&mut errs, "gender", "The gender field must be a string.");
            add(&mut errs, "gender", "The selected gender is invalid.");
        }
    }
    if let Some(v) = nullable_string(&input, "avatar", 2048, &mut errs) {
        sets.push(("avatar", v));
    }

    if !errs.is_empty() {
        return Err(validation(errs));
    }

    // Hanya kolom yang benar-benar berubah yang ditulis, seperti `isDirty()` di Eloquent.
    let changes: Vec<(&'static str, Option<String>)> = sets
        .into_iter()
        .filter(|(col, v)| current(col, &user) != *v)
        .collect();
    let new_hash = new_password.as_deref().map(hash_password).transpose()?;

    if changes.is_empty() && new_hash.is_none() {
        let out = users::resource(&state.pool, &state.app_url, actor.user_id)
            .await
            .map_err(internal)?
            .ok_or_else(ApiError::unauthenticated)?;
        return Ok(Json(out));
    }

    let url = format!(
        "{}/api/auth/profile",
        state.app_url.trim_end_matches('/')
    );
    let mut tx = state.pool.begin().await.map_err(internal)?;

    let mut cols: Vec<String> = changes.iter().map(|(c, _)| format!("{c} = ?")).collect();
    if new_hash.is_some() {
        cols.push("password = ?".to_string());
    }
    cols.push("updated_at = NOW()".to_string());
    let sql = format!("UPDATE users SET {} WHERE id = ?", cols.join(", "));
    let mut q = sqlx::query(&sql);
    for (_, v) in &changes {
        q = q.bind(v.clone());
    }
    if let Some(hash) = &new_hash {
        q = q.bind(hash.clone());
    }
    q.bind(actor.user_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;

    let updated_after: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT updated_at FROM users WHERE id = ?")
            .bind(actor.user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(internal)?;

    // Audit `updated`: `old_values`/`new_values` hanya berisi kolom yang berubah dan `updated_at`.
    let mut old = Map::new();
    let mut new = Map::new();
    for (col, v) in &changes {
        old.insert(col.to_string(), json!(current(col, &user)));
        new.insert(col.to_string(), json!(v));
    }
    old.insert("updated_at".into(), carbon_json(user.updated_at));
    new.insert("updated_at".into(), carbon_json(updated_after));
    audit::write(
        &mut tx,
        audit::Entry {
            actor: actor.user_id,
            event: "updated",
            auditable_type: MODEL_USER,
            auditable_id: actor.user_id,
            old: Some(old),
            new: Some(new),
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    let out = users::resource(&state.pool, &state.app_url, actor.user_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::unauthenticated)?;
    Ok(Json(out))
}

/// Satu field `avatar` dari multipart.
enum AvatarInput {
    /// Tidak ada field, atau berkas kosong (upload gagal di Laravel juga dihitung tidak ada).
    Missing,
    /// Field teks tanpa berkas yang tidak kosong.
    Text,
    File(media::Upload),
}

/// Membaca field `avatar`. Multipart yang tidak valid diperlakukan sebagai tanpa berkas.
async fn read_avatar(multipart: Option<Multipart>) -> AvatarInput {
    let Some(mut multipart) = multipart else {
        return AvatarInput::Missing;
    };
    let mut found = AvatarInput::Missing;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            _ => break,
        };
        let name = field.name().unwrap_or_default().to_string();
        let original = field.file_name().map(str::to_string);
        if name != "avatar" {
            continue;
        }
        match original {
            Some(original_name) => {
                let Ok(bytes) = field.bytes().await else {
                    break;
                };
                if !bytes.is_empty() && matches!(found, AvatarInput::Missing) {
                    found = AvatarInput::File(media::Upload {
                        original_name,
                        bytes: bytes.to_vec(),
                    });
                }
            }
            None => {
                let text = field.text().await.unwrap_or_default();
                if !text.trim().is_empty() && matches!(found, AvatarInput::Missing) {
                    found = AvatarInput::Text;
                }
            }
        }
    }
    found
}

/// Tipe MIME dari isi berkas untuk `image|mimes:jpg,jpeg,png,webp,gif`. `None` bila bukan salah satunya.
fn avatar_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// `POST /api/auth/avatar`. Avatar lama dihapus dalam transaksi yang sama dengan avatar baru.
/// Berkas lama dihapus dari disk setelah commit, seperti `clearMediaCollection` dan `singleFile`.
pub async fn upload_avatar(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Option<Multipart>,
) -> Result<Json<Value>, ApiError> {
    let actor = require_auth(&state, &headers).await?;

    let mut errs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let upload = match read_avatar(multipart).await {
        AvatarInput::Missing => {
            add(&mut errs, "avatar", "The avatar field is required.");
            None
        }
        AvatarInput::Text => {
            add(&mut errs, "avatar", "The avatar field must be an image.");
            add(
                &mut errs,
                "avatar",
                "The avatar field must be a file of type: jpg, jpeg, png, webp, gif.",
            );
            None
        }
        AvatarInput::File(up) => {
            let mime = avatar_mime(&up.bytes);
            if mime.is_none() {
                add(&mut errs, "avatar", "The avatar field must be an image.");
                add(
                    &mut errs,
                    "avatar",
                    "The avatar field must be a file of type: jpg, jpeg, png, webp, gif.",
                );
            }
            if up.bytes.len() > AVATAR_MAX_BYTES {
                add(
                    &mut errs,
                    "avatar",
                    format!("The avatar field must not be greater than {AVATAR_MAX_KB} kilobytes."),
                );
            }
            match mime {
                Some(m) if errs.is_empty() => Some((up, m)),
                _ => None,
            }
        }
    };
    if !errs.is_empty() {
        return Err(validation(errs));
    }
    let Some((upload, mime)) = upload else {
        return Err(internal("berkas avatar wajib ada setelah validasi"));
    };

    let mut tx = state.pool.begin().await.map_err(internal)?;
    let old_dirs = media::delete_collection(&mut tx, MODEL_USER, actor.user_id, AVATAR_COLLECTION, None)
        .await?;
    let stored = media::attach(
        &mut tx,
        MODEL_USER,
        actor.user_id,
        AVATAR_COLLECTION,
        &upload,
        mime,
        false,
    )
    .await?;
    if let Err(e) = tx.commit().await {
        media::remove_dirs(&[stored.dir]).await;
        return Err(internal(e));
    }
    media::remove_dirs(&old_dirs).await;

    let out = users::resource(&state.pool, &state.app_url, actor.user_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::unauthenticated)?;
    Ok(Json(out))
}

/// `DELETE /api/auth/avatar`. Tanpa avatar, respon tetap 200 dengan data user.
pub async fn delete_avatar(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let actor = require_auth(&state, &headers).await?;
    let mut tx = state.pool.begin().await.map_err(internal)?;
    let dirs = media::delete_collection(&mut tx, MODEL_USER, actor.user_id, AVATAR_COLLECTION, None)
        .await?;
    tx.commit().await.map_err(internal)?;
    media::remove_dirs(&dirs).await;

    let out = users::resource(&state.pool, &state.app_url, actor.user_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::unauthenticated)?;
    Ok(Json(out))
}

/// `POST /api/auth/impersonate/{user}`. Hanya admin.
///
/// Yang dicek, sesuai Laravel:
/// 1. `{user}` harus ada (binding sebelum auth): 404.
/// 2. Token valid (`auth:sanctum`): 401.
/// 3. Pemanggil punya role `admin` (`role:admin`, Spatie): 403 dengan pesan Spatie.
/// 4. Pemanggil bukan target (`AuthController`): 422 `Cannot impersonate yourself`.
///
/// Respon: `user` (UserResource target), `token` (`id|plain`, token baru bernama `impersonation-token`),
/// dan `message`. Token hanya dikirim di body, tidak di cookie. Audit `impersonation_started`
/// dicatat dengan IP dan user agent pemanggil.
///
/// Catatan: `route_permission` menganggap `/api/auth/impersonate/{id}` bukan admin-only (hanya cocok
/// `/auth/impersonate` persis), jadi bisa lolos bila ada rule `route_permissions` untuk role lain.
/// Cek role di sini wajib dan tidak boleh dilonggarkan.
pub async fn impersonate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(raw_user): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let found = match raw_user.parse::<u64>() {
        Ok(id) => login::find_by_id(&state.pool, id).await.map_err(internal)?,
        Err(_) => None,
    };
    let Some(target) = found else {
        return Err(ApiError::not_found());
    };

    let actor = require_auth(&state, &headers).await?;

    // `role:admin` (Spatie `RoleMiddleware`). Pesan default karena `display_role_in_exception` false.
    let roles = login::roles_of(&state.pool, actor.user_id)
        .await
        .map_err(internal)?;
    if !roles.iter().any(|(_, name)| name == "admin") {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "User does not have the right roles.",
        ));
    }

    if actor.user_id == target.id {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Cannot impersonate yourself",
        ));
    }

    let token = login::create_token(&state.pool, target.id, IMPERSONATION_TOKEN_NAME)
        .await
        .map_err(internal)?;

    let impersonator = login::find_by_id(&state.pool, actor.user_id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::unauthenticated)?;

    let url = format!(
        "{}/api/auth/impersonate/{}",
        state.app_url.trim_end_matches('/'),
        target.id
    );
    let mut new = Map::new();
    new.insert("impersonator_id".into(), json!(impersonator.id));
    new.insert("impersonator_email".into(), json!(impersonator.email));
    new.insert("target_user_id".into(), json!(target.id));
    new.insert("target_user_email".into(), json!(target.email));

    let mut tx = state.pool.begin().await.map_err(internal)?;
    audit::write(
        &mut tx,
        audit::Entry {
            actor: actor.user_id,
            event: "impersonation_started",
            auditable_type: MODEL_USER,
            auditable_id: target.id,
            old: None,
            new: Some(new),
            url: &url,
        },
        &headers,
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;

    let user = users::resource(&state.pool, &state.app_url, target.id)
        .await
        .map_err(internal)?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(json!({
        "user": user,
        "token": token,
        "message": format!("Now impersonating {}", target.name),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_avatar_types_by_content() {
        assert_eq!(avatar_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(avatar_mime(b"GIF89a....."), Some("image/gif"));
        assert_eq!(avatar_mime(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some("image/webp"));
        assert_eq!(avatar_mime(b"%PDF-1.4"), None);
    }

    #[test]
    fn email_shape_matches_users_write() {
        assert!(looks_like_email("a@b.id"));
        assert!(!looks_like_email("a@b"));
        assert!(!looks_like_email("a b@c.id"));
    }
}
