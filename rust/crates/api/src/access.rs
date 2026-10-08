//! Port `Pekerjaan::scopeByUserRole()` dan `Pekerjaan::userCanAccess()`.
//!
//! Urutan aturan mengikuti Laravel:
//! 1. `admin`, `manager`, `super-admin`, `operator`: semua pekerjaan.
//! 2. `pengawas`, `konsultan_pengawas`, `tfl`: hanya pekerjaan di `user_pekerjaan`.
//!    Header konteks app lapangan tidak mengubah hasil, karena kedua cabang Laravel
//!    membatasi dengan cara yang sama.
//! 3. Role lain: `user_pekerjaan` ATAU `kegiatan_id` yang ada di `kegiatan_role`
//!    untuk role user. Tanpa role, hanya `user_pekerjaan`.

use sqlx::MySqlPool;

use crate::{pekerjaan::has_full_access, pekerjaan_rel::role_ids};

/// Role yang hanya melihat pekerjaan hasil assign (`user_pekerjaan`).
pub const PENGAWAS_ROLES: &[&str] = &["pengawas", "konsultan_pengawas", "tfl"];

/// Klausa tambahan untuk query di `tbl_pekerjaan` beserta parameter `u64` berurutan.
/// `sql` kosong berarti tanpa pembatasan (akses penuh).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restriction {
    pub sql: String,
    pub binds: Vec<u64>,
}

impl Restriction {
    /// Tanpa pembatasan (akses penuh atau tidak perlu scope).
    pub fn none() -> Self {
        Self {
            sql: String::new(),
            binds: Vec::new(),
        }
    }
}

/// Port `scopeByUserRole()` sebagai klausa SQL. `alias` adalah alias tabel `tbl_pekerjaan` di query pemanggil.
pub fn restriction(user_id: u64, roles: &[(u64, String)], alias: &str) -> Restriction {
    let names: Vec<String> = roles.iter().map(|(_, n)| n.clone()).collect();
    if has_full_access(&names) {
        return Restriction::none();
    }

    let assigned =
        format!(" AND {alias}.id IN (SELECT pekerjaan_id FROM user_pekerjaan WHERE user_id = ?)");
    let assigned_only = || Restriction {
        sql: assigned.clone(),
        binds: vec![user_id],
    };
    if names.iter().any(|n| PENGAWAS_ROLES.contains(&n.as_str())) {
        return assigned_only();
    }

    let ids = role_ids(roles);
    if ids.is_empty() {
        return assigned_only();
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let mut binds = vec![user_id];
    binds.extend(ids);
    Restriction {
        sql: format!(
            " AND ({alias}.id IN (SELECT pekerjaan_id FROM user_pekerjaan WHERE user_id = ?) \
             OR {alias}.kegiatan_id IN (SELECT kegiatan_id FROM kegiatan_role WHERE role_id IN ({placeholders})))"
        ),
        binds,
    }
}

/// Port `userCanAccess()`: pekerjaan ada dan lolos `scopeByUserRole()`.
pub async fn user_can_access(
    pool: &MySqlPool,
    user_id: u64,
    roles: &[(u64, String)],
    pekerjaan_id: u64,
) -> Result<bool, sqlx::Error> {
    let r = restriction(user_id, roles, "tbl_pekerjaan");
    let sql = format!(
        "SELECT COUNT(*) FROM tbl_pekerjaan WHERE tbl_pekerjaan.id = ?{}",
        r.sql
    );
    let mut q = sqlx::query_scalar::<_, i64>(&sql).bind(pekerjaan_id);
    for b in r.binds {
        q = q.bind(b);
    }
    Ok(q.fetch_one(pool).await? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(list: &[(u64, &str)]) -> Vec<(u64, String)> {
        list.iter().map(|(i, n)| (*i, n.to_string())).collect()
    }

    #[test]
    fn full_access_roles_have_no_restriction() {
        for name in ["admin", "manager", "super-admin", "operator"] {
            let r = restriction(7, &roles(&[(1, name), (9, "pengawas")]), "p");
            assert!(r.sql.is_empty(), "{name} harus tanpa pembatasan");
            assert!(r.binds.is_empty());
        }
    }

    #[test]
    fn pengawas_is_limited_to_assigned_even_with_other_roles() {
        let r = restriction(7, &roles(&[(3, "user"), (4, "tfl")]), "p");
        assert_eq!(r.binds, vec![7]);
        assert!(!r.sql.contains("kegiatan_role"));
    }

    #[test]
    fn other_roles_get_assigned_or_sectoral() {
        let r = restriction(7, &roles(&[(5, "x"), (3, "user"), (5, "x")]), "p");
        assert_eq!(r.binds, vec![7, 3, 5], "id role unik dan terurut");
        assert!(r.sql.contains("kegiatan_role WHERE role_id IN (?,?)"));
        assert!(r.sql.contains("user_pekerjaan"));
        assert!(r.sql.contains("p.kegiatan_id"));
    }

    #[test]
    fn user_without_roles_is_limited_to_assigned() {
        let r = restriction(7, &[], "p");
        assert_eq!(
            r.sql,
            " AND p.id IN (SELECT pekerjaan_id FROM user_pekerjaan WHERE user_id = ?)"
        );
        assert_eq!(r.binds, vec![7]);
    }
}
