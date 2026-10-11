-- Data kepegawaian untuk laporan SKP: unit kerja dan pangkat/golongan di tabel users.
-- Pejabat penilai per pengguna disimpan di skp_atasan (dibuat oleh modul Rust `skp_atasan`).
ALTER TABLE `users`
  ADD COLUMN `unit_kerja` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL AFTER `jabatan`,
  ADD COLUMN `pangkat_golongan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL AFTER `unit_kerja`;

CREATE TABLE `skp_atasan` (
  `user_id` bigint unsigned NOT NULL,
  `nama` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nip` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `pangkat` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `jabatan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `unit` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`user_id`),
  CONSTRAINT `skp_atasan_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
