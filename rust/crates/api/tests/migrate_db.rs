//! Tes migrator terhadap MariaDB sungguhan. Membuat DB `apiamis_uji_migrasi*` lalu menghapusnya.
//!
//!     DATABASE_URL=mysql://root@localhost/apiamis?socket=/run/mysqld/mysqld.sock \
//!         cargo test -p api --test migrate_db -- --include-ignored

use sqlx::{mysql::MySqlConnectOptions, ConnectOptions, Connection, MySqlConnection, MySqlPool, Row};
use std::str::FromStr;

fn base_options() -> MySqlConnectOptions {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlConnectOptions::from_str(&url).expect("DATABASE_URL tidak valid")
}

async fn admin() -> MySqlConnection {
    MySqlConnection::connect_with(&base_options().database("mysql"))
        .await
        .expect("gagal konek sebagai admin")
}

async fn fresh_db(name: &str) -> MySqlPool {
    let mut a = admin().await;
    sqlx::raw_sql(&format!("DROP DATABASE IF EXISTS `{name}`"))
        .execute(&mut a)
        .await
        .unwrap();
    sqlx::raw_sql(&format!(
        "CREATE DATABASE `{name}` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci"
    ))
    .execute(&mut a)
    .await
    .unwrap();
    MySqlPool::connect_with(base_options().database(name)).await.unwrap()
}

async fn drop_db(name: &str) {
    let mut a = admin().await;
    sqlx::raw_sql(&format!("DROP DATABASE IF EXISTS `{name}`"))
        .execute(&mut a)
        .await
        .unwrap();
}

async fn count(pool: &MySqlPool, sql: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(sql).fetch_one(pool).await.unwrap()
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn fresh_database_gets_baseline_then_is_noop() {
    let name = "apiamis_uji_migrasi";
    let pool = fresh_db(name).await;

    let first = api::migrate::run(&pool).await.unwrap();
    assert!(first.baseline_executed);
    assert!(first.baseline_recorded);
    assert_eq!(first.batch, Some(1));
    assert!(first.applied.is_empty());
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = DATABASE()").await,
        110,
        "110 tabel production, termasuk migrations"
    );
    // 139 riwayat Laravel dari baseline + 1 catatan baseline.
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM migrations").await, 140);

    let second = api::migrate::run(&pool).await.unwrap();
    assert_eq!(second, api::migrate::Report::default());
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM migrations").await, 140);

    pool.close().await;
    drop_db(name).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn existing_production_schema_only_records_baseline() {
    let name = "apiamis_uji_migrasi_prod";
    let pool = fresh_db(name).await;
    // Simulasi production: schema dan riwayat Laravel sudah ada, tanpa catatan baseline.
    let baseline = api::migrate::MIGRATIONS[0].1;
    // Satu koneksi: SET FOREIGN_KEY_CHECKS di baseline hanya berlaku di koneksi yang sama.
    let mut conn = pool.acquire().await.unwrap();
    for stmt in api::migrate::split_statements(baseline) {
        sqlx::raw_sql(&stmt).execute(&mut *conn).await.unwrap();
    }
    drop(conn);
    sqlx::query("DELETE FROM migrations WHERE migration = ?")
        .bind(api::migrate::BASELINE)
        .execute(&pool)
        .await
        .unwrap();
    let before = count(&pool, "SELECT COUNT(*) FROM migrations").await;

    let report = api::migrate::run(&pool).await.unwrap();
    assert!(!report.baseline_executed, "baseline tidak boleh dijalankan ulang di production");
    assert!(report.baseline_recorded);
    assert_eq!(report.batch, None, "tidak ada migrasi yang dijalankan");
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM migrations").await, before + 1);
    let batch: i64 = sqlx::query("SELECT batch FROM migrations WHERE migration = ?")
        .bind(api::migrate::BASELINE)
        .fetch_one(&pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(batch, 0);

    pool.close().await;
    drop_db(name).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn refuses_database_with_tables_but_no_migrations_table() {
    let name = "apiamis_uji_migrasi_tolak";
    let pool = fresh_db(name).await;
    sqlx::raw_sql("CREATE TABLE tabel_liar (id INT)").execute(&pool).await.unwrap();

    let err = api::migrate::run(&pool).await.unwrap_err();
    assert!(err.to_string().contains("migrations"), "{err}");
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = DATABASE()").await,
        1,
        "tidak boleh ada perubahan"
    );

    pool.close().await;
    drop_db(name).await;
}
