-- Skema tbl_events (sumber: database/migrations/2025_12_26_080222_create_events_table.php,
-- ditambah kolom attachments dari 2026_05_05_141538_add_attachments_to_tbl_events_table.php).
-- Tabel ini belum ada di basis data lokal. Dibuat dengan IF NOT EXISTS, tidak mengubah tabel lain.
--
-- Pemakaian:
--   mysql -uroot --socket=/run/mysqld/mysqld.sock apiamis < rust/fixtures/events_schema.sql

CREATE TABLE IF NOT EXISTS `tbl_events` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `title` varchar(255) NOT NULL,
  `is_allday` tinyint(1) NOT NULL DEFAULT '0',
  `start` datetime NOT NULL,
  `end` datetime NOT NULL,
  `category` varchar(255) NOT NULL DEFAULT 'event',
  `location` varchar(255) DEFAULT NULL,
  `description` text,
  `color` varchar(255) DEFAULT NULL,
  `bg_color` varchar(255) DEFAULT NULL,
  `border_color` varchar(255) DEFAULT NULL,
  `attachments` json DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_events_user_id_foreign` (`user_id`),
  CONSTRAINT `tbl_events_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
