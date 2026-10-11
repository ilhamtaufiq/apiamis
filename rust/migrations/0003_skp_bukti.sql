-- Slot foto bukti per RHK per bulan (setara folder bukti/<BULAN>/rhkN*.jpg di generator Python).
-- Foto disimpan di tabel `media` dengan model `App\Models\SkpBukti`, dengan `model_id` = `id` di sini.
CREATE TABLE `skp_bukti` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `tahun` smallint unsigned NOT NULL,
  `bulan` tinyint unsigned NOT NULL,
  `rhk_no` smallint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `skp_bukti_unique` (`user_id`, `tahun`, `bulan`, `rhk_no`),
  CONSTRAINT `skp_bukti_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
