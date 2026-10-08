CREATE TABLE IF NOT EXISTS `tbl_foto` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned DEFAULT NULL,
  `komponen_id` bigint unsigned NOT NULL,
  `penerima_id` bigint unsigned DEFAULT NULL,
  `unit_index` int DEFAULT NULL,
  `keterangan` enum('0%','25%','50%','75%','100%') CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `koordinat` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  `validasi_koordinat` tinyint(1) NOT NULL DEFAULT '0',
  `validasi_koordinat_message` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_foto_pekerjaan_id_foreign` (`pekerjaan_id`),
  KEY `tbl_foto_komponen_id_foreign` (`komponen_id`),
  KEY `tbl_foto_penerima_id_foreign` (`penerima_id`),
  KEY `idx_foto_pekerjaan_koordinat` (`pekerjaan_id`,`koordinat`),
  KEY `idx_foto_pekerjaan_koordinat_created` (`pekerjaan_id`,`koordinat`,`created_at`)
) ENGINE=InnoDB AUTO_INCREMENT=13374 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_penerima` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `nama` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `jumlah_jiwa` int DEFAULT NULL,
  `nik` text CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci,
  `alamat` text CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci,
  `is_komunal` tinyint(1) NOT NULL DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_penerima_pekerjaan_id_foreign` (`pekerjaan_id`),
  KEY `tbl_penerima_is_komunal_index` (`is_komunal`),
  FULLTEXT KEY `ft_penerima_search` (`nama`,`nik`,`alamat`)
) ENGINE=InnoDB AUTO_INCREMENT=2728 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_kontrak` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `id_kegiatan` bigint unsigned DEFAULT '0',
  `id_pekerjaan` bigint unsigned DEFAULT '0',
  `id_penyedia` bigint unsigned DEFAULT NULL,
  `kode_rup` varchar(50) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `kode_paket` varchar(50) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nomor_penawaran` varchar(50) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `tanggal_penawaran` date DEFAULT NULL,
  `nilai_kontrak` decimal(18,2) DEFAULT NULL,
  `tgl_sppbj` date DEFAULT NULL,
  `tgl_spk` date DEFAULT NULL,
  `tgl_spmk` date DEFAULT NULL,
  `tgl_selesai` date DEFAULT NULL,
  `sppbj` varchar(50) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT '0',
  `spk` varchar(50) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT '0',
  `spmk` varchar(50) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT '0',
  `spse_sppbj_id` varchar(32) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `spse_spk_id` varchar(32) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `spse_rekanan_id` varchar(32) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `spse_pushed_at` timestamp NULL DEFAULT NULL,
  `spse_push_log` json DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `idx_kontrak_pekerjaan_id` (`id_pekerjaan`),
  FULLTEXT KEY `ft_kontrak_search` (`spk`,`spmk`,`kode_paket`)
) ENGINE=InnoDB AUTO_INCREMENT=480 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `kontrak_pekerjaan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `kontrak_id` bigint unsigned NOT NULL,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `kontrak_pekerjaan_kontrak_id_pekerjaan_id_unique` (`kontrak_id`,`pekerjaan_id`),
  KEY `kontrak_pekerjaan_pekerjaan_id_foreign` (`pekerjaan_id`),
  CONSTRAINT `kontrak_pekerjaan_kontrak_id_foreign` FOREIGN KEY (`kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE CASCADE,
  CONSTRAINT `kontrak_pekerjaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=485 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_kontrak_addendums` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `kontrak_id` bigint unsigned NOT NULL,
  `addendum_ke` int unsigned NOT NULL,
  `nomor_addendum` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `attachment_nomors` json DEFAULT NULL,
  `tanggal_addendum` date NOT NULL,
  `jenis_addendum` enum('teknis','biaya','waktu','teknis_biaya','lainnya') CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'lainnya',
  `alasan` text CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci,
  `deskripsi_perubahan` text CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci,
  `nilai_kontrak_sebelum` decimal(18,2) DEFAULT NULL,
  `nilai_kontrak_sesudah` decimal(18,2) DEFAULT NULL,
  `tgl_selesai_sebelum` date DEFAULT NULL,
  `tgl_selesai_sesudah` date DEFAULT NULL,
  `status` enum('draft','diajukan','diproses','disetujui','ditolak') CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'draft',
  `kelengkapan_override` tinyint(1) NOT NULL DEFAULT '0',
  `created_by` bigint unsigned DEFAULT NULL,
  `approved_by` bigint unsigned DEFAULT NULL,
  `approved_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_kontrak_addendums_kontrak_id_addendum_ke_unique` (`kontrak_id`,`addendum_ke`),
  KEY `tbl_kontrak_addendums_created_by_foreign` (`created_by`),
  KEY `tbl_kontrak_addendums_approved_by_foreign` (`approved_by`),
  KEY `tbl_kontrak_addendums_status_tanggal_addendum_index` (`status`,`tanggal_addendum`),
  CONSTRAINT `tbl_kontrak_addendums_approved_by_foreign` FOREIGN KEY (`approved_by`) REFERENCES `users` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_kontrak_addendums_created_by_foreign` FOREIGN KEY (`created_by`) REFERENCES `users` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_kontrak_addendums_kontrak_id_foreign` FOREIGN KEY (`kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=10 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `user_pekerjaan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `user_pekerjaan_user_id_pekerjaan_id_unique` (`user_id`,`pekerjaan_id`),
  KEY `user_pekerjaan_pekerjaan_id_foreign` (`pekerjaan_id`),
  CONSTRAINT `user_pekerjaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE,
  CONSTRAINT `user_pekerjaan_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=171 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `kegiatan_role` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `role_id` bigint unsigned NOT NULL,
  `kegiatan_id` bigint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `kegiatan_role_role_id_foreign` (`role_id`),
  KEY `kegiatan_role_kegiatan_id_foreign` (`kegiatan_id`),
  CONSTRAINT `kegiatan_role_kegiatan_id_foreign` FOREIGN KEY (`kegiatan_id`) REFERENCES `tbl_kegiatan` (`id`) ON DELETE CASCADE,
  CONSTRAINT `kegiatan_role_role_id_foreign` FOREIGN KEY (`role_id`) REFERENCES `roles` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=36 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_sipd_pekerjaan_links` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `id_sub_bl` bigint unsigned NOT NULL,
  `id_rinci_sub_bl` bigint unsigned NOT NULL,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_sipd_pekerjaan_links_id_sub_bl_id_rinci_sub_bl_unique` (`id_sub_bl`,`id_rinci_sub_bl`),
  KEY `tbl_sipd_pekerjaan_links_pekerjaan_id_index` (`pekerjaan_id`)
) ENGINE=InnoDB AUTO_INCREMENT=73 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_penyedia` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `nama` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '0',
  `direktur` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '0',
  `no_akta` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '0',
  `notaris` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '0',
  `tanggal_akta` date DEFAULT NULL,
  `alamat` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '0',
  `npwp` varchar(32) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `bank` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT '0',
  `norek` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  FULLTEXT KEY `ft_penyedia_search` (`nama`,`direktur`)
) ENGINE=InnoDB AUTO_INCREMENT=188 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `pengawas` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `nama` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `nip` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `jabatan` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `telepon` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=30 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_output` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `komponen` varchar(255) NOT NULL,
  `satuan` varchar(255) NOT NULL,
  `volume` decimal(10,2) NOT NULL,
  `penerima_is_optional` tinyint(1) NOT NULL DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_output_pekerjaan_id_foreign` (`pekerjaan_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS `tbl_document_registers` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `kontrak_id` bigint unsigned NOT NULL,
  `type_id` bigint unsigned NOT NULL,
  `addendum_id` bigint unsigned DEFAULT NULL,
  `attachment_type` varchar(255) DEFAULT NULL,
  `nomor` varchar(255) NOT NULL,
  `tanggal` date NOT NULL,
  `sequence_number` int NOT NULL,
  `year` int NOT NULL,
  `description` text,
  `nilai` decimal(18,2) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_document_registers_nomor_unique` (`nomor`),
  KEY `tbl_document_registers_type_id_foreign` (`type_id`),
  KEY `tbl_document_registers_kontrak_id_index` (`kontrak_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
