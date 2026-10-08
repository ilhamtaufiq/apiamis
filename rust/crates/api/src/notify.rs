//! Notifikasi database ke admin, setara `NotifiesAdminsOnChanges::notifyAdmins` dan `AppNotification`.
//! Baris ditulis ke `notifications` dengan `data` JSON yang sama bentuknya dengan `toArray()`.

use serde_json::json;
use sqlx::{MySql, Transaction};

/// UUID v4 untuk `notifications.id` dan `media.uuid`.
pub fn new_uuid() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Nama pelaku, atau `System` bila user tidak ada (sama dengan Laravel).
pub async fn actor_name(
    tx: &mut Transaction<'_, MySql>,
    actor: u64,
) -> Result<String, sqlx::Error> {
    let name: Option<String> = sqlx::query_scalar("SELECT name FROM users WHERE id = ?")
        .bind(actor)
        .fetch_optional(&mut **tx)
        .await?
        .flatten();
    Ok(name.unwrap_or_else(|| "System".to_string()))
}

/// Pesan notifikasi perubahan. Kalimat suffix hanya bila model punya URL.
pub fn change_message(model: &str, id: u64, action: &str, actor: &str, has_url: bool) -> String {
    let mut message = format!("Model {model} dengan ID #{id} telah {action} oleh {actor}.");
    if has_url {
        message.push_str(" Klik untuk membuka detail perubahan.");
    }
    message
}

/// Kirim notifikasi ke semua user ber-role `admin`, kecuali pelaku.
pub async fn admins(
    tx: &mut Transaction<'_, MySql>,
    actor: u64,
    title: &str,
    message: &str,
    url: Option<&str>,
) -> Result<(), sqlx::Error> {
    let admins: Vec<u64> = sqlx::query_scalar(
        "SELECT u.id FROM users u \
         JOIN model_has_roles mr ON mr.model_id = u.id AND mr.model_type = 'App\\\\Models\\\\User' \
         JOIN roles r ON r.id = mr.role_id WHERE r.name = 'admin' ORDER BY u.id",
    )
    .fetch_all(&mut **tx)
    .await?;

    let data = json!({
        "title": title,
        "message": message,
        "url": url,
        "type": "info",
        "is_banner": false,
        "broadcast_history_id": null,
    })
    .to_string();

    for admin in admins.into_iter().filter(|a| *a != actor) {
        sqlx::query(
            "INSERT INTO notifications (id, type, notifiable_type, notifiable_id, data, created_at, updated_at) \
             VALUES (?, 'App\\\\Notifications\\\\AppNotification', 'App\\\\Models\\\\User', ?, ?, NOW(), NOW())",
        )
        .bind(new_uuid())
        .bind(admin)
        .bind(&data)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
