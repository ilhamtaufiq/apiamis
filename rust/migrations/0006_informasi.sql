-- Informasi Arumanis: pengumuman dan informasi untuk pengguna (fitur baru, pemeliharaan sistem, dll.).
-- Ditulis oleh admin. Pengguna biasa hanya melihat yang terbit dan berada dalam rentang tampil.

CREATE TABLE IF NOT EXISTS informasi (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  judul VARCHAR(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  isi TEXT COLLATE utf8mb4_unicode_ci NOT NULL,
  kategori VARCHAR(32) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'pengumuman',
  terbit TINYINT(1) NOT NULL DEFAULT 1,
  tampil_dari DATE DEFAULT NULL,
  tampil_sampai DATE DEFAULT NULL,
  created_by BIGINT UNSIGNED DEFAULT NULL,
  created_at TIMESTAMP NULL DEFAULT NULL,
  updated_at TIMESTAMP NULL DEFAULT NULL,
  PRIMARY KEY (id),
  KEY informasi_kategori_index (kategori),
  KEY informasi_tampil_index (terbit, tampil_dari, tampil_sampai)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
