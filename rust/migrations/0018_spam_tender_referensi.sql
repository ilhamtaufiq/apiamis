-- Referensi tender/non-tender SPSE untuk paket SPAM. Hanya dibaca, tidak membuat paket pekerjaan dan
-- tidak mengubah unit SPAM. `unit_spam_id` diisi dari desa yang cocok, bila ada.
CREATE TABLE tbl_spam_tender_referensi (
  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
  kode_paket VARCHAR(32) NOT NULL,
  sumber VARCHAR(16) NOT NULL,
  nama_paket VARCHAR(500) NOT NULL,
  jenis_paket VARCHAR(64) NULL,
  metode_pengadaan VARCHAR(255) NULL,
  tahun_anggaran VARCHAR(32) NULL,
  nilai_pagu DECIMAL(18,2) NULL,
  nilai_hps DECIMAL(18,2) NULL,
  nilai_kontrak DECIMAL(18,2) NULL,
  tahap_paket VARCHAR(128) NULL,
  lokasi VARCHAR(500) NULL,
  kecamatan_nama VARCHAR(255) NULL,
  desa_nama VARCHAR(255) NULL,
  nama_pemenang VARCHAR(255) NULL,
  url_detail VARCHAR(500) NULL,
  desa_id BIGINT UNSIGNED NULL,
  unit_spam_id BIGINT UNSIGNED NULL,
  created_at TIMESTAMP NULL DEFAULT NULL,
  updated_at TIMESTAMP NULL DEFAULT NULL,
  PRIMARY KEY (id),
  UNIQUE KEY tbl_spam_tender_referensi_kode_paket_unique (kode_paket),
  KEY tbl_spam_tender_referensi_unit_spam_id_index (unit_spam_id),
  KEY tbl_spam_tender_referensi_desa_id_index (desa_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
