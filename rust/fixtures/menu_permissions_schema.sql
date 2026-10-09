-- Skema tabel menu_permissions (migrasi 2025_12_04_025049_create_menu_permissions_table).
-- Dipakai tes DB untuk menu_permissions. Migrasi berikutnya hanya menambah baris data.
CREATE TABLE IF NOT EXISTS `menu_permissions` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `menu_key` varchar(255) NOT NULL,
  `menu_label` varchar(255) NOT NULL,
  `menu_parent` varchar(255) DEFAULT NULL,
  `allowed_roles` json DEFAULT NULL,
  `is_active` tinyint(1) NOT NULL DEFAULT '1',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `menu_permissions_menu_key_unique` (`menu_key`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
