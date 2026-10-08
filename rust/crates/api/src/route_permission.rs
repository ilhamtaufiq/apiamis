//! Middleware route permission untuk `/api/*`, setara `check.route.permission` di Laravel.
//!
//! Tanpa token yang valid, request diteruskan: handler yang butuh login akan
//! mengembalikan 401 sendiri, seperti Laravel (`auth:sanctum` dijalankan dulu).

use auth::permission::{self, Decision};
use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::AppState;

pub async fn check(State(state): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let raw_path = req.uri().path().to_string();
    if !raw_path.starts_with("/api/") {
        return next.run(req).await;
    }
    let method = req.method().as_str().to_string();

    let Some(token) = crate::session::token_from_headers(req.headers(), &state.session.name) else {
        return next.run(req).await;
    };
    let Ok(user) = auth::authenticate(&state.pool, &token).await else {
        return next.run(req).await;
    };

    let roles = match permission::user_role_names(&state.pool, user.user_id).await {
        Ok(r) => r,
        Err(e) => return server_error(e),
    };
    if roles.iter().any(|r| r == "admin") {
        return next.run(req).await;
    }

    let rules = match permission::active_rules(&state.pool, &method).await {
        Ok(r) => r,
        Err(e) => return server_error(e),
    };

    let decision = permission::decide(false, &roles, &raw_path, &method, &rules);
    let route = format!("{method} {}", permission::normalize_path(&raw_path));
    match decision {
        Decision::Allow => next.run(req).await,
        Decision::DenyRule { required_roles } => (
            StatusCode::FORBIDDEN,
            Json(json!({
                "message": "Akses ditolak. Anda tidak memiliki permission untuk mengakses route ini.",
                "required_roles": required_roles,
                "your_roles": roles,
            })),
        )
            .into_response(),
        Decision::DenyAdminOnly => (
            StatusCode::FORBIDDEN,
            Json(json!({
                "message": "Akses ditolak. Route ini hanya dapat diakses oleh admin.",
                "route": route,
                "your_roles": roles,
            })),
        )
            .into_response(),
        Decision::DenyMutationWithoutRule => (
            StatusCode::FORBIDDEN,
            Json(json!({
                "message": "Akses ditolak. Route mutasi ini memerlukan permission eksplisit.",
                "route": route,
                "your_roles": roles,
            })),
        )
            .into_response(),
    }
}

fn server_error(e: sqlx::Error) -> Response {
    crate::desa::internal(e).into_response()
}
