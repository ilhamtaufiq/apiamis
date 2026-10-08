//! `POST /api/auth/login`, setara `AuthController@login`.

use auth::login::{self, UserRow};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::{desa::internal, maintenance, AppState};

const LOGIN_MAX_PER_MINUTE: usize = 5;

/// Bentuk Carbon saat di-serialisasi (`toJSON`), dipakai untuk kolom datetime model.
/// Belum diverifikasi terhadap respon produksi (lihat log).
fn carbon_json(ts: Option<DateTime<Utc>>) -> Value {
    match ts {
        Some(t) => Value::String(t.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()),
        None => Value::Null,
    }
}

/// Bentuk `UserResource` dengan relasi roles dan permissions yang sudah dimuat.
pub fn user_resource(
    user: &UserRow,
    roles: &[(u64, String)],
    permissions: &[(u64, String)],
    avatar_url: Option<String>,
) -> Value {
    let named = |items: &[(u64, String)]| -> Vec<Value> {
        items
            .iter()
            .map(|(id, name)| json!({ "id": id, "name": name }))
            .collect()
    };
    json!({
        "id": user.id,
        "name": user.name,
        "email": user.email,
        "avatar": user.avatar,
        "avatar_url": avatar_url,
        "gender": user.gender,
        "email_verified_at": carbon_json(user.email_verified_at),
        "nip": user.nip,
        "jabatan": user.jabatan,
        "roles": named(roles),
        "permissions": named(permissions),
        "is_protected_from_deletion": is_protected_from_deletion(&user.email),
        "created_at": carbon_json(user.created_at),
        "updated_at": carbon_json(user.updated_at),
    })
}

/// `User::PROTECTED_FROM_DELETION_EMAILS`.
pub fn is_protected_from_deletion(email: &str) -> bool {
    let e = email.trim().to_lowercase();
    e == "ilhamtaufiq@gmail.com"
}

/// Respon validasi Laravel: `message` default dan `errors` per field.
fn validation_error(errors: BTreeMap<&str, Vec<&str>>) -> Response {
    let mut map = Map::new();
    for (k, v) in errors {
        map.insert(k.to_string(), json!(v));
    }
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "message": "The given data was invalid.", "errors": map })),
    )
        .into_response()
}

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    // throttle:login — per IP dan per email, dihitung sebelum validasi.
    let email_raw = body
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let email_key = if email_raw.is_empty() {
        client_ip(&headers)
    } else {
        email_raw.to_lowercase()
    };
    let window = Duration::from_secs(60);
    for key in [
        format!("login-ip:{}", client_ip(&headers)),
        format!("login-email:{email_key}"),
    ] {
        if let Err(retry) = state.limiter.hit(&key, LOGIN_MAX_PER_MINUTE, window) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", retry.to_string())],
                Json(json!({ "message": "Too Many Attempts." })),
            )
                .into_response();
        }
    }

    let mut errors = BTreeMap::new();
    let email = body
        .get("email")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let password = body.get("password").and_then(Value::as_str).unwrap_or("");
    if email.is_empty() {
        errors.insert("email", vec!["The email field is required."]);
    } else if !email.contains('@') {
        errors.insert(
            "email",
            vec!["The email field must be a valid email address."],
        );
    }
    if password.is_empty() {
        errors.insert("password", vec!["The password field is required."]);
    }
    if !errors.is_empty() {
        return validation_error(errors);
    }

    let user = match login::find_by_email(&state.pool, email).await {
        Ok(u) => u,
        Err(e) => return internal(e).into_response(),
    };
    let credentials_ok = user
        .as_ref()
        .and_then(|u| u.password.as_deref())
        .is_some_and(|hash| login::verify_password(hash, password));
    let Some(user) = user.filter(|_| credentials_ok) else {
        let mut e = BTreeMap::new();
        e.insert("email", vec!["The provided credentials are incorrect."]);
        return validation_error(e);
    };

    // Setelah kredensial benar, mode maintenance tetap menutup login kecuali bypass.
    match maintenance::is_enabled_db(&state.pool).await {
        Ok(true) => {
            let bypass = match maintenance::bypass_list_db(&state.pool).await {
                Ok(list) => list,
                Err(e) => return internal(e).into_response(),
            };
            if !auth::login::is_bypass(&user.email, &bypass) {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "message": "Aplikasi sedang maintenance. Login ditutup sementara.",
                        "code": "MAINTENANCE_MODE",
                        "maintenance": true,
                    })),
                )
                    .into_response();
            }
        }
        Ok(false) => {}
        Err(e) => return internal(e).into_response(),
    }

    let result = async {
        let roles = login::roles_of(&state.pool, user.id).await?;
        let permissions = login::permissions_of(&state.pool, user.id).await?;
        let avatar = login::avatar_media(&state.pool, user.id).await?;
        let token = login::create_token(&state.pool, user.id, "auth-token").await?;
        Ok::<_, sqlx::Error>((roles, permissions, avatar, token))
    }
    .await;
    let (roles, permissions, avatar, token) = match result {
        Ok(v) => v,
        Err(e) => return internal(e).into_response(),
    };

    let avatar_url = avatar.and_then(|(media_id, disk, file_name)| {
        // Hanya disk `public` yang URL-nya diketahui dari kode (`APP_URL/storage/{id}/{file}`).
        (disk == "public").then(|| {
            format!(
                "{}/storage/{media_id}/{file_name}",
                state.app_url.trim_end_matches('/')
            )
        })
    });

    Json(json!({
        "user": user_resource(&user, &roles, &permissions, avatar_url),
        "token": token,
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> UserRow {
        UserRow {
            id: 7,
            name: "Uji".to_string(),
            email: "uji@example.test".to_string(),
            avatar: None,
            gender: None,
            nip: None,
            jabatan: Some("Pengawas".to_string()),
            email_verified_at: None,
            password: None,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn user_resource_has_laravel_keys_and_nested_roles() {
        let v = user_resource(&sample(), &[(2, "pengawas".to_string())], &[], None);
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "avatar",
                "avatar_url",
                "created_at",
                "email",
                "email_verified_at",
                "gender",
                "id",
                "is_protected_from_deletion",
                "jabatan",
                "name",
                "nip",
                "permissions",
                "roles",
                "updated_at",
            ]
        );
        assert_eq!(v["roles"], json!([{ "id": 2, "name": "pengawas" }]));
        assert_eq!(v["is_protected_from_deletion"], false);
    }

    #[test]
    fn protected_email_is_case_insensitive() {
        assert!(is_protected_from_deletion(" ILHAMTAUFIQ@gmail.com "));
        assert!(!is_protected_from_deletion("orang@lain.id"));
    }

    #[test]
    fn client_ip_uses_first_forwarded_address() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "203.0.113.5, 10.0.0.1".parse().unwrap());
        assert_eq!(client_ip(&h), "203.0.113.5");
        assert_eq!(client_ip(&HeaderMap::new()), "unknown");
    }
}
