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

pub const OUTPUT: Target = Target {
    model_type: "App\\Models\\Output",
    label: "Output",
    tab: "output",
};

pub const KONTRAK: Target = Target {
    model_type: "App\\Models\\Kontrak",
    label: "Kontrak",
    tab: "kontrak",
};

pub const BERKAS: Target = Target {
    model_type: "App\\Models\\Berkas",
    label: "Berkas",
    tab: "berkas",
};

pub const DESA: Target = Target {
    model_type: "App\\Models\\Desa",
    label: "Desa",
    tab: "",
};

pub const KECAMATAN: Target = Target {
    model_type: "App\\Models\\Kecamatan",
    label: "Kecamatan",
    tab: "",
};

pub const PENYEDIA: Target = Target {
    model_type: "App\\Models\\Penyedia",
    label: "Penyedia",
    tab: "",
};

pub const KEGIATAN: Target = Target {
    model_type: "App\\Models\\Kegiatan",
    label: "Kegiatan",
    tab: "",
};

/// Tulis baris audit (`event`: created, updated, deleted) dan notifikasi ke admin lain.
/// Tautan notifikasi ke detail pekerjaan, sesuai `NotifiesAdminsOnChanges` untuk pekerjaan.
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
    let link = pekerjaan_id.map(|p| format!("/pekerjaan/{p}?tab={}", target.tab));
    log_linked(tx, headers, actor, target, event, id, old, new, link, url).await
}

/// Seperti `log`, dengan tautan notifikasi yang ditentukan pemanggil (mis. `/kecamatan/{id}/edit`).
#[allow(clippy::too_many_arguments)]
pub async fn log_linked(
    tx: &mut sqlx::Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    target: &Target,
    event: &str,
    id: i64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    link: Option<String>,
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

/// Audit saja, tanpa notifikasi admin (model yang hanya memakai `Auditable`).
#[allow(clippy::too_many_arguments)]
pub async fn audit_only(
    tx: &mut sqlx::Transaction<'_, MySql>,
    headers: &HeaderMap,
    actor: u64,
    model_type: &str,
    event: &str,
    id: i64,
    old: Option<Map<String, Value>>,
    new: Option<Map<String, Value>>,
    url: &str,
) -> Result<(), ApiError> {
    audit::write(
        tx,
        audit::Entry {
            actor,
            event,
            auditable_type: model_type,
            auditable_id: id as u64,
            old,
            new,
            url,
        },
        headers,
    )
    .await
    .map_err(internal)
}
