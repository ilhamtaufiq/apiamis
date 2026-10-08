-- Tabel `tbl_peta_peripaan` (lihat database/migrations/2026_09_02_000001_create_tbl_peta_peripaan_table.php).
-- Tabel ini tidak ada di DB lokal; dibuat sama dengan migrasi.
CREATE TABLE IF NOT EXISTS tbl_peta_peripaan (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  pekerjaan_id BIGINT UNSIGNED NULL DEFAULT NULL,
  nama VARCHAR(255) NOT NULL,
  geojson LONGTEXT NULL,
  uploaded_by BIGINT UNSIGNED NULL DEFAULT NULL,
  created_at TIMESTAMP NULL DEFAULT NULL,
  updated_at TIMESTAMP NULL DEFAULT NULL,
  PRIMARY KEY (id),
  KEY tbl_peta_peripaan_pekerjaan_id_index (pekerjaan_id),
  KEY tbl_peta_peripaan_uploaded_by_index (uploaded_by),
  CONSTRAINT tbl_peta_peripaan_pekerjaan_id_foreign FOREIGN KEY (pekerjaan_id) REFERENCES tbl_pekerjaan (id) ON DELETE SET NULL,
  CONSTRAINT tbl_peta_peripaan_uploaded_by_foreign FOREIGN KEY (uploaded_by) REFERENCES users (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
