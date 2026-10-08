//! `UserResource` untuk relasi `user` pada tiket dan komentar.
//! Bentuknya sama dengan `user_resource` di login, lalu avatar memakai URL disk `public`.

use serde_json::Value;
use sqlx::MySqlPool;

use crate::auth_routes::user_resource;

/// User dengan roles, permissions, dan avatar. `None` bila user tidak ada.
pub async fn resource(
    pool: &MySqlPool,
    app_url: &str,
    user_id: u64,
) -> Result<Option<Value>, sqlx::Error> {
    let Some(user) = auth::login::find_by_id(pool, user_id).await? else {
        return Ok(None);
    };
    let roles = auth::login::roles_of(pool, user_id).await?;
    let permissions = auth::login::permissions_of(pool, user_id).await?;
    let avatar = auth::login::avatar_media(pool, user_id).await?;
    let avatar_url = avatar.and_then(|(media_id, disk, file_name)| {
        (disk == "public").then(|| {
            format!(
                "{}/storage/{media_id}/{file_name}",
                app_url.trim_end_matches('/')
            )
        })
    });
    Ok(Some(user_resource(&user, &roles, &permissions, avatar_url)))
}
