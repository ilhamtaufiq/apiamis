-- Staging SPAM terbangun: data dari tbl_spam_terbangun_raw dipetakan ke desa dan unit SPAM sebelum diterapkan.
-- Baris dengan desa tidak ditemukan atau nama desa rusak (#REF!) ditandai `invalid` dan tidak diterapkan.
CREATE TABLE tbl_spam_terbangun_staging (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  batch_id VARCHAR(36) NOT NULL,
  raw_id BIGINT UNSIGNED NOT NULL,
  kecamatan VARCHAR(255) NULL,
  desa_kelurahan VARCHAR(255) NULL,
  sistem_aliran VARCHAR(255) NULL,
  sumber_air_baku VARCHAR(255) NULL,
  sumber_dana_raw VARCHAR(255) NULL,
  tahun_pembangunan_awal SMALLINT NULL,
  kondisi_normalized VARCHAR(50) NULL,
  desa_id BIGINT UNSIGNED NULL,
  unit_spam_id BIGINT UNSIGNED NULL,
  match_status VARCHAR(32) NOT NULL DEFAULT 'unmatched',
  applied_at TIMESTAMP NULL DEFAULT NULL,
  created_at TIMESTAMP NULL DEFAULT NULL,
  updated_at TIMESTAMP NULL DEFAULT NULL,
  PRIMARY KEY (id),
  KEY tbl_spam_terbangun_staging_batch_id_index (batch_id),
  KEY tbl_spam_terbangun_staging_raw_id_index (raw_id),
  KEY tbl_spam_terbangun_staging_unit_spam_id_index (unit_spam_id),
  KEY tbl_spam_terbangun_staging_match_status_index (match_status)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
