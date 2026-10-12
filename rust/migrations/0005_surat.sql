-- Penomoran surat terpusat: satu deret nomor per tahun untuk semua surat, dengan atau tanpa pekerjaan.
-- Deret nomor disimpan di surat_sequence (satu baris per tahun), diambil dengan UPDATE ... LAST_INSERT_ID(),
-- sehingga dua permintaan bersamaan tidak mendapat nomor urut yang sama.

CREATE TABLE IF NOT EXISTS surat_sequence (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  tahun SMALLINT UNSIGNED NOT NULL,
  last_number INT UNSIGNED NOT NULL DEFAULT 0,
  PRIMARY KEY (id),
  UNIQUE KEY surat_sequence_tahun_unique (tahun)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS surat (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  tahun SMALLINT UNSIGNED NOT NULL,
  sequence_number INT UNSIGNED NOT NULL,
  type_id BIGINT UNSIGNED NOT NULL,
  nomor VARCHAR(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  tanggal DATE NOT NULL,
  perihal VARCHAR(500) COLLATE utf8mb4_unicode_ci NOT NULL,
  tujuan VARCHAR(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  keterangan TEXT COLLATE utf8mb4_unicode_ci,
  pekerjaan_id BIGINT UNSIGNED DEFAULT NULL,
  created_by BIGINT UNSIGNED DEFAULT NULL,
  created_at TIMESTAMP NULL DEFAULT NULL,
  updated_at TIMESTAMP NULL DEFAULT NULL,
  PRIMARY KEY (id),
  UNIQUE KEY surat_tahun_nomor_unique (tahun, nomor),
  UNIQUE KEY surat_tahun_sequence_unique (tahun, sequence_number),
  KEY surat_tanggal_index (tanggal),
  KEY surat_pekerjaan_index (pekerjaan_id),
  CONSTRAINT surat_type_id_foreign FOREIGN KEY (type_id) REFERENCES tbl_document_types (id),
  CONSTRAINT surat_pekerjaan_id_foreign FOREIGN KEY (pekerjaan_id) REFERENCES tbl_pekerjaan (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
