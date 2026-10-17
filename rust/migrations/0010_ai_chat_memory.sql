-- Memori chat Asisten Data (desain UniiChat): satu chat per pengguna yang tidak pernah berakhir.
-- ai_chat_messages: log pesan, append-only, nomor urut per pengguna (seq).
-- ai_chat_nodes: pohon ringkasan biner. Node (level, idx) mencakup 2^level pesan mulai dari idx*2^level.
-- ai_chat_state: view (daftar garis [level, idx] yang menutupi seluruh chat) dalam JSON.
CREATE TABLE IF NOT EXISTS ai_chat_messages (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  user_id BIGINT UNSIGNED NOT NULL,
  seq BIGINT UNSIGNED NOT NULL,
  kind VARCHAR(16) NOT NULL,
  text MEDIUMTEXT NOT NULL,
  created_at TIMESTAMP NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (id),
  UNIQUE KEY ai_chat_messages_user_seq_unique (user_id, seq)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS ai_chat_nodes (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  user_id BIGINT UNSIGNED NOT NULL,
  level TINYINT UNSIGNED NOT NULL,
  idx BIGINT UNSIGNED NOT NULL,
  text TEXT NOT NULL,
  size INT UNSIGNED NOT NULL,
  PRIMARY KEY (id),
  UNIQUE KEY ai_chat_nodes_unique (user_id, level, idx)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS ai_chat_state (
  user_id BIGINT UNSIGNED NOT NULL,
  tampilan TEXT NOT NULL,
  PRIMARY KEY (user_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
