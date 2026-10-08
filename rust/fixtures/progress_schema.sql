CREATE TABLE IF NOT EXISTS `tbl_progress` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `content` json DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_progress_pekerjaan_id_foreign` (`pekerjaan_id`),
  CONSTRAINT `tbl_progress_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=165 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `pekerjaan_progress_estimasi_history` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `tahun_anggaran` smallint unsigned NOT NULL,
  `jenis` enum('fisik','keuangan') CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `tipe` enum('rencana','realisasi') CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `tanggal` date NOT NULL,
  `persen` decimal(5,2) NOT NULL,
  `nilai` decimal(18,2) DEFAULT NULL,
  `nomor_sp2d` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `tanggal_pembuatan` date DEFAULT NULL,
  `tanggal_pencairan` date DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `ppe_history_lookup_idx` (`pekerjaan_id`,`tahun_anggaran`,`jenis`,`tipe`),
  KEY `pekerjaan_progress_estimasi_history_tanggal_index` (`tanggal`),
  CONSTRAINT `pekerjaan_progress_estimasi_history_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=2101 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
