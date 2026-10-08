-- Pivot Spatie `role_has_permissions` (lihat database/migrations/2025_12_02_151248_create_permission_tables.php).
-- Tabel ini tidak ada di DB lokal; dibuat sama dengan migrasi.
CREATE TABLE IF NOT EXISTS role_has_permissions (
  permission_id BIGINT UNSIGNED NOT NULL,
  role_id BIGINT UNSIGNED NOT NULL,
  PRIMARY KEY (permission_id, role_id),
  KEY role_has_permissions_role_id_foreign (role_id),
  CONSTRAINT role_has_permissions_permission_id_foreign FOREIGN KEY (permission_id) REFERENCES permissions (id) ON DELETE CASCADE,
  CONSTRAINT role_has_permissions_role_id_foreign FOREIGN KEY (role_id) REFERENCES roles (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
