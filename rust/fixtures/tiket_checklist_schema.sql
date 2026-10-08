-- Struktur tabel Tiket dan Checklist, dari docs/schema.md (tanpa data, tanpa foreign key).
CREATE TABLE IF NOT EXISTS `tbl_tiket` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `pekerjaan_id` bigint unsigned DEFAULT NULL,
  `subjek` varchar(255) NOT NULL,
  `deskripsi` text NOT NULL,
  `kategori` enum('bug','request','lapangan','document','other') DEFAULT 'other',
  `prioritas` enum('low','medium','high') NOT NULL DEFAULT 'medium',
  `status` enum('open','pending','closed') NOT NULL DEFAULT 'open',
  `admin_notes` text,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_tiket_user_id_foreign` (`user_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_tiket_comment` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `tiket_id` bigint unsigned NOT NULL,
  `user_id` bigint unsigned NOT NULL,
  `message` text NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_tiket_comment_tiket_id_foreign` (`tiket_id`),
  KEY `tbl_tiket_comment_user_id_foreign` (`user_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_checklist_items` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `name` varchar(100) NOT NULL,
  `description` varchar(255) DEFAULT NULL,
  `sort_order` int NOT NULL DEFAULT '0',
  `context` varchar(30) NOT NULL DEFAULT 'pekerjaan',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_checklist_items_context_index` (`context`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `pekerjaan_checklist` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `checklist_item_id` bigint unsigned NOT NULL,
  `is_checked` tinyint(1) NOT NULL DEFAULT '0',
  `checked_at` timestamp NULL DEFAULT NULL,
  `checked_by` bigint unsigned DEFAULT NULL,
  `notes` text,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `pekerjaan_checklist_pekerjaan_id_checklist_item_id_unique` (`pekerjaan_id`,`checklist_item_id`),
  KEY `pekerjaan_checklist_checklist_item_id_foreign` (`checklist_item_id`),
  KEY `pekerjaan_checklist_checked_by_foreign` (`checked_by`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
