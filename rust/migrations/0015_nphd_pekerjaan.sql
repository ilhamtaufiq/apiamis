-- Data NPHD per paket pekerjaan: jenis pengelola, daftar penerima manfaat, dan nomor NPHD terakhir.
-- Diisi dari halaman NPHD agar form pembuatan naskah terisi otomatis. Baris hilang bila paket dihapus.
CREATE TABLE IF NOT EXISTS `nphd_pekerjaan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `jenis_pengelola` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NULL,
  `penerima_manfaat` json NULL,
  `nomor_nphd` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `nphd_pekerjaan_pekerjaan_id_unique` (`pekerjaan_id`),
  CONSTRAINT `nphd_pekerjaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
