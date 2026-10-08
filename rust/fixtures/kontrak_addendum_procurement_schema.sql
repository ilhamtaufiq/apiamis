-- tbl_kontrak_addendum_items dan tbl_procurement_staging_paket (sumber: docs/schema.md).
CREATE TABLE IF NOT EXISTS `tbl_kontrak_addendum_items` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `addendum_id` bigint unsigned NOT NULL,
  `nama_item` varchar(255) DEFAULT NULL,
  `spesifikasi_sebelum` text,
  `spesifikasi_sesudah` text,
  `volume_sebelum` decimal(18,4) DEFAULT NULL,
  `volume_sesudah` decimal(18,4) DEFAULT NULL,
  `harga_sebelum` decimal(18,2) DEFAULT NULL,
  `harga_sesudah` decimal(18,2) DEFAULT NULL,
  `subtotal_sebelum` decimal(18,2) DEFAULT NULL,
  `subtotal_sesudah` decimal(18,2) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_kontrak_addendum_items_addendum_id_foreign` (`addendum_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_procurement_staging_paket` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `sync_run_id` bigint unsigned NOT NULL DEFAULT 0,
  `sumber` varchar(32) NOT NULL,
  `kode_paket` varchar(32) NOT NULL,
  `nama_paket` varchar(500) NOT NULL,
  `status_paket` varchar(128) DEFAULT NULL,
  `metode_pengadaan` varchar(128) DEFAULT NULL,
  `jenis_paket` varchar(64) DEFAULT NULL,
  `matched_pekerjaan_id` bigint unsigned DEFAULT NULL,
  `matched_kontrak_id` bigint unsigned DEFAULT NULL,
  `match_status` varchar(32) NOT NULL DEFAULT 'unmatched',
  `raw_row` json DEFAULT NULL,
  `fetched_at` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (`id`),
  KEY `tbl_procurement_staging_paket_kode_paket_index` (`kode_paket`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
