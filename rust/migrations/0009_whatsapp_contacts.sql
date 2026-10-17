-- Nomor WhatsApp: banyak per penyedia (penyedia_whatsapp) dan satu per user (users.whatsapp).
CREATE TABLE IF NOT EXISTS penyedia_whatsapp (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  penyedia_id BIGINT UNSIGNED NOT NULL,
  nomor VARCHAR(20) NOT NULL,
  label VARCHAR(100) DEFAULT NULL,
  created_at TIMESTAMP NULL DEFAULT NULL,
  updated_at TIMESTAMP NULL DEFAULT NULL,
  PRIMARY KEY (id),
  UNIQUE KEY penyedia_whatsapp_unique (penyedia_id, nomor),
  KEY penyedia_whatsapp_penyedia_index (penyedia_id),
  CONSTRAINT penyedia_whatsapp_penyedia_fk FOREIGN KEY (penyedia_id) REFERENCES tbl_penyedia (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

ALTER TABLE users ADD COLUMN whatsapp VARCHAR(20) DEFAULT NULL AFTER email;
