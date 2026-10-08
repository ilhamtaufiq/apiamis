use std::time::Duration;

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use shared::{ApiError, Config};
pub mod access;
pub mod audit;
pub mod audit_logs;
pub mod auth_oauth;
pub mod auth_routes;
pub mod berita_acara;
pub mod berkas;
pub mod berkas_upload_url;
pub mod changes;
pub mod checklist;
pub mod checklist_items_write;
pub mod crypt;
pub mod desa;
pub mod desa_profile;
pub mod desa_write;
pub mod docx_template;
pub mod document_registers;
pub mod document_types_write;
pub mod draft;
pub mod draft_export;
pub mod format;
pub mod foto;
pub mod kecamatan;
pub mod kecamatan_write;
pub mod kegiatan;
pub mod kegiatan_role_write;
pub mod kegiatan_write;
pub mod kontrak;
pub mod kontrak_addendum;
pub mod kontrak_document;
pub mod kontrak_document_data;
pub mod kontrak_register_gap;
pub mod kontrak_xlsx;
pub mod koordinat;
pub mod lookup;
pub mod mailer;
pub mod master_fase;
pub mod maintenance;
pub mod media;
pub mod notifications;
pub mod onlyoffice;
pub mod onlyoffice_editor;
pub mod notify;
pub mod output;
pub mod pagination;
pub mod pekerjaan;
pub mod pekerjaan_checklist_write;
pub mod peripaan;
pub mod pekerjaan_detail;
pub mod pekerjaan_rel;
pub mod pekerjaan_write;
pub mod pengawas_write;
pub mod penerima;
pub mod penyedia;
pub mod penyedia_write;
pub mod progress_estimasi;
pub mod progress_metrics;
pub mod progress_write;
pub mod quality_insight;
pub mod ratelimit;
pub mod roles;
pub mod signature_library;
pub mod sipd_pekerjaan_links;
pub mod route_permission;
pub mod session;
pub mod sk;
pub mod tags_write;
pub mod tiket;
pub mod tiket_write;
pub mod users;
pub mod users_write;
pub mod validation;

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
        .route("/api/kecamatan", post(kecamatan_write::store))
        .route(
            "/api/kecamatan/{id}",
            get(kecamatan::show)
                .put(kecamatan_write::update)
                .patch(kecamatan_write::update)
                .delete(kecamatan_write::destroy),
        )
        .route("/api/desa", get(desa::index))
        .route("/api/desa", post(desa_write::store))
        .route(
            "/api/desa/{id}",
            get(desa::show)
                .put(desa_write::update)
                .patch(desa_write::update)
                .delete(desa_write::destroy),
        )
        .route("/api/desa/kecamatan/{id}", get(desa_write::by_kecamatan))
        .route("/api/desa/{id}/profile", get(desa_profile::profile))
        .route("/api/kegiatan", get(kegiatan::index))
        .route("/api/kegiatan", post(kegiatan_write::store))
        .route(
            "/api/kegiatan/{id}",
            get(kegiatan::show)
                .put(kegiatan_write::update)
                .patch(kegiatan_write::update)
                .delete(kegiatan_write::destroy),
        )
        .route("/api/kegiatan/tahun/{tahun}", get(kegiatan_write::by_tahun))
        .route(
            "/api/kegiatan-role",
            get(kegiatan_role_write::index).post(kegiatan_role_write::store),
        )
        .route("/api/kegiatan-role/{id}", delete(kegiatan_role_write::destroy))
        .route(
            "/api/pengawas",
            get(pengawas_write::index).post(pengawas_write::store),
        )
        .route("/api/pengawas/statistics", get(pengawas_write::statistics))
        .route(
            "/api/users",
            get(users_write::index).post(users_write::store),
        )
        .route(
            "/api/users/{id}",
            get(users_write::show)
                .put(users_write::update)
                .patch(users_write::update)
                .delete(users_write::destroy),
        )
        .route("/api/koordinat/validate", post(koordinat::validate_endpoint))
        .route("/api/progress/pekerjaan/{id}", get(progress_write::report).post(progress_write::store))
        .route(
            "/api/pengawas/{id}",
            get(pengawas_write::show)
                .put(pengawas_write::update)
                .patch(pengawas_write::update)
                .delete(pengawas_write::destroy),
        )
        .route("/api/auth/login", post(auth_routes::login))
        .route("/api/auth/handoff", post(auth_oauth::create_handoff))
        .route("/api/auth/handoff/exchange", post(auth_oauth::exchange_handoff))
        .route("/api/auth/google", get(auth_oauth::redirect_to_google))
        .route(
            "/api/auth/google/callback",
            get(auth_oauth::handle_google_callback),
        )
        .route("/api/auth/me", get(auth_routes::me))
        .route("/api/user", get(users_write::me_raw))
        .route("/api/berkas/{id}/export-pdf", get(onlyoffice::berkas_export_pdf))
        .route("/api/onlyoffice/media/{id}/download", get(onlyoffice::media_download))
        .route("/api/onlyoffice/media/{id}/config", get(onlyoffice_editor::config))
        .route("/api/onlyoffice/callback", post(onlyoffice_editor::callback))
        .route("/api/onlyoffice/health", get(onlyoffice_editor::health))
        .route("/api/onlyoffice/temp/{file}", get(onlyoffice::temp_download))
        .route(
            "/api/berita-acara/sequence",
            get(berita_acara::get_sequence).post(berita_acara::update_sequence),
        )
        .route("/api/audit-logs", get(audit_logs::index))
        .route("/api/audit-logs/{id}", get(audit_logs::show))
        .route(
            "/api/sipd-pekerjaan-links",
            get(sipd_pekerjaan_links::index)
                .put(sipd_pekerjaan_links::upsert)
                .delete(sipd_pekerjaan_links::destroy),
        )
        .route(
            "/api/signature-libraries",
            get(signature_library::index).post(signature_library::store),
        )
        .route(
            "/api/signature-libraries/{id}",
            delete(signature_library::destroy),
        )
        .route(
            "/api/peripaan",
            get(peripaan::index)
                .post(peripaan::store)
                .layer(DefaultBodyLimit::max(foto::BODY_LIMIT)),
        )
        .route("/api/peripaan/{id}", delete(peripaan::destroy))
        .route("/api/roles", get(roles::index).post(roles::store))
        .route(
            "/api/roles/{id}",
            get(roles::show)
                .put(roles::update)
                .patch(roles::update)
                .delete(roles::destroy),
        )
        .route(
            "/api/master-fase-pekerjaan",
            get(master_fase::index).post(master_fase::store),
        )
        .route(
            "/api/master-fase-pekerjaan/{id}",
            get(master_fase::show)
                .put(master_fase::update)
                .patch(master_fase::update)
                .delete(master_fase::destroy),
        )
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
        .route(
            "/api/document-types",
            get(lookup::document_types_index).post(document_types_write::store_type),
        )
        .route(
            "/api/document-registers",
            get(document_registers::index).post(document_registers::store),
        )
        .route(
            "/api/document-registers/{id}",
            put(document_registers::update)
                .patch(document_registers::update)
                .delete(document_registers::destroy),
        )
        .route(
            "/api/document-types/{id}",
            put(document_types_write::update_type)
                .patch(document_types_write::update_type)
                .delete(document_types_write::destroy_type),
        )
        .route("/api/penyedia", get(penyedia::index).post(penyedia_write::store))
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
        .route("/api/berkas/upload-from-url", post(berkas_upload_url::upload_from_url))
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
        .route("/api/draft-pekerjaan", get(draft::index).post(draft::store))
        .route("/api/draft-pekerjaan/export/excel", get(draft_export::export_excel))
        .route(
            "/api/draft-pekerjaan/{id}",
            get(draft::show)
                .put(draft::update)
                .patch(draft::update)
                .delete(draft::destroy),
        )
        .route("/api/output", get(output::index).post(output::store))
        .route("/api/output/summary", get(output::summary))
        .route(
            "/api/output/{id}",
            get(output::show)
                .put(output::update)
                .patch(output::update)
                .delete(output::destroy),
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
        .route(
            "/api/sk",
            get(sk::index)
                .post(sk::store)
                .layer(DefaultBodyLimit::max(sk::BODY_LIMIT)),
        )
        .route(
            "/api/sk/{id}",
            get(sk::show)
                .put(sk::update)
                .patch(sk::update)
                .post(sk::update_post)
                .delete(sk::destroy)
                .layer(DefaultBodyLimit::max(sk::BODY_LIMIT)),
        )
        .route(
            "/api/penyedia/{id}",
            get(penyedia::show)
                .post(penyedia_write::update_post)
                .put(penyedia_write::update)
                .patch(penyedia_write::update)
                .delete(penyedia_write::destroy),
        )
        .route("/api/data-quality/stats", get(quality_insight::stats))
        .route("/api/data-quality/items", get(quality_insight::items))
        .route("/api/data-quality/action-inbox", get(quality_insight::action_inbox))
        .route("/api/client-error-reports", post(quality_insight::store))
        .route("/api/tiket", get(tiket::index).post(tiket_write::store))
        // Rute statis `bulk-update` didaftarkan sebelum `/{id}`.
        .route("/api/tiket/bulk-update", post(tiket_write::bulk_update))
        .route(
            "/api/tiket/{id}",
            get(tiket::show)
                .put(tiket_write::update)
                .patch(tiket_write::update)
                .delete(tiket_write::destroy),
        )
        .route("/api/tiket/{id}/comments", post(tiket_write::store_comment))
        .route(
            "/api/checklist-items",
            get(checklist::items_index).post(checklist_items_write::store),
        )
        .route(
            "/api/checklist-items/reorder",
            post(checklist_items_write::reorder),
        )
        .route(
            "/api/checklist-items/{id}",
            get(checklist::items_show)
                .put(checklist_items_write::update)
                .patch(checklist_items_write::update)
                .delete(checklist_items_write::destroy),
        )
        .route("/api/pekerjaan-checklist", get(checklist::pekerjaan_index))
        .route(
            "/api/pekerjaan-checklist/toggle",
            post(pekerjaan_checklist_write::toggle),
        )
        .route(
            "/api/pekerjaan-checklist/export/excel",
            get(pekerjaan_checklist_write::export_excel),
        )
        .route(
            "/api/pekerjaan-checklist/history",
            get(checklist::history_index),
        )
        .route("/api/post-pekerjaan-checklist", get(checklist::post_index))
        // Catch-all untuk /api: route yang belum ada di Rust tetap lewat pengecekan
        // permission (Laravel menolak lebih dulu, bukan 404).
        .route("/api/kontrak", get(kontrak::index).post(kontrak::store))
        .route(
            "/api/kontrak/{id}",
            get(kontrak::show)
                .put(kontrak::update)
                .patch(kontrak::update)
                .delete(kontrak::destroy),
        )
        .route("/api/kontrak-addendums/register-gaps", get(kontrak_register_gap::register_gaps))
        .route(
            "/api/kontrak/{id}/addendum-register-gaps",
            get(kontrak_register_gap::register_gaps_for_kontrak),
        )
        .route("/api/kontrak-addendums", get(kontrak_addendum::all))
        .route(
            "/api/kontrak-addendums/{id}",
            get(kontrak_addendum::show)
                .put(kontrak_addendum::update)
                .patch(kontrak_addendum::update)
                .delete(kontrak_addendum::destroy),
        )
        .route("/api/kontrak-addendums/{id}/submit", post(kontrak_addendum::submit))
        .route("/api/kontrak-addendums/{id}/process", post(kontrak_addendum::process))
        .route("/api/kontrak-addendums/{id}/approve", post(kontrak_addendum::approve))
        .route(
            "/api/kontrak-addendums/{id}/override-kelengkapan",
            post(kontrak_addendum::override_kelengkapan),
        )
        .route("/api/kontrak-addendums/{id}/reject", post(kontrak_addendum::reject))
        .route("/api/kontrak-addendums/{id}/upload", post(kontrak_addendum::upload))
        .route(
            "/api/kontrak-addendums/{id}/attachment-numbers",
            put(kontrak_addendum::update_attachment_numbers),
        )
        .route(
            "/api/kontrak/{id}/addendums",
            get(kontrak_addendum::index).post(kontrak_addendum::store),
        )
        .route(
            "/api/kontrak/{id}/addendum-numbers",
            post(kontrak_addendum::generate_numbers),
        )
        .route("/api/kontrak/export/excel", get(kontrak_xlsx::export_excel))
        .route(
            "/api/kontrak/export-all-covers",
            get(kontrak_document::export_all_covers),
        )
        .route("/api/kontrak/{id}/export", get(kontrak_document::export))
        .route(
            "/api/kontrak/{id}/export-cover",
            get(kontrak_document::export_cover),
        )
        .route(
            "/api/kontrak/{id}/bap-context",
            get(kontrak_document::bap_context),
        )
        .route(
            "/api/kontrak/{id}/export-bap",
            get(kontrak_document::export_bap),
        )
        .route(
            "/api/kontrak/import/template",
            get(kontrak_xlsx::download_template),
        )
        .route("/api/kontrak/import", post(kontrak_xlsx::import))
        .route("/api/kontrak/pekerjaan/{id}", get(kontrak::by_pekerjaan))
        .route("/api/kontrak/kegiatan/{id}", get(kontrak::by_kegiatan))
        .route("/api/kontrak/penyedia/{id}", get(kontrak::by_penyedia))
        .route("/api/notifications", get(notifications::index))
        .route(
            "/api/notifications/{id}/read",
            post(notifications::mark_as_read),
        )
        .route(
            "/api/notifications/mark-all-read",
            post(notifications::mark_all_as_read),
        )
        .route(
            "/api/notifications/broadcast",
            post(notifications::send_broadcast),
        )
        .route(
            "/api/notifications/broadcast-history",
            get(notifications::broadcast_history),
        )
        .route(
            "/api/notifications/broadcast/{id}",
            delete(notifications::delete_broadcast),
        )
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
