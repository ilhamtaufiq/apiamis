use std::time::Duration;

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post},
    Json, Router,
};
use serde_json::{json, Value};
use shared::{ApiError, Config};
pub mod access;
pub mod audit;
pub mod auth_routes;
pub mod berkas;
pub mod changes;
pub mod checklist;
pub mod crypt;
pub mod desa;
pub mod format;
pub mod foto;
pub mod kecamatan;
pub mod kegiatan;
pub mod koordinat;
pub mod lookup;
pub mod maintenance;
pub mod media;
pub mod notify;
pub mod pagination;
pub mod pekerjaan;
pub mod pekerjaan_detail;
pub mod pekerjaan_rel;
pub mod pekerjaan_write;
pub mod penerima;
pub mod penyedia;
pub mod progress_estimasi;
pub mod progress_metrics;
pub mod ratelimit;
pub mod route_permission;
pub mod session;
pub mod tags_write;
pub mod tiket;
pub mod users;

use tower_http::{
    cors::{AllowHeaders, AllowMethods, AllowOrigin, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

/// Origin yang diizinkan, disalin dari `config/cors.php` (Laravel).
const ALLOWED_ORIGINS: &[&str] = &[
    "http://localhost:3000",
    "http://127.0.0.1:3000",
    "http://localhost:5173",
    "http://127.0.0.1:5173",
    "https://localhost:5173",
    "https://127.0.0.1:5173",
    "http://arumanis.test",
    "https://arumanis.test",
    "http://bun.test",
    "https://bun.test",
    "https://arumanis.cianjur.space",
    "https://bun.cianjur.space",
    "https://apiamis.cianjur.space",
    "https://ami.cianjur.space",
];

/// State bersama untuk handler yang butuh database.
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::MySqlPool,
    /// `APP_URL`, dipakai untuk URL pagination.
    pub app_url: String,
    pub limiter: std::sync::Arc<ratelimit::Limiter>,
    /// Cookie sesi (`arumanis_session`) yang dulu dikelola BFF.
    pub session: session::SessionCookie,
}

impl AppState {
    pub fn new(pool: sqlx::MySqlPool, app_url: String) -> Self {
        Self {
            pool,
            app_url,
            limiter: std::sync::Arc::new(ratelimit::Limiter::default()),
            session: session::SessionCookie::from_env(),
        }
    }
}

/// Memastikan header `Authorization: Bearer <token>` valid. Dipakai handler yang butuh login.
pub async fn require_auth(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<auth::AuthUser, ApiError> {
    let token = session::token_from_headers(headers, &state.session.name)
        .ok_or_else(ApiError::unauthenticated)?;
    auth::authenticate(&state.pool, &token)
        .await
        .map_err(|_| ApiError::unauthenticated())
}

/// Router utama. Dipisah dari `main` supaya bisa diuji tanpa membuka port.
pub fn app(config: &Config, state: AppState) -> Router {
    let permission_state = state.clone();
    let maintenance_state = state.clone();
    Router::new()
        // Sama seperti `health: '/up'` di bootstrap/app.php (Laravel).
        .route("/up", get(up))
        .route("/api/health", get(health))
        .route("/api/kecamatan", get(kecamatan::index))
        .route("/api/kecamatan/{id}", get(kecamatan::show))
        .route("/api/desa", get(desa::index))
        .route("/api/desa/{id}", get(desa::show))
        .route("/api/kegiatan", get(kegiatan::index))
        .route("/api/kegiatan/{id}", get(kegiatan::show))
        .route("/api/auth/login", post(auth_routes::login))
        .route("/api/auth/me", get(auth_routes::me))
        .route("/api/auth/logout", post(auth_routes::logout))
        .route("/api/auth/sync-token", post(auth_routes::sync_token))
        .route("/api/app-settings", get(lookup::index))
        .route(
            "/api/app-settings/maintenance",
            get(lookup::maintenance_status),
        )
        .route("/api/tags", get(lookup::tags_index).post(tags_write::store))
        .route(
            "/api/tags/{id}",
            get(lookup::tags_show)
                .put(tags_write::update)
                .patch(tags_write::update)
                .delete(tags_write::destroy),
        )
        .route("/api/document-types", get(lookup::document_types_index))
        .route("/api/penyedia", get(penyedia::index))
        .route("/api/pekerjaan", get(pekerjaan::index))
        .route(
            "/api/pekerjaan/{id}",
            get(pekerjaan::show)
                .put(pekerjaan_write::update)
                .patch(pekerjaan_write::update),
        )
        .route(
            "/api/foto",
            get(foto::index)
                .post(foto::store)
                .layer(DefaultBodyLimit::max(foto::BODY_LIMIT)),
        )
        // Sebelum `/api/foto/{id}`: segmen statis menang atas parameter.
        .route("/api/foto/bulk", delete(foto::bulk_destroy))
        .route(
            "/api/foto/{id}",
            get(foto::show)
                .put(foto::update)
                .patch(foto::update)
                .post(foto::update_post)
                .delete(foto::destroy)
                .layer(DefaultBodyLimit::max(foto::BODY_LIMIT)),
        )
        // Rute statis (summary, rekap, pekerjaan/...) didaftarkan sebelum `/api/penerima/{id}`.
        // Berkas: `export-pdf`, `upload-from-url`, dan `quick-share` masih di Laravel.
        .route("/api/berkas/jenis-dokumen", get(berkas::jenis_dokumen))
        .route("/api/berkas/bulk", delete(berkas::bulk_destroy))
        .route(
            "/api/berkas",
            get(berkas::index)
                .post(berkas::store)
                .layer(DefaultBodyLimit::max(foto::BODY_LIMIT)),
        )
        .route(
            "/api/berkas/{id}",
            get(berkas::show)
                .put(berkas::update)
                .patch(berkas::update)
                .post(berkas::update_post)
                .delete(berkas::destroy)
                .layer(DefaultBodyLimit::max(foto::BODY_LIMIT)),
        )
        .route("/api/penerima", get(penerima::index).post(penerima::store))
        .route("/api/penerima/summary", get(penerima::summary))
        .route("/api/penerima/rekap", get(penerima::rekap))
        .route(
            "/api/penerima/pekerjaan/{pekerjaan_id}",
            get(penerima::by_pekerjaan),
        )
        .route(
            "/api/penerima/pekerjaan/{pekerjaan_id}/stats/komunal",
            get(penerima::komunal_count),
        )
        .route(
            "/api/penerima/{id}",
            get(penerima::show)
                .put(penerima::update)
                .patch(penerima::update)
                .delete(penerima::destroy),
        )
        .route("/api/penyedia/{id}", get(penyedia::show))
        .route("/api/tiket", get(tiket::index))
        .route("/api/tiket/{id}", get(tiket::show))
        .route("/api/checklist-items", get(checklist::items_index))
        .route("/api/checklist-items/{id}", get(checklist::items_show))
        .route("/api/pekerjaan-checklist", get(checklist::pekerjaan_index))
        .route(
            "/api/pekerjaan-checklist/history",
            get(checklist::history_index),
        )
        .route("/api/post-pekerjaan-checklist", get(checklist::post_index))
        // Catch-all untuk /api: route yang belum ada di Rust tetap lewat pengecekan
        // permission (Laravel menolak lebih dulu, bukan 404).
        .route("/api/{*rest}", any(not_found))
        .with_state(state)
        .route_layer(axum::middleware::from_fn_with_state(
            permission_state,
            route_permission::check,
        ))
        // Layer terakhir = paling luar: maintenance jalan sebelum permission, seperti Laravel.
        .route_layer(axum::middleware::from_fn_with_state(
            maintenance_state,
            maintenance::check,
        ))
        .fallback(not_found)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(config.request_timeout_secs),
        ))
        .layer(DefaultBodyLimit::max(config.body_limit_bytes))
        .layer(cors_layer())
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}

