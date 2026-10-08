-- Skema error_logs (sumber: database/migrations/2026_05_21_000001_create_error_logs_table.php).
-- Tabel ini tidak ada di dump struktur (docs/schema.md) dan dibuat untuk POST /api/client-error-reports.
CREATE TABLE IF NOT EXISTS `error_logs` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned DEFAULT NULL,
  `source` varchar(50) NOT NULL,
  `message` text NOT NULL,
  `stack` longtext,
  `component_stack` longtext,
  `url` text,
  `user_agent` text,
  `ip_address` varchar(45) DEFAULT NULL,
  `metadata` json DEFAULT NULL,
  `resolved_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `error_logs_source_created_at_index` (`source`,`created_at`),
  KEY `error_logs_user_id_created_at_index` (`user_id`,`created_at`),
  KEY `error_logs_resolved_at_index` (`resolved_at`),
  CONSTRAINT `error_logs_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
