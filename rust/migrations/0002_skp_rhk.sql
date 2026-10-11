-- Master RHK SKP per pengguna dan tahun, serta rencana aksi per bulan.
-- Dibuat oleh modul Rust `skp_rhk`, bukan Laravel.
-- `skp_jurnal_kegiatan.rhk` menyimpan nomor RHK (`no`) dari tabel ini.
CREATE TABLE `skp_rhk` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `tahun` smallint unsigned NOT NULL,
  `no` smallint unsigned NOT NULL,
  `rhk` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `target` double NOT NULL DEFAULT '0',
  `satuan` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'Dokumen',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `skp_rhk_user_tahun_no_unique` (`user_id`, `tahun`, `no`),
  CONSTRAINT `skp_rhk_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `skp_rhk_rencana` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `tahun` smallint unsigned NOT NULL,
  `bulan` tinyint unsigned NOT NULL,
  `rhk_no` smallint unsigned NOT NULL,
  `rencana` double NOT NULL DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `skp_rhk_rencana_unique` (`user_id`, `tahun`, `bulan`, `rhk_no`),
  CONSTRAINT `skp_rhk_rencana_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
