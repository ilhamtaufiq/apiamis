-- Catatan pribadi user. Dibuat dari aplikasi atau dari perintah "@arumanis catat" di WhatsApp.
CREATE TABLE IF NOT EXISTS catatan (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  user_id BIGINT UNSIGNED NOT NULL,
  isi TEXT NOT NULL,
  sumber VARCHAR(16) NOT NULL DEFAULT 'web',
  chat_id VARCHAR(255) NULL,
  created_at TIMESTAMP NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (id),
  KEY catatan_user_created_index (user_id, created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
