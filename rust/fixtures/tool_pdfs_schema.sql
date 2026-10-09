-- Skema untuk modul tool PDF (rust/crates/api/src/tool_pdfs.rs).
-- Sumber: database/migrations/2026_06_03_010000_create_tool_pdfs_table.php
--         database/migrations/2026_06_04_000000_create_tool_pdf_signature_placements_table.php

CREATE TABLE IF NOT EXISTS `tool_pdfs` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `parent_id` bigint unsigned DEFAULT NULL,
  `name` varchar(255) NOT NULL,
  `original_filename` varchar(255) DEFAULT NULL,
  `kind` varchar(20) NOT NULL DEFAULT 'source',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  `deleted_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tool_pdfs_user_id_kind_index` (`user_id`,`kind`),
  KEY `tool_pdfs_parent_id_foreign` (`parent_id`),
  CONSTRAINT `tool_pdfs_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tool_pdfs_parent_id_foreign` FOREIGN KEY (`parent_id`) REFERENCES `tool_pdfs` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tool_pdf_signature_placements` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `tool_pdf_id` bigint unsigned NOT NULL,
  `signature_id` char(36) NOT NULL,
  `page_number` int unsigned NOT NULL,
  `x_ratio` decimal(8,6) NOT NULL,
  `y_ratio` decimal(8,6) NOT NULL,
  `scale` decimal(8,6) NOT NULL,
  `sort_order` int unsigned NOT NULL DEFAULT '0',
  `signature_name` varchar(255) NOT NULL,
  `signature_file_name` varchar(255) NOT NULL,
  `signature_mime_type` varchar(100) NOT NULL,
  `signature_width` int unsigned NOT NULL,
  `signature_height` int unsigned NOT NULL,
  `signature_data_url` longtext DEFAULT NULL,
  `signature_source_type` enum('upload','library') DEFAULT NULL,
  `signature_source_id` varchar(64) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tool_pdf_signature_placements_tool_pdf_id_page_number_index` (`tool_pdf_id`,`page_number`),
  CONSTRAINT `tool_pdf_signature_placements_tool_pdf_id_foreign` FOREIGN KEY (`tool_pdf_id`) REFERENCES `tool_pdfs` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
