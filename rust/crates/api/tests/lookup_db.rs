//! Tes integrasi lookup (tags dan app-settings) terhadap MySQL.
//!
//! ```bash
//! DATABASE_URL=mysql://user:pass@127.0.0.1:3306/apiamis \
//!     cargo test -p api --test lookup_db -- --ignored
//! ```

use api::lookup::{self, setting_view, tag, tags};
use sqlx::{MySqlPool, Row};

async fn pool() -> MySqlPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    MySqlPool::connect(&url).await.unwrap()
}

async fn cleanup_tags(pool: &MySqlPool) {
    sqlx::query("DELETE FROM tbl_tags WHERE slug LIKE 'uji-%'")
        .execute(pool)
        .await
        .unwrap();
}

async fn cleanup_settings(pool: &MySqlPool) {
    sqlx::query("DELETE FROM app_settings WHERE `key` LIKE 'uji\\_%' OR `key` LIKE 'chat\\_api\\_key\\_uji%' OR `key` = 'mail_password'")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn tags_search_order_and_show() {
    let pool = pool().await;
    cleanup_tags(&pool).await;
    for (name, slug) in [("Uji Zeta", "uji-zeta"), ("Uji Alfa", "uji-alfa")] {
        sqlx::query("INSERT INTO tbl_tags (name, slug, color, created_at, updated_at) VALUES (?, ?, '#112233', NOW(), NOW())")
            .bind(name)
            .bind(slug)
            .execute(&pool)
            .await
            .unwrap();
    }

    let found = tags(&pool, Some("Uji")).await.unwrap();
    let names: Vec<&str> = found
        .iter()
        .map(|t| t.name.as_str())
        .filter(|n| n.starts_with("Uji"))
        .collect();
    assert_eq!(names, vec!["Uji Alfa", "Uji Zeta"], "diurutkan nama");

    let all = tags(&pool, None).await.unwrap();
    assert!(all.len() >= 2);

    let id: u64 = sqlx::query("SELECT id FROM tbl_tags WHERE slug = 'uji-alfa'")
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get("id")
        .unwrap();
    let row = tag(&pool, id).await.unwrap().expect("ada");
    assert_eq!(lookup::tag_resource(&row)["color"], "#112233");
    assert!(tag(&pool, 999_999_999).await.unwrap().is_none());

    cleanup_tags(&pool).await;
}

#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn settings_hide_secrets_and_flag_configured() {
    let pool = pool().await;
    cleanup_settings(&pool).await;
    for (key, value) in [
        ("uji_site_name", "APIAMIS Uji"),
        ("chat_api_key_uji", "sk-rahasia"),
        ("mail_password", ""),
    ] {
        sqlx::query("INSERT INTO app_settings (`key`, `value`, `type`, created_at, updated_at) VALUES (?, ?, 'text', NOW(), NOW())")
            .bind(key)
            .bind(value)
            .execute(&pool)
            .await
            .unwrap();
    }

    let rows = lookup::settings(&pool).await.unwrap();
    let view = |k: &str| {
        let r = rows.iter().find(|r| r.key == k).expect("ada");
        setting_view(&r.key, &r.kind, r.value.as_deref(), None)
    };
    assert_eq!(
        view("uji_site_name"),
        (serde_json::json!("APIAMIS Uji"), false)
    );
    assert_eq!(view("chat_api_key_uji"), (serde_json::Value::Null, true));
    assert_eq!(view("mail_password"), (serde_json::Value::Null, false));

    cleanup_settings(&pool).await;
}
