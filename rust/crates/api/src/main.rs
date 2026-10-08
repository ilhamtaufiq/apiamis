use api::{app, AppState};
use shared::Config;
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let _ = dotenvy::from_path("../.env");
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env();
    let addr = SocketAddr::from(([0, 0, 0, 0], config.app_port));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("gagal bind port");
    tracing::info!(%addr, env = %config.app_env, "apiamis rust listening");

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect_lazy(&database_url).expect("DATABASE_URL tidak valid");

    axum::serve(listener, app(&config, AppState { pool }))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server berhenti dengan error");
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown");
}
