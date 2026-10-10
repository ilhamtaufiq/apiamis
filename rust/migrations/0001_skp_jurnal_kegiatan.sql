-- Jurnal kegiatan SKP (harian, per pengguna). Dibuat oleh modul Rust `skp_jurnal`, bukan Laravel.
-- `rhk` adalah nomor RHK (1..99) di SKP pengguna; bisa kosong bila kegiatan tidak terkait RHK.
-- `output` memakai DOUBLE agar sesuai dengan kolom angka di generator SKP (satuan bebas).
CREATE TABLE `skp_jurnal_kegiatan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `tanggal` date NOT NULL,
  `rhk` smallint unsigned DEFAULT NULL,
  `kegiatan` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `output` double DEFAULT NULL,
  `satuan` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `keterangan` varchar(500) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `skp_jurnal_user_tanggal_index` (`user_id`, `tanggal`),
  CONSTRAINT `skp_jurnal_kegiatan_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
