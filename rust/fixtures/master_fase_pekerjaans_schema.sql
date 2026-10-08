-- Skema master_fase_pekerjaans dari database/migrations/2026_05_27_154820 dan 2026_07_16_230000.
CREATE TABLE IF NOT EXISTS `master_fase_pekerjaans` (
  `id` bigint(20) unsigned NOT NULL AUTO_INCREMENT,
  `jenis_proyek` varchar(50) NOT NULL,
  `kode_fase` varchar(30) NOT NULL,
  `nama_fase` varchar(100) NOT NULL,
  `prioritas` int(11) NOT NULL,
  `overlap_persen` int(11) NOT NULL DEFAULT 0,
  `durasi_faktor` float NOT NULL DEFAULT 1,
  `keywords` longtext NOT NULL CHECK (json_valid(`keywords`)),
  `deskripsi` text DEFAULT NULL,
  `is_active` tinyint(1) NOT NULL DEFAULT 1,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `master_fase_jenis_kode_unique` (`jenis_proyek`, `kode_fase`),
  KEY `master_fase_pekerjaans_is_active_index` (`is_active`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
