//! Keputusan akses route, setara `App\Http\Middleware\CheckRoutePermission`.
//!
//! Urutan keputusan (sama dengan Laravel):
//! 1. Admin selalu boleh.
//! 2. Path di whitelist boleh.
//! 3. Ada rule aktif untuk path + method (exact, lalu pola `:param`): cek `allowed_roles`.
//! 4. Tanpa rule: route admin-only ditolak.
//! 5. Tanpa rule: method mutasi ditolak kecuali prefix path ada di daftar resource.
//! 6. Selain itu boleh.

use sqlx::{MySqlPool, Row};

/// Path yang selalu boleh (sudah dinormalisasi tanpa prefix `/api`).
pub const WHITELIST: &[&str] = &[
    "/auth/me",
    "/auth/logout",
    "/auth/profile",
    "/auth/avatar",
    "/broadcasting/auth",
    "/menu-permissions/user/menus",
    "/route-permissions/rules",
    "/route-permissions/user/accessible",
    "/dashboard/stats",
    "/app-settings",
];

/// Route yang hanya admin tanpa rule di database.
pub const ADMIN_ONLY_ROUTES: &[&str] = &[
    "/users",
    "/roles",
    "/permissions",
    "/route-permissions",
    "/menu-permissions",
];

/// Prefix resource yang boleh bermutasi tanpa rule eksplisit.
pub const MUTATION_RESOURCE_PREFIXES: &[&str] = &[
    "/pekerjaan",
    "/kontrak",
    "/kontrak-addendums",
    "/kegiatan",
    "/penyedia",
    "/desa",
    "/kecamatan",
    "/foto",
    "/progress",
    "/berita-acara",
    "/berkas",
    "/output",
    "/penerima",
    "/draft-pekerjaan",
    "/tiket",
    "/notifications",
    "/tool-pdfs",
    "/user-drive",
    "/blog",
    "/events",
    "/tags",
    "/spam-units",
    "/survey-lokasi",
    "/survey-tugas",
    "/checklist-items",
    "/pekerjaan-checklist",
    "/pengawas",
    "/master-fase-pekerjaan",
    "/signature-library",
    "/koordinat",
    "/client-error-reports",
    "/auth/logout",
    "/auth/handoff",
    "/auth/impersonate",
    "/search",
    "/menu-permissions/check-access",
    "/route-permissions/check-access",
    "/dashboard",
    "/app-settings",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteRule {
    pub id: u64,
    pub route_path: String,
    pub route_method: String,
    pub allowed_roles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Rule ada, tetapi role user tidak termasuk `allowed_roles`.
    DenyRule {
        required_roles: Vec<String>,
    },
    DenyAdminOnly,
    DenyMutationWithoutRule,
}

/// `/api/pekerjaan/3` -> `/pekerjaan/3`, dan memastikan ada `/` di depan.
pub fn normalize_path(raw: &str) -> String {
    let trimmed = raw
        .strip_prefix("/api/")
        .or_else(|| raw.strip_prefix("api/"));
    match trimmed {
        Some(rest) => format!("/{rest}"),
        None if raw.starts_with('/') => raw.to_string(),
        None => format!("/{raw}"),
    }
}

/// Pola `/pekerjaan/:id` cocok dengan `/pekerjaan/397`. Tanpa `:`, harus sama persis.
pub fn matches_pattern(pattern: &str, path: &str) -> bool {
    if !pattern.contains(':') {
        return pattern == path;
    }
    let p: Vec<&str> = pattern.split('/').collect();
    let s: Vec<&str> = path.split('/').collect();
    if p.len() != s.len() {
        return false;
    }
    p.iter().zip(&s).all(|(pp, ss)| {
        if pp.starts_with(':') {
            !ss.is_empty()
        } else {
            pp == ss
        }
    })
}

/// Rule pertama yang cocok: exact dulu, lalu pola. `rules` sudah difilter method dan aktif.
pub fn find_rule<'a>(rules: &'a [RouteRule], path: &str) -> Option<&'a RouteRule> {
    rules
        .iter()
        .find(|r| r.route_path == path)
        .or_else(|| rules.iter().find(|r| matches_pattern(&r.route_path, path)))
}

