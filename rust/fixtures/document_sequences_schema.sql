-- Skema tbl_document_sequences setelah migrasi 2026_02_26_224145 dan 2026_04_26_105100.
-- Kunci unik (year, type); type default 'global'.
CREATE TABLE IF NOT EXISTS `tbl_document_sequences` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `year` int NOT NULL,
  `type` varchar(255) NOT NULL DEFAULT 'global',
  `last_number` int NOT NULL DEFAULT 0,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_document_sequences_year_type_unique` (`year`, `type`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