/// Setara `config/cors.php`: `supports_credentials` aktif, sehingga header dan
/// method dicerminkan dari request, bukan `*`.
fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
            origin
                .to_str()
                .map(|o| ALLOWED_ORIGINS.contains(&o) || is_pages_dev_origin(o))
                .unwrap_or(false)
        }))
        .allow_headers(AllowHeaders::mirror_request())
        .allow_credentials(true)
        .expose_headers([header::HeaderName::from_static("x-request-id")])
        .allow_methods(AllowMethods::list([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ]))
}

/// Pola `#^https://[a-z0-9-]+\.pages\.dev$#` dari Laravel (preview Cloudflare Pages).
fn is_pages_dev_origin(origin: &str) -> bool {
    let Some(host) = origin.strip_prefix("https://") else {
        return false;
    };
    let Some(label) = host.strip_suffix(".pages.dev") else {
        return false;
    };
    !label.is_empty()
        && label
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

async fn up() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "apiamis", "runtime": "rust" }))
}

async fn not_found() -> Response {
    ApiError::not_found().into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    fn test_config() -> Config {
        Config {
            app_env: "testing".to_string(),
            app_port: 0,
            request_timeout_secs: 30,
            body_limit_bytes: 1024,
            app_url: "http://localhost".to_string(),
        }
    }

    /// Pool yang tidak pernah terkoneksi kecuali dipakai; cukup untuk route tanpa DB.
    fn test_state() -> AppState {
        AppState::new(
            sqlx::MySqlPool::connect_lazy("mysql://test:test@127.0.0.1:1/none").unwrap(),
            "http://localhost".to_string(),
        )
    }

    async fn send(req: Request<Body>) -> Response {
        app(&test_config(), test_state())
            .oneshot(req)
            .await
            .unwrap()
    }

    async fn body_json(res: Response) -> Value {
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn get_json(path: &str) -> (StatusCode, Value) {
        let res = send(Request::builder().uri(path).body(Body::empty()).unwrap()).await;
        let status = res.status();
        (status, body_json(res).await)
    }

    #[tokio::test]
    async fn up_returns_ok() {
        let (status, body) = get_json("/up").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({ "status": "ok" }));
    }

    #[tokio::test]
    async fn unknown_route_returns_laravel_style_404() {
        let (status, body) = get_json("/tidak-ada").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, json!({ "message": "Not Found." }));
    }

    #[tokio::test]
    async fn response_carries_request_id() {
        let res = send(Request::builder().uri("/up").body(Body::empty()).unwrap()).await;
        assert!(res.headers().contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn cors_allows_configured_origin_with_credentials() {
        let res = send(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/api/health")
                .header(header::ORIGIN, "https://apiamis.cianjur.space")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "https://apiamis.cianjur.space"
        );
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                .unwrap(),
            "true"
        );
    }

    #[tokio::test]
    async fn cors_rejects_unknown_origin() {
        let res = send(
            Request::builder()
                .uri("/up")
                .header(header::ORIGIN, "https://evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert!(!res
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
    }

    #[test]
    fn pages_dev_pattern_matches_only_preview_hosts() {
        assert!(is_pages_dev_origin("https://ami-asisten.pages.dev"));
        assert!(!is_pages_dev_origin("http://ami-asisten.pages.dev"));
        assert!(!is_pages_dev_origin("https://Ami.pages.dev"));
        assert!(!is_pages_dev_origin("https://.pages.dev"));
        assert!(!is_pages_dev_origin("https://a.b.pages.dev"));
    }
}
