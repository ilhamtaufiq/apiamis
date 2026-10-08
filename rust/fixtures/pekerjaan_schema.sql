CREATE TABLE IF NOT EXISTS `pengawas` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `nama` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `nip` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `jabatan` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `telepon` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=30 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `pekerjaan_tag` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `tag_id` bigint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `pekerjaan_tag_pekerjaan_id_tag_id_unique` (`pekerjaan_id`,`tag_id`),
  KEY `pekerjaan_tag_tag_id_foreign` (`tag_id`),
  CONSTRAINT `pekerjaan_tag_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE,
  CONSTRAINT `pekerjaan_tag_tag_id_foreign` FOREIGN KEY (`tag_id`) REFERENCES `tbl_tags` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=69 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
