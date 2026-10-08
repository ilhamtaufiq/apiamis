-- tbl_spse_sessions dan tbl_procurement_sync_runs
-- (sumber: database/migrations/2026_07_03_150000_create_spse_procurement_tables.php).
-- tbl_procurement_staging_paket ada di kontrak_addendum_procurement_schema.sql.
CREATE TABLE IF NOT EXISTS `tbl_spse_sessions` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `encrypted_cookies` text NOT NULL,
  `lpse_slug` varchar(64) NOT NULL DEFAULT 'cianjurkab',
  `expires_at` timestamp NULL DEFAULT NULL,
  `last_validated_at` timestamp NULL DEFAULT NULL,
  `is_active` tinyint(1) NOT NULL DEFAULT 1,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_spse_sessions_user_id_is_active_index` (`user_id`,`is_active`),
  CONSTRAINT `tbl_spse_sessions_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_procurement_sync_runs` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned DEFAULT NULL,
  `status` varchar(32) NOT NULL DEFAULT 'running',
  `item_count` int unsigned NOT NULL DEFAULT 0,
  `matched_count` int unsigned NOT NULL DEFAULT 0,
  `error_log` text,
  `started_at` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
  `finished_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_procurement_sync_runs_user_id_foreign` (`user_id`),
  CONSTRAINT `tbl_procurement_sync_runs_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
