//! Migrator schema: `apiamis-api migrate`.
//!
//! Format tabel `migrations` sama dengan Laravel: `(id, migration, batch)`, sehingga production dan
//! Rust membaca state yang sama.
//!
//! Aturan:
//! - DB kosong (tanpa tabel sama sekali): baseline dijalankan. Baseline membuat tabel `migrations`
//!   sendiri (berisi 139 riwayat Laravel), lalu dicatat sebagai batch 1.
//! - DB yang sudah ada dan punya tabel `migrations` berisi: baseline tidak dijalankan, hanya dicatat
//!   dengan batch 0, karena production sudah punya schema itu.
//! - Migrasi lain di `MIGRATIONS` dijalankan bila belum tercatat, satu batch per pemanggilan.
//! - Pemanggilan dikunci dengan `GET_LOCK`, supaya dua proses tidak menjalankan migrasi bersamaan.
//!
//! Catatan: DDL MySQL/MariaDB tidak bisa di-rollback. Bila satu migrasi gagal di tengah, statement
//! sebelumnya tetap berlaku, dan migrasi itu tidak dicatat sehingga harus diperbaiki secara manual.

use sqlx::{MySqlConnection, MySqlPool};

pub const BASELINE: &str = "0000_baseline";

/// Daftar migrasi, berurutan. Tiap file SQL disisipkan saat build, sehingga binary tidak butuh
/// folder `migrations/` saat berjalan. Tambahkan file baru di sini.
pub const MIGRATIONS: &[(&str, &str)] = &[(BASELINE, include_str!("../../../migrations/0000_baseline.sql"))];

const LOCK_NAME: &str = "apiamis_migrate";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub baseline_executed: bool,
    pub baseline_recorded: bool,
    pub applied: Vec<String>,
    pub batch: Option<i64>,
}

/// Memecah file SQL menjadi statement. Pemisah: baris yang diakhiri `;`. Baris komentar `--` dibuang.
/// Aman untuk file yang dibuat dari dump (tanpa `;` di akhir baris di dalam string).
pub fn split_statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in sql.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("--") {
            continue;
        }
        cur.push_str(line);
        cur.push('\n');
        if t.ends_with(';') {
            push_stmt(&mut out, &cur);
            cur.clear();
        }
    }
    push_stmt(&mut out, &cur);
    out
}

fn push_stmt(out: &mut Vec<String>, raw: &str) {
    let s = raw.trim().trim_end_matches(';').trim();
    if !s.is_empty() {
        out.push(s.to_string());
    }
}

/// Jalankan migrasi terhadap `pool`. Mengembalikan ringkasan apa yang dijalankan.
pub async fn run(pool: &MySqlPool) -> Result<Report, sqlx::Error> {
    let mut conn = pool.acquire().await?;
    let got: Option<i64> = sqlx::query_scalar("SELECT GET_LOCK(?, 30)")
        .bind(LOCK_NAME)
        .fetch_one(&mut *conn)
        .await?;
    if got != Some(1) {
        return Err(protocol("migrasi lain sedang berjalan (GET_LOCK gagal)"));
    }
    let result = apply(&mut conn).await;
    let _ = sqlx::query("SELECT RELEASE_LOCK(?)")
        .bind(LOCK_NAME)
        .execute(&mut *conn)
        .await;
    result
}

async fn apply(conn: &mut MySqlConnection) -> Result<Report, sqlx::Error> {
    let mut report = Report::default();
    let has_table: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = 'migrations'",
    )
    .fetch_one(&mut *conn)
    .await?;

    if has_table == 0 {
        let tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = DATABASE()",
        )
        .fetch_one(&mut *conn)
        .await?;
        if tables > 0 {
            return Err(protocol(
                "database sudah berisi tabel tetapi tidak punya tabel migrations; migrator menolak berjalan",
            ));
        }
        run_statements(conn, baseline_sql()?).await?;
        insert_record(conn, BASELINE, 1).await?;
        report.baseline_executed = true;
        report.baseline_recorded = true;
        report.batch = Some(1);
    } else if !is_recorded(conn, BASELINE).await? {
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM migrations")
            .fetch_one(&mut *conn)
            .await?;
        if rows == 0 {
            return Err(protocol("tabel migrations kosong; baseline tidak bisa dicatat dengan aman"));
        }
        insert_record(conn, BASELINE, 0).await?;
        report.baseline_recorded = true;
    }

    let mut pending: Vec<(&str, &str)> = Vec::new();
    for (name, sql) in MIGRATIONS {
        if *name != BASELINE && !is_recorded(conn, name).await? {
            pending.push((name, sql));
        }
    }
    if pending.is_empty() {
        return Ok(report);
    }

    let batch: i64 = sqlx::query_scalar("SELECT CAST(COALESCE(MAX(batch), 0) + 1 AS SIGNED) FROM migrations")
        .fetch_one(&mut *conn)
        .await?;
    for (name, sql) in pending {
        run_statements(conn, sql).await?;
        insert_record(conn, name, batch).await?;
        report.applied.push(name.to_string());
    }
    report.batch = Some(batch);
    Ok(report)
}

fn baseline_sql() -> Result<&'static str, sqlx::Error> {
    MIGRATIONS
        .iter()
        .find(|(n, _)| *n == BASELINE)
        .map(|(_, sql)| *sql)
        .ok_or_else(|| protocol("baseline tidak terdaftar"))
}

async fn is_recorded(conn: &mut MySqlConnection, name: &str) -> Result<bool, sqlx::Error> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM migrations WHERE migration = ?")
        .bind(name)
        .fetch_one(&mut *conn)
        .await?;
    Ok(n > 0)
}

async fn insert_record(conn: &mut MySqlConnection, name: &str, batch: i64) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO migrations (migration, batch) VALUES (?, ?)")
        .bind(name)
        .bind(batch)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn run_statements(conn: &mut MySqlConnection, sql: &str) -> Result<(), sqlx::Error> {
    for stmt in split_statements(sql) {
        sqlx::raw_sql(&stmt).execute(&mut *conn).await?;
    }
    Ok(())
}

fn protocol(msg: &str) -> sqlx::Error {
    sqlx::Error::Protocol(msg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_multi_line_statements_and_drops_comments() {
        let sql = "-- komentar\nCREATE TABLE `a` (\n  `id` int\n) ENGINE=InnoDB;\n\nINSERT INTO `a` VALUES\n(1),\n(2);\n";
        let s = split_statements(sql);
        assert_eq!(s.len(), 2);
        assert!(s[0].starts_with("CREATE TABLE `a`"));
        assert!(s[0].ends_with("ENGINE=InnoDB"));
        assert!(s[1].contains("(2)"));
    }

    #[test]
    fn baseline_is_registered_first() {
        assert_eq!(MIGRATIONS[0].0, BASELINE);
        let stmts = split_statements(MIGRATIONS[0].1);
        assert!(stmts.iter().filter(|s| s.starts_with("CREATE TABLE")).count() >= 110);
    }
}