fn is_admin_only(path: &str) -> bool {
    if ADMIN_ONLY_ROUTES.contains(&path) {
        return true;
    }
    let base = path.split('/').find(|s| !s.is_empty());
    base.is_some_and(|b| ADMIN_ONLY_ROUTES.contains(&format!("/{b}").as_str()))
}

fn is_mutation(method: &str) -> bool {
    matches!(
        method.to_ascii_uppercase().as_str(),
        "POST" | "PUT" | "PATCH" | "DELETE"
    )
}

fn has_mutation_prefix(path: &str) -> bool {
    MUTATION_RESOURCE_PREFIXES
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

/// Keputusan akses. `rules` berisi rule aktif untuk `method` saja.
pub fn decide(
    is_admin: bool,
    user_roles: &[String],
    raw_path: &str,
    method: &str,
    rules: &[RouteRule],
) -> Decision {
    if is_admin {
        return Decision::Allow;
    }
    let path = normalize_path(raw_path);
    if WHITELIST.contains(&path.as_str()) {
        return Decision::Allow;
    }
    if let Some(rule) = find_rule(rules, &path) {
        if rule.allowed_roles.is_empty()
            || rule.allowed_roles.iter().any(|r| user_roles.contains(r))
        {
            return Decision::Allow;
        }
        return Decision::DenyRule {
            required_roles: rule.allowed_roles.clone(),
        };
    }
    if is_admin_only(&path) {
        return Decision::DenyAdminOnly;
    }
    if is_mutation(method) && !has_mutation_prefix(&path) {
        return Decision::DenyMutationWithoutRule;
    }
    Decision::Allow
}

/// Nama role user (tanpa filter guard, seperti `$user->roles` di Laravel), diurutkan.
pub async fn user_role_names(pool: &MySqlPool, user_id: u64) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT r.name FROM model_has_roles m JOIN roles r ON r.id = m.role_id \
         WHERE m.model_type = 'App\\\\Models\\\\User' AND m.model_id = ? ORDER BY r.name",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(|r| r.try_get("name")).collect()
}

