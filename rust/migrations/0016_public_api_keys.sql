-- Kunci API untuk layanan lain (mis. arsgis) yang membaca data lewat /api/public/v1/*.
-- Hanya hash SHA-256 yang disimpan. Kunci asli tampil sekali saat dibuat: apiamis-api create-api-key.
CREATE TABLE IF NOT EXISTS api_keys (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  name VARCHAR(100) NOT NULL,
  key_prefix VARCHAR(24) NOT NULL,
  key_hash CHAR(64) NOT NULL,
  scopes JSON NOT NULL,
  expires_at TIMESTAMP NULL DEFAULT NULL,
  revoked_at TIMESTAMP NULL DEFAULT NULL,
  last_used_at TIMESTAMP NULL DEFAULT NULL,
  created_at TIMESTAMP NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (id),
  UNIQUE KEY api_keys_key_hash_unique (key_hash)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
