-- Tabel `signature_libraries` (lihat database/migrations/2026_06_03_000000_create_signature_libraries_table.php).
-- Tabel ini tidak ada di DB lokal; dibuat sama dengan migrasi.
CREATE TABLE IF NOT EXISTS signature_libraries (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  user_id BIGINT UNSIGNED NOT NULL,
  name VARCHAR(255) NOT NULL,
  mime_type VARCHAR(100) NOT NULL,
  data_url LONGTEXT NOT NULL,
  width INT UNSIGNED NOT NULL,
  height INT UNSIGNED NOT NULL,
  created_at TIMESTAMP NULL DEFAULT NULL,
  updated_at TIMESTAMP NULL DEFAULT NULL,
  deleted_at TIMESTAMP NULL DEFAULT NULL,
  PRIMARY KEY (id),
  KEY signature_libraries_user_id_name_index (user_id, name),
  CONSTRAINT signature_libraries_user_id_foreign FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
