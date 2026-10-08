-- Skema tabel sk (sumber: docs/schema.md, migrasi 2026_08_17_000001_create_sk_table).
CREATE TABLE IF NOT EXISTS `sk` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `nomor_sk` varchar(255) NOT NULL,
  `nama` varchar(255) NOT NULL,
  `tanggal_sk` date DEFAULT NULL,
  `uploaded_by` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `sk_uploaded_by_foreign` (`uploaded_by`),
  CONSTRAINT `sk_uploaded_by_foreign` FOREIGN KEY (`uploaded_by`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
