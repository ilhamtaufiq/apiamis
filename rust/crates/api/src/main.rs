use api::{app, AppState};
use shared::Config;
use std::net::SocketAddr;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // `.env` root repo Laravel: dicari dari direktori kerja (`rust/`), lalu dari lokasi crate.
    let _ = dotenvy::from_path("../.env");
    let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../.env"));
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // Subcommand CLI pengganti `php artisan`. Argumen lain (tanpa subcommand) menjalankan server.
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("migrate") => {
            run_migrate().await;
            return;
        }
        Some("sync-route-permissions") => {
            run_sync_route_permissions(&args[1..]).await;
            return;
        }
        Some("desa-import-population") => {
            run_desa_import_population(&args[1..]).await;
            return;
        }
        Some("blog-assets-cleanup-orphans") => {
            run_blog_assets_cleanup_orphans(&args[1..]).await;
            return;
        }
        Some("regenerate-thumbs") => {
            run_regenerate_thumbs(&args[1..]).await;
            return;
        }
        _ => {}
    }

    let config = Config::from_env();
    let addr = SocketAddr::from(([0, 0, 0, 0], config.app_port));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("gagal bind port");
    tracing::info!(%addr, env = %config.app_env, "apiamis rust listening");

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect_lazy(&database_url).expect("DATABASE_URL tidak valid");

    axum::serve(
        listener,
        app(&config, AppState::new(pool, config.app_url.clone())),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("server berhenti dengan error");
}

async fn run_migrate() {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&database_url)
        .await
        .expect("gagal konek ke DATABASE_URL");
    match api::migrate::run(&pool).await {
        Ok(r) => tracing::info!(?r, "migrasi selesai"),
        Err(e) => {
            tracing::error!(error = %e, "migrasi gagal");
            std::process::exit(1);
        }
    }
}

/// Koneksi untuk subcommand CLI. Sama dengan `run_migrate`: `DATABASE_URL` wajib.
async fn connect_db() -> sqlx::MySqlPool {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    sqlx::MySqlPool::connect(&database_url)
        .await
        .expect("gagal konek ke DATABASE_URL")
}

/// Keluar dengan kode 1 dan pesan di stderr, seperti `return 1` pada command Laravel.
fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

/// Nilai `--nama=isi` dari daftar argumen.
fn flag_value(args: &[String], name: &str) -> Option<String> {
    let prefix = format!("--{name}=");
    args.iter()
        .find_map(|a| a.strip_prefix(prefix.as_str()).map(str::to_string))
}

fn has_flag(args: &[String], name: &str) -> bool {
    let flag = format!("--{name}");
    args.iter().any(|a| *a == flag)
}

/// Argumen posisi, yaitu yang tidak diawali `--`.
fn positional(args: &[String]) -> Vec<&String> {
    args.iter().filter(|a| !a.starts_with("--")).collect()
}

/// Setara `php artisan app:sync-route-permissions`. `--clean` menghapus izin, jadi perlu `--yes`.
async fn run_sync_route_permissions(args: &[String]) {
    let prefix = flag_value(args, "prefix").unwrap_or_else(|| "api".to_string());
    let role = flag_value(args, "role").unwrap_or_else(|| "admin".to_string());
    let clean = has_flag(args, "clean");
    if clean && !has_flag(args, "yes") {
        eprintln!(
            "--clean menghapus izin rute yang sudah tidak ada. Tambahkan --yes untuk menjalankan."
        );
        std::process::exit(2);
    }

    let pool = connect_db().await;
    println!("Scanning routes with prefix: {prefix}...");
    match api::route_permissions_sync::sync_cli(&pool, &prefix, &role, clean).await {
        Ok(sum) => {
            if clean && sum.removed > 0 {
                println!("Removed {} stale route permission(s).", sum.removed);
            }
            println!(
                "Done! Scanned {} routes. Created {} new entries.",
                sum.scanned, sum.created
            );
        }
        Err(e) => fail(&e.message),
    }
}

/// Setara `php artisan desa:import-population file [--dry-run]`. Menimpa `jumlah_penduduk`, jadi
/// tanpa `--dry-run` perlu `--yes`.
async fn run_desa_import_population(args: &[String]) {
    let files = positional(args);
    let Some(file) = files.first() else {
        fail("Pakai: desa-import-population <file.xlsx> [--dry-run] [--yes]");
    };
    let dry_run = has_flag(args, "dry-run");
    if !dry_run && !has_flag(args, "yes") {
        eprintln!(
            "Import ini menimpa jumlah_penduduk. Tambahkan --yes, atau --dry-run untuk cek saja."
        );
        std::process::exit(2);
    }
    let path = std::path::Path::new(file.as_str());
    if !path.is_file() {
        fail(&format!("File tidak ditemukan: {file}"));
    }

    let pool = connect_db().await;
    let rep = match api::desa_population::import(&pool, path, dry_run).await {
        Ok(r) => r,
        Err(e) => fail(&e),
    };
    println!("Import penduduk selesai.");
    println!("Rows: {}", rep.rows);
    println!("Matched unik: {}", rep.matched);
    println!("Updated: {}", rep.updated);
    println!("Unmatched: {}", rep.unmatched.len());
    println!("Ambiguous: {}", rep.ambiguous.len());
    for r in rep.unmatched.iter().take(20) {
        println!(
            "UNMATCHED {} / {} = {}",
            r.kecamatan, r.desa, r.jumlah_penduduk
        );
    }
    for r in rep.ambiguous.iter().take(20) {
        println!(
            "AMBIGUOUS {} / {} = {}",
            r.kecamatan, r.desa, r.jumlah_penduduk
        );
    }
}

/// Setara `php artisan blog-assets:cleanup-orphans --hours=24`. Menghapus data, jadi perlu `--yes`.
async fn run_blog_assets_cleanup_orphans(args: &[String]) {
    let hours: i64 = match flag_value(args, "hours") {
        None => 24,
        Some(v) => match v.parse::<i64>() {
            Ok(h) if h >= 0 => h,
            _ => fail("--hours harus bilangan bulat tidak negatif"),
        },
    };
    if !has_flag(args, "yes") {
        eprintln!("Command ini menghapus aset blog yatim beserta berkas medianya. Tambahkan --yes untuk menjalankan.");
        std::process::exit(2);
    }

    let pool = connect_db().await;
    match api::blog_write::cleanup_orphan_assets(&pool, hours).await {
        Ok(n) => println!("Deleted {n} orphan blog assets."),
        Err(e) => fail(&e.message),
    }
}

/// Membuat ulang thumbnail foto yang hilang di disk (koleksi `foto/pekerjaan`).
/// Aman dijalankan ulang: hanya menulis thumbnail yang belum ada, berkas asli tidak diubah.
/// `--dry-run` hanya menghitung.
async fn run_regenerate_thumbs(args: &[String]) {
    let dry_run = has_flag(args, "dry-run");
    let pool = connect_db().await;
    match api::media::regenerate_missing_thumbs(&pool, "App\\Models\\Foto", "foto/pekerjaan", dry_run).await {
        Ok(r) => println!(
            "{}: diperiksa {}, sudah ada {}, {} {}, berkas asli hilang {}, gagal {}",
            if dry_run { "DRY-RUN" } else { "SELESAI" },
            r.checked,
            r.present,
            if dry_run { "akan dibuat" } else { "dibuat" },
            r.created,
            r.missing_original,
            r.failed,
        ),
        Err(e) => fail(&e.to_string()),
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown");
}