/// Rule aktif untuk satu method, urutan `id` seperti query Laravel.
pub async fn active_rules(pool: &MySqlPool, method: &str) -> Result<Vec<RouteRule>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, route_path, route_method, CAST(allowed_roles AS CHAR) AS allowed_roles \
         FROM route_permissions WHERE route_method = ? AND is_active = 1 ORDER BY id",
    )
    .bind(method.to_ascii_uppercase())
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|r| {
            let raw: Option<String> = r.try_get("allowed_roles")?;
            let allowed_roles = raw
                .as_deref()
                .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                .unwrap_or_default();
            Ok(RouteRule {
                id: r.try_get("id")?,
                route_path: r.try_get("route_path")?,
                route_method: r.try_get("route_method")?,
                allowed_roles,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(path: &str, method: &str, roles: &[&str]) -> RouteRule {
        RouteRule {
            id: 1,
            route_path: path.to_string(),
            route_method: method.to_string(),
            allowed_roles: roles.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn roles(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn normalizes_api_prefix() {
        assert_eq!(normalize_path("/api/pekerjaan/3"), "/pekerjaan/3");
        assert_eq!(normalize_path("api/desa"), "/desa");
        assert_eq!(normalize_path("/auth/me"), "/auth/me");
    }

    #[test]
    fn dynamic_pattern_matches_one_segment_only() {
        assert!(matches_pattern("/pekerjaan/:id", "/pekerjaan/397"));
        assert!(!matches_pattern("/pekerjaan/:id", "/pekerjaan/1/x"));
        assert!(!matches_pattern("/pekerjaan/:id", "/pekerjaan/"));
        assert!(!matches_pattern("/pekerjaan", "/pekerjaan/1"));
    }

    #[test]
    fn admin_bypasses_everything() {
        let rules = [rule("/users", "GET", &["x"])];
        assert_eq!(
            decide(true, &roles(&[]), "/api/users", "GET", &rules),
            Decision::Allow
        );
    }

    #[test]
    fn whitelist_is_always_allowed() {
        assert_eq!(
            decide(false, &roles(&[]), "/api/auth/me", "GET", &[]),
            Decision::Allow
        );
    }

    #[test]
    fn rule_checks_user_roles() {
        let rules = [rule("/pekerjaan/:id", "GET", &["pengawas"])];
        assert_eq!(
            decide(
                false,
                &roles(&["pengawas"]),
                "/api/pekerjaan/9",
                "GET",
                &rules
            ),
            Decision::Allow
        );
        assert_eq!(
            decide(false, &roles(&["tamu"]), "/api/pekerjaan/9", "GET", &rules),
            Decision::DenyRule {
                required_roles: roles(&["pengawas"])
            }
        );
    }

    #[test]
    fn rule_with_empty_roles_allows_everyone() {
        let rules = [rule("/desa", "GET", &[])];
        assert_eq!(
            decide(false, &roles(&[]), "/api/desa", "GET", &rules),
            Decision::Allow
        );
    }

    #[test]
    fn admin_only_routes_are_denied_without_rule() {
        assert_eq!(
            decide(false, &roles(&["pengawas"]), "/api/users/5", "GET", &[]),
            Decision::DenyAdminOnly
        );
        assert_eq!(
            decide(false, &roles(&["pengawas"]), "/api/roles", "GET", &[]),
            Decision::DenyAdminOnly
        );
    }

    #[test]
    fn mutation_without_rule_needs_known_prefix() {
        assert_eq!(
            decide(false, &roles(&[]), "/api/pekerjaan", "POST", &[]),
            Decision::Allow
        );
        assert_eq!(
            decide(
                false,
                &roles(&[]),
                "/api/pekerjaan-checklist-x",
                "POST",
                &[]
            ),
            Decision::DenyMutationWithoutRule
        );
        assert_eq!(
            decide(false, &roles(&[]), "/api/gudang", "DELETE", &[]),
            Decision::DenyMutationWithoutRule
        );
        // GET tanpa rule selalu boleh.
        assert_eq!(
            decide(false, &roles(&[]), "/api/gudang", "GET", &[]),
            Decision::Allow
        );
    }

    /// Matriks role × route. Hasil yang diharapkan mengikuti urutan keputusan Laravel.
    #[test]
    fn decision_matrix() {
        let rules = [
            rule("/pekerjaan/:id", "GET", &["pengawas", "pendamping"]),
            rule("/kontrak/:id", "PUT", &["admin_kontrak"]),
        ];
        let cases: &[(&[&str], &str, &str, Decision)] = &[
            (&["admin"], "/api/kontrak/3", "PUT", Decision::Allow),
            (&["pengawas"], "/api/pekerjaan/3", "GET", Decision::Allow),
            (&["pendamping"], "/api/pekerjaan/3", "GET", Decision::Allow),
            (
                &["tamu"],
                "/api/pekerjaan/3",
                "GET",
                Decision::DenyRule {
                    required_roles: roles(&["pengawas", "pendamping"]),
                },
            ),
            (
                &["pengawas"],
                "/api/kontrak/3",
                "PUT",
                Decision::DenyRule {
                    required_roles: roles(&["admin_kontrak"]),
                },
            ),
            (&["pengawas"], "/api/users", "GET", Decision::DenyAdminOnly),
            (&["pengawas"], "/api/desa", "POST", Decision::Allow),
            (
                &["tamu"],
                "/api/gudang",
                "POST",
                Decision::DenyMutationWithoutRule,
            ),
            (&["tamu"], "/api/gudang", "GET", Decision::Allow),
        ];
        for (user_roles, path, method, want) in cases {
            let owned = roles(user_roles);
            let is_admin = owned.iter().any(|r| r == "admin");
            assert_eq!(
                decide(is_admin, &owned, path, method, &rules),
                *want,
                "{user_roles:?} {method} {path}"
            );
        }
    }
}
