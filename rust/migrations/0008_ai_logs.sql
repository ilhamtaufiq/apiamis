-- Audit fitur AI (analisa dashboard dan tanya-jawab data). Setiap pemanggilan dicatat, termasuk pertanyaan.
CREATE TABLE IF NOT EXISTS ai_logs (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  user_id BIGINT UNSIGNED DEFAULT NULL,
  fitur VARCHAR(32) NOT NULL,
  tool VARCHAR(64) DEFAULT NULL,
  args_json TEXT DEFAULT NULL,
  pertanyaan TEXT DEFAULT NULL,
  jumlah_baris INT DEFAULT NULL,
  model VARCHAR(128) DEFAULT NULL,
  durasi_ms INT UNSIGNED DEFAULT NULL,
  status VARCHAR(32) NOT NULL,
  created_at TIMESTAMP NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (id),
  KEY ai_logs_user_created_index (user_id, created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
