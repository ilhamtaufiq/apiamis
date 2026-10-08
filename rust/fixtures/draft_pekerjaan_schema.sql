-- Skema tbl_draft_pekerjaan (sumber: docs/schema.md; migrasi create_draft_pekerjaan_table dan penambahan kolom).
CREATE TABLE IF NOT EXISTS `tbl_draft_pekerjaan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `penyedia_id` bigint unsigned DEFAULT NULL,
  `kode_rup` varchar(255) DEFAULT NULL,
  `kode_paket` varchar(255) DEFAULT NULL,
  `nama_pelaksana` varchar(255) DEFAULT NULL,
  `nama_penyedia` varchar(255) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_draft_pekerjaan_pekerjaan_id_foreign` (`pekerjaan_id`),
  KEY `tbl_draft_pekerjaan_penyedia_id_foreign` (`penyedia_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
