-- Skema tbl_berkas (sumber: docs/schema.md, migrasi 2026-07-12 menambah uploaded_by).
CREATE TABLE IF NOT EXISTS `tbl_berkas` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `jenis_dokumen` varchar(255) NOT NULL,
  `uploaded_by` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_berkas_pekerjaan_id_foreign` (`pekerjaan_id`),
  KEY `tbl_berkas_uploaded_by_foreign` (`uploaded_by`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
