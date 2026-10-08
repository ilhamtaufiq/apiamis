//! Audit dan notifikasi admin untuk satu perubahan model, dalam transaksi yang sama.
//! Setara trait `Auditable` dan `NotifiesAdminsOnChanges` di Laravel.

use axum::http::HeaderMap;
use serde_json::{Map, Value};
use shared::ApiError;
use sqlx::MySql;

use crate::{audit, media::internal, notify};

/// Model yang diaudit: nama kelas Laravel, label di pesan, dan tab di URL detail pekerjaan.
pub struct Target {
    pub model_type: &'static str,
    pub label: &'static str,
    pub tab: &'static str,
}

pub const FOTO: Target = Target {
    model_type: "App\\Models\\Foto",
    label: "Foto",
    tab: "foto",
};

pub const BERKAS: Target = Target {
    model_type: "App\\Models\\Berkas",
    label: "Berkas",
    tab: "berkas",
};

/// Tulis baris audit (`event`: created, updated, deleted) dan notifikasi ke admin lain.
#[allow(clippy::too_many_arguments)]
pub async fn log(
    tx: &mut sqlx::Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    target: &Target,
    event: &str,
    id: i64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    pekerjaan_id: Option<i64>,
    url: &str,
) -> Result<(), ApiError> {
    audit::write(
        tx,
        audit::Entry {
            actor,
            event,
            auditable_type: target.model_type,
            auditable_id: id as u64,
            old,
            new,
            url,
        },
        headers,
    )
    .await
    .map_err(internal)?;

    let action = match event {
        "created" => "dibuat",
        "updated" => "diperbarui",
        _ => "dihapus",
    };
    let name = notify::actor_name(tx, actor).await.map_err(internal)?;
    let link = pekerjaan_id.map(|p| format!("/pekerjaan/{p}?tab={}", target.tab));
    let message = notify::change_message(target.label, id as u64, action, &name, link.is_some());
    notify::admins(
        tx,
        actor,
        &format!("Data {} {action}", target.label),
        &message,
        link.as_deref(),
    )
    .await
    .map_err(internal)
}
