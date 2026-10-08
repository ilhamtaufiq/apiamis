//! Uji integrasi keputusan route permission terhadap tabel Spatie dan `route_permissions`.
//!
//! Butuh `fixtures/permission_schema.sql` dan satu user di tabel `users`:
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p auth --test permission_db -- --ignored
//! ```
//!
//! Test memakai role dan rule dengan nama khusus (`uji_*`) dan menghapusnya di akhir.

use auth::permission::{active_rules, decide, user_role_names, Decision};
use sqlx::{MySqlPool, Row};

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

async fn cleanup(pool: &MySqlPool, user_id: u64) {
    sqlx::query("DELETE FROM model_has_roles WHERE model_type = 'App\\\\Models\\\\User' AND model_id = ? AND role_id IN (SELECT id FROM roles WHERE name LIKE 'uji_%')")
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM roles WHERE name LIKE 'uji_%'")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM route_permissions WHERE description = 'uji'")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn role_rule_and_decision_read_from_database() {
    let pool = pool().await;
    let user_id: u64 = sqlx::query("SELECT id FROM users ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("id")
        .unwrap();
    cleanup(&pool, user_id).await;

    sqlx::query("INSERT INTO roles (name, guard_name, created_at, updated_at) VALUES ('uji_pengawas', 'web', NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();
    let role_id: u64 = sqlx::query("SELECT id FROM roles WHERE name = 'uji_pengawas'")
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("id")
        .unwrap();
    sqlx::query("INSERT INTO model_has_roles (role_id, model_type, model_id) VALUES (?, 'App\\\\Models\\\\User', ?)")
        .bind(role_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO route_permissions (route_path, route_method, description, allowed_roles, is_active, created_at, updated_at) VALUES ('/uji-pola/:id', 'GET', 'uji', '[\"uji_pengawas\"]', 1, NOW(), NOW())")
        .execute(&pool)
        .await
        .unwrap();

    let roles = user_role_names(&pool, user_id).await.unwrap();
    assert!(roles.contains(&"uji_pengawas".to_string()));

    let rules = active_rules(&pool, "GET").await.unwrap();
    let mine: Vec<_> = rules
        .iter()
        .filter(|r| r.route_path == "/uji-pola/:id")
        .cloned()
        .collect();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].allowed_roles, vec!["uji_pengawas".to_string()]);

    assert_eq!(
        decide(false, &roles, "/api/uji-pola/7", "GET", &rules),
        Decision::Allow
    );
    assert_eq!(
        decide(
            false,
            &["tamu".to_string()],
            "/api/uji-pola/7",
            "GET",
            &rules
        ),
        Decision::DenyRule {
            required_roles: vec!["uji_pengawas".to_string()]
        }
    );

    cleanup(&pool, user_id).await;
}
