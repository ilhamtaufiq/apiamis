CREATE TABLE IF NOT EXISTS `tbl_kecamatan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `n_kec` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `idx_kecamatan_n_kec` (`n_kec`),
  KEY `idx_kecamatan_name` (`n_kec`),
  FULLTEXT KEY `ft_kecamatan_search` (`n_kec`)
) ENGINE=InnoDB AUTO_INCREMENT=100 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_desa` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `n_desa` varchar(100) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `luas` double DEFAULT NULL,
  `jumlah_penduduk` int DEFAULT NULL,
  `jumlah_kk` int unsigned DEFAULT NULL,
  `target` int NOT NULL DEFAULT '0',
  `bjp_master` int NOT NULL DEFAULT '0',
  `kecamatan_id` int DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `idx_desa_n_desa` (`n_desa`),
  KEY `idx_desa_kecamatan_id` (`kecamatan_id`),
  KEY `idx_desa_name` (`n_desa`),
  FULLTEXT KEY `ft_desa_search` (`n_desa`)
) ENGINE=InnoDB AUTO_INCREMENT=1004 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
