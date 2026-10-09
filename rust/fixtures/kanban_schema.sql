-- Skema kanban (sumber: database/migrations/2026_06_26_000001_create_kanban_tables.php).
-- Tabel ini belum ada di basis data lokal. Dibuat dengan IF NOT EXISTS, tidak mengubah tabel lain.
-- Seed papan `organisasi` dan tiga kolom (Baru, Proses, Selesai) ikut migrasi. Seed dijaga agar
-- tidak dobel saat file dijalankan ulang.
--
-- Pemakaian:
--   mysql -uroot --socket=/run/mysqld/mysqld.sock apiamis < rust/fixtures/kanban_schema.sql

CREATE TABLE IF NOT EXISTS `tbl_kanban_boards` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `slug` varchar(255) NOT NULL,
  `title` varchar(255) NOT NULL,
  `description` text,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_kanban_boards_slug_unique` (`slug`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_kanban_columns` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `board_id` bigint unsigned NOT NULL,
  `title` varchar(255) NOT NULL,
  `position` int unsigned NOT NULL DEFAULT '0',
  `tiket_status` enum('open','pending','closed') DEFAULT NULL,
  `color` varchar(7) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_kanban_columns_board_id_foreign` (`board_id`),
  CONSTRAINT `tbl_kanban_columns_board_id_foreign` FOREIGN KEY (`board_id`) REFERENCES `tbl_kanban_boards` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_kanban_cards` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `board_id` bigint unsigned NOT NULL,
  `column_id` bigint unsigned NOT NULL,
  `position` int unsigned NOT NULL DEFAULT '0',
  `title` varchar(255) NOT NULL,
  `description` text,
  `status_label` varchar(255) DEFAULT NULL,
  `pekerjaan_id` bigint unsigned DEFAULT NULL,
  `tiket_id` bigint unsigned DEFAULT NULL,
  `source` enum('manual','tiket') NOT NULL DEFAULT 'manual',
  `metadata` json DEFAULT NULL,
  `created_by` bigint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_kanban_cards_board_id_tiket_id_unique` (`board_id`, `tiket_id`),
  KEY `tbl_kanban_cards_column_id_foreign` (`column_id`),
  KEY `tbl_kanban_cards_pekerjaan_id_foreign` (`pekerjaan_id`),
  KEY `tbl_kanban_cards_tiket_id_foreign` (`tiket_id`),
  KEY `tbl_kanban_cards_created_by_foreign` (`created_by`),
  CONSTRAINT `tbl_kanban_cards_board_id_foreign` FOREIGN KEY (`board_id`) REFERENCES `tbl_kanban_boards` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_kanban_cards_column_id_foreign` FOREIGN KEY (`column_id`) REFERENCES `tbl_kanban_columns` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_kanban_cards_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_kanban_cards_tiket_id_foreign` FOREIGN KEY (`tiket_id`) REFERENCES `tbl_tiket` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_kanban_cards_created_by_foreign` FOREIGN KEY (`created_by`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- Seed papan dan kolom dari migrasi, hanya bila belum ada.
INSERT INTO `tbl_kanban_boards` (`slug`, `title`, `description`, `created_at`, `updated_at`)
SELECT 'organisasi', 'Kanban Organisasi', 'Papan kerja bersama organisasi ARUMANIS', NOW(), NOW()
FROM DUAL
WHERE NOT EXISTS (SELECT 1 FROM `tbl_kanban_boards` WHERE `slug` = 'organisasi');

INSERT INTO `tbl_kanban_columns` (`board_id`, `title`, `position`, `tiket_status`, `color`, `created_at`, `updated_at`)
SELECT b.`id`, 'Baru', 0, 'open', '#3b82f6', NOW(), NOW()
FROM `tbl_kanban_boards` b
WHERE b.`slug` = 'organisasi'
  AND NOT EXISTS (SELECT 1 FROM `tbl_kanban_columns` c WHERE c.`board_id` = b.`id` AND c.`title` = 'Baru');

INSERT INTO `tbl_kanban_columns` (`board_id`, `title`, `position`, `tiket_status`, `color`, `created_at`, `updated_at`)
SELECT b.`id`, 'Proses', 1, 'pending', '#f59e0b', NOW(), NOW()
FROM `tbl_kanban_boards` b
WHERE b.`slug` = 'organisasi'
  AND NOT EXISTS (SELECT 1 FROM `tbl_kanban_columns` c WHERE c.`board_id` = b.`id` AND c.`title` = 'Proses');

INSERT INTO `tbl_kanban_columns` (`board_id`, `title`, `position`, `tiket_status`, `color`, `created_at`, `updated_at`)
SELECT b.`id`, 'Selesai', 2, 'closed', '#22c55e', NOW(), NOW()
FROM `tbl_kanban_boards` b
WHERE b.`slug` = 'organisasi'
  AND NOT EXISTS (SELECT 1 FROM `tbl_kanban_columns` c WHERE c.`board_id` = b.`id` AND c.`title` = 'Selesai');
