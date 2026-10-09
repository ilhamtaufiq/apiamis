-- Tabel berita acara (migrasi 2025_12_16_000001_create_berita_acara_table). Belum ada di basis data lokal
-- `apiamis`, sedangkan `GET /api/pekerjaan/document-register` membacanya lewat relasi `beritaAcara`.
-- Tanpa foreign key ke tbl_pekerjaan agar tes bisa membersihkan baris uji dengan urutan sederhana.
CREATE TABLE IF NOT EXISTS `tbl_berita_acara` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `data` json DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_berita_acara_pekerjaan_id_unique` (`pekerjaan_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
