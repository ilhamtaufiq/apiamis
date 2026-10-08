use std::time::Duration;

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use shared::{ApiError, Config};
pub mod kecamatan;

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
}

/// Router utama. Dipisah dari `main` supaya bisa diuji tanpa membuka port.
pub fn app(config: &Config, state: AppState) -> Router {
    Router::new()
        // Sama seperti `health: '/up'` di bootstrap/app.php (Laravel).
        .route("/up", get(up))
        .route("/api/health", get(health))
        .route("/api/kecamatan", get(kecamatan::index))
        .with_state(state)
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
        }
    }

    /// Pool yang tidak pernah terkoneksi kecuali dipakai; cukup untuk route tanpa DB.
    fn test_state() -> AppState {
        AppState {
            pool: sqlx::MySqlPool::connect_lazy("mysql://test:test@127.0.0.1:1/none").unwrap(),
        }
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
    async fn api_health_returns_service_info() {
        let (status, body) = get_json("/api/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["service"], "apiamis");
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

    #[tokio::test]
    async fn kecamatan_requires_bearer_token() {
        let (status, body) = get_json("/api/kecamatan").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, json!({ "message": "Unauthenticated." }));
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
