use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use shared::ApiError;
use tower_http::trace::TraceLayer;

/// Router utama. Dipisah dari `main` supaya bisa diuji tanpa membuka port.
pub fn app() -> Router {
    Router::new()
        // Sama seperti `health: '/up'` di bootstrap/app.php (Laravel).
        .route("/up", get(up))
        .route("/api/health", get(health))
        .fallback(not_found)
        .layer(TraceLayer::new_for_http())
}

async fn up() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "service": "apiamis", "runtime": "rust" }))
}

async fn not_found() -> Response {
    let err = ApiError::not_found();
    (StatusCode::NOT_FOUND, Json(err.body())).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    async fn get_json(path: &str) -> (StatusCode, Value) {
        let res = app()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
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
}
