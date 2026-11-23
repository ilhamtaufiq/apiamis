-- Posting lapangan: cerita dan foto dari lapangan, terpisah dari foto pekerjaan (tbl_foto).
-- Foto disimpan sebagai media dengan model App\Models\LapanganPost (koleksi lapangan/post).
CREATE TABLE tbl_lapangan_post (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  pekerjaan_id BIGINT UNSIGNED NOT NULL,
  user_id BIGINT UNSIGNED NULL,
  caption VARCHAR(500) NULL,
  koordinat VARCHAR(64) NULL,
  created_at TIMESTAMP NULL DEFAULT NULL,
  updated_at TIMESTAMP NULL DEFAULT NULL,
  PRIMARY KEY (id),
  KEY tbl_lapangan_post_pekerjaan_id_index (pekerjaan_id),
  KEY tbl_lapangan_post_user_id_index (user_id),
  KEY tbl_lapangan_post_created_at_index (created_at),
  CONSTRAINT tbl_lapangan_post_pekerjaan_id_foreign FOREIGN KEY (pekerjaan_id) REFERENCES tbl_pekerjaan (id) ON DELETE CASCADE,
  CONSTRAINT tbl_lapangan_post_user_id_foreign FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
