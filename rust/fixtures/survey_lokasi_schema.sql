-- Skema survei lokasi (keadaan akhir setelah migrasi 2026_10_05 sampai 2026_10_07).
-- Dipakai tes DB untuk tbl_survey_lokasi beserta tabel tugas yang direferensikannya.
CREATE TABLE IF NOT EXISTS `tbl_survey_tugas` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned DEFAULT NULL,
  `judul` varchar(255) NOT NULL,
  `tahun_anggaran` int NOT NULL,
  `jenis` enum('spam_perpipaan','spam_pengeboran','mck_individu','mck_komunal') DEFAULT NULL,
  `kecamatan_id` bigint unsigned DEFAULT NULL,
  `desa_id` bigint unsigned DEFAULT NULL,
  `lokasi_catatan` text,
  `assignee_id` bigint unsigned DEFAULT NULL,
  `status` enum('ditugaskan','dikerjakan','selesai') NOT NULL DEFAULT 'ditugaskan',
  `batas_waktu` date DEFAULT NULL,
  `catatan_admin` text,
  `created_by` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_survey_tugas_assignee_id_status_index` (`assignee_id`,`status`),
  KEY `tbl_survey_tugas_tahun_anggaran_index` (`tahun_anggaran`),
  CONSTRAINT `tbl_survey_tugas_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_tugas_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_tugas_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_tugas_assignee_id_foreign` FOREIGN KEY (`assignee_id`) REFERENCES `users` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_tugas_created_by_foreign` FOREIGN KEY (`created_by`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_survey_tugas_assignees` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `survey_tugas_id` bigint unsigned NOT NULL,
  `user_id` bigint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tugas_assignee_unique` (`survey_tugas_id`,`user_id`),
  CONSTRAINT `tbl_survey_tugas_assignees_survey_tugas_id_foreign` FOREIGN KEY (`survey_tugas_id`) REFERENCES `tbl_survey_tugas` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_survey_tugas_assignees_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_survey_lokasi` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `tugas_id` bigint unsigned DEFAULT NULL,
  `jenis` enum('spam_perpipaan','spam_pengeboran','mck_individu','mck_komunal') NOT NULL,
  `nama_lokasi` varchar(255) NOT NULL,
  `kecamatan_id` bigint unsigned DEFAULT NULL,
  `desa_id` bigint unsigned DEFAULT NULL,
  `alamat` text,
  `latitude` decimal(10,7) DEFAULT NULL,
  `longitude` decimal(10,7) DEFAULT NULL,
  `detail` json DEFAULT NULL,
  `status` enum('diajukan','diverifikasi','ditolak') NOT NULL DEFAULT 'diajukan',
  `catatan_verifikasi` text,
  `verified_by` bigint unsigned DEFAULT NULL,
  `verified_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  CONSTRAINT `tbl_survey_lokasi_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_survey_lokasi_tugas_id_foreign` FOREIGN KEY (`tugas_id`) REFERENCES `tbl_survey_tugas` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_lokasi_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_lokasi_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_lokasi_verified_by_foreign` FOREIGN KEY (`verified_by`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
