//! Scope `byUserRole()` terhadap MySQL: pengawas hanya pekerjaan assign, role lain juga lewat kegiatan_role.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test access_db -- --ignored
//! ```

use api::access::user_can_access;
use auth::login::roles_of;
use sqlx::MySqlPool;

async fn make_user(pool: &MySqlPool, email: &str, role: &str) -> (u64, Vec<(u64, String)>) {
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (name, email, password, created_at, updated_at) VALUES (?, ?, 'x', NOW(), NOW())")
        .bind(format!("Uji {email}"))
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
    let uid: u64 = sqlx::query_scalar("SELECT id FROM users WHERE email = ?")
        .bind(email)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query("INSERT IGNORE INTO roles (name, guard_name, created_at, updated_at) VALUES (?, 'web', NOW(), NOW())")
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
    let rid: u64 =
        sqlx::query_scalar("SELECT id FROM roles WHERE name = ? AND guard_name = 'web' LIMIT 1")
            .bind(role)
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query("INSERT IGNORE INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(rid)
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    let roles = roles_of(pool, uid).await.unwrap();
    (uid, roles)
}

async fn cleanup(pool: &MySqlPool, email: &str, uid: u64) {
    sqlx::query("DELETE FROM user_pekerjaan WHERE user_id = ?")
        .bind(uid)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM model_has_roles WHERE model_id = ? AND model_type = 'App\\\\Models\\\\User'",
    )
    .bind(uid)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM users WHERE email = ?")
        .bind(email)
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL dan data tbl_pekerjaan"]
async fn scope_matches_laravel_by_role() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = MySqlPool::connect(&url).await.unwrap();
    let pid: u64 = sqlx::query_scalar("SELECT id FROM tbl_pekerjaan ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let kegiatan: i64 = sqlx::query_scalar("SELECT kegiatan_id FROM tbl_pekerjaan WHERE id = ?")
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();

    // Pengawas: tidak boleh tanpa assign, boleh setelah di-assign, walau ada kegiatan_role.
    let pengawas_mail = "uji-scope-pengawas@example.test";
    let (pengawas, roles) = make_user(&pool, pengawas_mail, "pengawas").await;
    sqlx::query("INSERT IGNORE INTO kegiatan_role (kegiatan_id, role_id) SELECT ?, id FROM roles WHERE name = 'pengawas' AND guard_name = 'web' LIMIT 1")
        .bind(kegiatan)
        .execute(&pool)
        .await
        .unwrap();
    assert!(!user_can_access(&pool, pengawas, &roles, pid).await.unwrap());
    sqlx::query("INSERT INTO user_pekerjaan (user_id, pekerjaan_id, created_at, updated_at) VALUES (?, ?, NOW(), NOW())")
        .bind(pengawas)
        .bind(pid)
        .execute(&pool)
        .await
        .unwrap();
    assert!(user_can_access(&pool, pengawas, &roles, pid).await.unwrap());
    assert!(!user_can_access(&pool, pengawas, &roles, 999_999_999)
        .await
        .unwrap());

    // Role biasa: lewat kegiatan_role tanpa assign.
    let user_mail = "uji-scope-user@example.test";
    let (biasa, roles) = make_user(&pool, user_mail, "user").await;
    assert!(!user_can_access(&pool, biasa, &roles, pid).await.unwrap());
    sqlx::query("INSERT IGNORE INTO kegiatan_role (kegiatan_id, role_id) SELECT ?, id FROM roles WHERE name = 'user' AND guard_name = 'web' LIMIT 1")
        .bind(kegiatan)
        .execute(&pool)
        .await
        .unwrap();
    assert!(user_can_access(&pool, biasa, &roles, pid).await.unwrap());
    sqlx::query("DELETE FROM kegiatan_role WHERE kegiatan_id = ? AND role_id IN (SELECT id FROM roles WHERE name = 'user' AND guard_name = 'web')")
        .bind(kegiatan)
        .execute(&pool)
        .await
        .unwrap();

    // Admin: semua pekerjaan yang ada.
    let (admin, roles) = make_user(&pool, "uji-scope-admin@example.test", "admin").await;
    assert!(user_can_access(&pool, admin, &roles, pid).await.unwrap());
    assert!(!user_can_access(&pool, admin, &roles, 999_999_999)
        .await
        .unwrap());

    cleanup(&pool, pengawas_mail, pengawas).await;
    cleanup(&pool, user_mail, biasa).await;
    cleanup(&pool, "uji-scope-admin@example.test", admin).await;
    sqlx::query("DELETE FROM kegiatan_role WHERE kegiatan_id = ? AND role_id IN (SELECT id FROM roles WHERE name = 'pengawas' AND guard_name = 'web')")
        .bind(kegiatan)
        .execute(&pool)
        .await
        .unwrap();
}
