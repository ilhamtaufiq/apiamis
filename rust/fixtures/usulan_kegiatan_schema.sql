-- tbl_usulan_kegiatan: skema akhir setelah migrasi
-- 2026_07_29_042503 (create), 2026_08_05_000001 (surat fields), 2026_08_10_000001 (not null).
CREATE TABLE IF NOT EXISTS `tbl_usulan_kegiatan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `sub_bidang` enum('air minum','sanitasi') NOT NULL,
  `nama_pengusul` varchar(255) NOT NULL,
  `kecamatan_id` bigint unsigned NOT NULL,
  `desa_id` bigint unsigned NOT NULL,
  `perihal` varchar(255) NOT NULL,
  `ringkasan` text NOT NULL,
  `tanggal_surat_masuk` date NOT NULL,
  `nomor_surat_masuk` varchar(100) NOT NULL,
  `tanggal_surat` date NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_usulan_kegiatan_user_id_foreign` (`user_id`),
  KEY `tbl_usulan_kegiatan_kecamatan_id_foreign` (`kecamatan_id`),
  KEY `tbl_usulan_kegiatan_desa_id_foreign` (`desa_id`),
  CONSTRAINT `tbl_usulan_kegiatan_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_usulan_kegiatan_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`),
  CONSTRAINT `tbl_usulan_kegiatan_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
