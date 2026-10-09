-- Baseline schema dari backup production Arumanis (2026-10-09 06:49 UTC).
-- Hanya struktur (CREATE TABLE) dan tabel `migrations` agar kompatibel dengan Laravel.
-- Tidak berisi data. Dijalankan sekali di database kosong; di production baseline ditandai sudah diterapkan.
SET FOREIGN_KEY_CHECKS=0;

CREATE TABLE `app_settings` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `key` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `value` text CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci,
  `type` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'text',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `app_settings_key_unique` (`key`)
) ENGINE=InnoDB AUTO_INCREMENT=52 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `broadcast_histories` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `title` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `message` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `type` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `notification_type` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `url` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `is_banner` tinyint(1) NOT NULL DEFAULT '0',
  `recipient_count` int NOT NULL DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=53 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `cache` (
  `key` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `value` mediumtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `expiration` int NOT NULL,
  PRIMARY KEY (`key`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `cache_locks` (
  `key` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `owner` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `expiration` int NOT NULL,
  PRIMARY KEY (`key`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `chat_knowledge_cache` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `query_hash` varchar(64) COLLATE utf8mb4_unicode_ci NOT NULL,
  `query` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `context_summary` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `response` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `hit_count` int unsigned NOT NULL DEFAULT '0',
  `quality_score` double unsigned NOT NULL DEFAULT '0.5',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `chat_knowledge_cache_query_hash_unique` (`query_hash`),
  KEY `chat_knowledge_cache_query_hash_index` (`query_hash`),
  KEY `chat_knowledge_cache_hit_count_index` (`hit_count`)
) ENGINE=InnoDB AUTO_INCREMENT=45 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `chat_messages` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `chat_session_id` bigint unsigned NOT NULL,
  `role` enum('user','assistant') COLLATE utf8mb4_unicode_ci NOT NULL,
  `content` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `tool_calls` json DEFAULT NULL,
  `tokens_used` int unsigned DEFAULT NULL,
  `prompt_tokens` int unsigned DEFAULT NULL,
  `completion_tokens` int unsigned DEFAULT NULL,
  `cost_idr` decimal(12,2) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `idx_chat_messages_session_role_id` (`chat_session_id`,`role`,`id`),
  CONSTRAINT `chat_messages_chat_session_id_foreign` FOREIGN KEY (`chat_session_id`) REFERENCES `chat_sessions` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=318 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `chat_sessions` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `title` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'Percakapan Baru',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  `context_summary` text COLLATE utf8mb4_unicode_ci,
  `summary_upto_id` bigint unsigned DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `idx_chat_sessions_user_updated` (`user_id`,`updated_at`),
  CONSTRAINT `chat_sessions_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=101 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `chat_user_memories` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `fact_hash` varchar(64) COLLATE utf8mb4_unicode_ci NOT NULL,
  `fact` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `score` double NOT NULL DEFAULT '1',
  `hit_count` int unsigned NOT NULL DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `chat_user_memories_user_id_fact_hash_unique` (`user_id`,`fact_hash`),
  CONSTRAINT `chat_user_memories_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=13 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `desas` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `error_logs` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned DEFAULT NULL,
  `source` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL,
  `message` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `stack` longtext COLLATE utf8mb4_unicode_ci,
  `component_stack` longtext COLLATE utf8mb4_unicode_ci,
  `url` text COLLATE utf8mb4_unicode_ci,
  `user_agent` text COLLATE utf8mb4_unicode_ci,
  `ip_address` varchar(45) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `metadata` json DEFAULT NULL,
  `resolved_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `error_logs_source_created_at_index` (`source`,`created_at`),
  KEY `error_logs_user_id_created_at_index` (`user_id`,`created_at`),
  KEY `error_logs_resolved_at_index` (`resolved_at`),
  CONSTRAINT `error_logs_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=10763 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `failed_jobs` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `uuid` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `connection` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `queue` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `payload` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `exception` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `failed_at` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (`id`),
  UNIQUE KEY `failed_jobs_uuid_unique` (`uuid`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `job_batches` (
  `id` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `name` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `total_jobs` int NOT NULL,
  `pending_jobs` int NOT NULL,
  `failed_jobs` int NOT NULL,
  `failed_job_ids` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `options` mediumtext COLLATE utf8mb4_unicode_ci,
  `cancelled_at` int DEFAULT NULL,
  `created_at` int NOT NULL,
  `finished_at` int DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `jobs` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `queue` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `payload` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `attempts` tinyint unsigned NOT NULL,
  `reserved_at` int unsigned DEFAULT NULL,
  `available_at` int unsigned NOT NULL,
  `created_at` int unsigned NOT NULL,
  PRIMARY KEY (`id`),
  KEY `jobs_queue_index` (`queue`)
) ENGINE=InnoDB AUTO_INCREMENT=62572 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `kecamatans` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `kegiatan_role` (
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

CREATE TABLE `kegiatans` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `kontrak_pekerjaan` (
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

CREATE TABLE `master_fase_pekerjaans` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `jenis_proyek` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL,
  `kode_fase` varchar(30) COLLATE utf8mb4_unicode_ci NOT NULL,
  `nama_fase` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `prioritas` int NOT NULL,
  `overlap_persen` int NOT NULL DEFAULT '0',
  `durasi_faktor` double NOT NULL DEFAULT '1',
  `keywords` json NOT NULL,
  `deskripsi` text COLLATE utf8mb4_unicode_ci,
  `is_active` tinyint(1) NOT NULL DEFAULT '1',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `master_fase_jenis_kode_unique` (`jenis_proyek`,`kode_fase`),
  KEY `master_fase_pekerjaans_is_active_index` (`is_active`)
) ENGINE=InnoDB AUTO_INCREMENT=17 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `media` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `model_type` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `model_id` bigint unsigned NOT NULL,
  `uuid` char(36) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `collection_name` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `name` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `file_name` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `mime_type` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `disk` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `conversions_disk` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `size` bigint unsigned NOT NULL,
  `manipulations` json NOT NULL,
  `custom_properties` json NOT NULL,
  `generated_conversions` json NOT NULL,
  `responsive_images` json NOT NULL,
  `order_column` int unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `media_uuid_unique` (`uuid`),
  KEY `media_model_type_model_id_index` (`model_type`,`model_id`),
  KEY `media_order_column_index` (`order_column`)
) ENGINE=InnoDB AUTO_INCREMENT=17042 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `menu_permissions` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `menu_key` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `menu_label` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `menu_parent` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `allowed_roles` json DEFAULT NULL,
  `is_active` tinyint(1) NOT NULL DEFAULT '1',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `menu_permissions_menu_key_unique` (`menu_key`)
) ENGINE=InnoDB AUTO_INCREMENT=22 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `migrations` (
  `id` int unsigned NOT NULL AUTO_INCREMENT,
  `migration` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `batch` int NOT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=140 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `model_has_permissions` (
  `permission_id` bigint unsigned NOT NULL,
  `model_type` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `model_id` bigint unsigned NOT NULL,
  PRIMARY KEY (`permission_id`,`model_id`,`model_type`),
  KEY `model_has_permissions_model_id_model_type_index` (`model_id`,`model_type`),
  CONSTRAINT `model_has_permissions_permission_id_foreign` FOREIGN KEY (`permission_id`) REFERENCES `permissions` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `model_has_roles` (
  `role_id` bigint unsigned NOT NULL,
  `model_type` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `model_id` bigint unsigned NOT NULL,
  PRIMARY KEY (`role_id`,`model_id`,`model_type`),
  KEY `model_has_roles_model_id_model_type_index` (`model_id`,`model_type`),
  CONSTRAINT `model_has_roles_role_id_foreign` FOREIGN KEY (`role_id`) REFERENCES `roles` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `notifications` (
  `id` char(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `type` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `notifiable_type` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `notifiable_id` bigint unsigned NOT NULL,
  `data` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `read_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `notifications_notifiable_type_notifiable_id_index` (`notifiable_type`,`notifiable_id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `panduan_pages` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `slug` varchar(120) COLLATE utf8mb4_unicode_ci NOT NULL,
  `title` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `description` varchar(500) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `section` varchar(80) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'umum',
  `sort_order` int unsigned NOT NULL DEFAULT '0',
  `body` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `is_published` tinyint(1) NOT NULL DEFAULT '1',
  `updated_by` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `panduan_pages_slug_unique` (`slug`),
  KEY `panduan_pages_updated_by_foreign` (`updated_by`),
  KEY `panduan_pages_section_index` (`section`),
  KEY `panduan_pages_sort_order_index` (`sort_order`),
  KEY `panduan_pages_is_published_index` (`is_published`),
  CONSTRAINT `panduan_pages_updated_by_foreign` FOREIGN KEY (`updated_by`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `password_reset_tokens` (
  `email` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `token` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`email`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `pekerjaan_checklist` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `checklist_item_id` bigint unsigned NOT NULL,
  `is_checked` tinyint(1) NOT NULL DEFAULT '0',
  `checked_at` timestamp NULL DEFAULT NULL,
  `checked_by` bigint unsigned DEFAULT NULL,
  `notes` text COLLATE utf8mb4_unicode_ci,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `pekerjaan_checklist_pekerjaan_id_checklist_item_id_unique` (`pekerjaan_id`,`checklist_item_id`),
  KEY `pekerjaan_checklist_checklist_item_id_foreign` (`checklist_item_id`),
  KEY `pekerjaan_checklist_checked_by_foreign` (`checked_by`),
  CONSTRAINT `pekerjaan_checklist_checked_by_foreign` FOREIGN KEY (`checked_by`) REFERENCES `users` (`id`) ON DELETE SET NULL,
  CONSTRAINT `pekerjaan_checklist_checklist_item_id_foreign` FOREIGN KEY (`checklist_item_id`) REFERENCES `tbl_checklist_items` (`id`) ON DELETE CASCADE,
  CONSTRAINT `pekerjaan_checklist_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=456 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `pekerjaan_checklist_histories` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `checklist_item_id` bigint unsigned NOT NULL,
  `is_checked` tinyint(1) NOT NULL,
  `notes` text COLLATE utf8mb4_unicode_ci,
  `user_id` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (`id`),
  KEY `pekerjaan_checklist_histories_pekerjaan_id_created_at_index` (`pekerjaan_id`,`created_at`),
  KEY `pekerjaan_checklist_histories_checklist_item_id_created_at_index` (`checklist_item_id`,`created_at`),
  KEY `pekerjaan_checklist_histories_user_id_created_at_index` (`user_id`,`created_at`),
  CONSTRAINT `pekerjaan_checklist_histories_checklist_item_id_foreign` FOREIGN KEY (`checklist_item_id`) REFERENCES `tbl_checklist_items` (`id`) ON DELETE CASCADE,
  CONSTRAINT `pekerjaan_checklist_histories_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE,
  CONSTRAINT `pekerjaan_checklist_histories_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=380 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `pekerjaan_progress_estimasi` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `tahun_anggaran` smallint unsigned NOT NULL,
  `fisik_rencana_tanggal` date DEFAULT NULL,
  `fisik_rencana_persen` decimal(5,2) DEFAULT NULL,
  `fisik_realisasi_tanggal` date DEFAULT NULL,
  `fisik_realisasi_persen` decimal(5,2) DEFAULT NULL,
  `keuangan_rencana_tanggal` date DEFAULT NULL,
  `keuangan_rencana_persen` decimal(5,2) DEFAULT NULL,
  `keuangan_realisasi_tanggal` date DEFAULT NULL,
  `keuangan_realisasi_persen` decimal(5,2) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `pekerjaan_progress_estimasi_pekerjaan_id_tahun_anggaran_unique` (`pekerjaan_id`,`tahun_anggaran`),
  KEY `pekerjaan_progress_estimasi_tahun_anggaran_index` (`tahun_anggaran`),
  CONSTRAINT `pekerjaan_progress_estimasi_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `pekerjaan_progress_estimasi_history` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `tahun_anggaran` smallint unsigned NOT NULL,
  `jenis` enum('fisik','keuangan') COLLATE utf8mb4_unicode_ci NOT NULL,
  `tipe` enum('rencana','realisasi') COLLATE utf8mb4_unicode_ci NOT NULL,
  `tanggal` date NOT NULL,
  `persen` decimal(5,2) NOT NULL,
  `nilai` decimal(18,2) DEFAULT NULL,
  `nomor_sp2d` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `tanggal_pembuatan` date DEFAULT NULL,
  `tanggal_pencairan` date DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `ppe_history_lookup_idx` (`pekerjaan_id`,`tahun_anggaran`,`jenis`,`tipe`),
  KEY `pekerjaan_progress_estimasi_history_tanggal_index` (`tanggal`),
  CONSTRAINT `pekerjaan_progress_estimasi_history_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=2216 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `pekerjaan_tag` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `tag_id` bigint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `pekerjaan_tag_pekerjaan_id_tag_id_unique` (`pekerjaan_id`,`tag_id`),
  KEY `pekerjaan_tag_tag_id_foreign` (`tag_id`),
  CONSTRAINT `pekerjaan_tag_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE,
  CONSTRAINT `pekerjaan_tag_tag_id_foreign` FOREIGN KEY (`tag_id`) REFERENCES `tbl_tags` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=91 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `pengawas` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `nama` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `nip` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `jabatan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `telepon` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=30 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `penyedias` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `permissions` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `name` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `guard_name` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `permissions_name_guard_name_unique` (`name`,`guard_name`)
) ENGINE=InnoDB AUTO_INCREMENT=5 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `personal_access_tokens` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `tokenable_type` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `tokenable_id` bigint unsigned NOT NULL,
  `name` text CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `token` varchar(64) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `abilities` text CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci,
  `last_used_at` timestamp NULL DEFAULT NULL,
  `expires_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `personal_access_tokens_token_unique` (`token`),
  KEY `personal_access_tokens_tokenable_type_tokenable_id_index` (`tokenable_type`,`tokenable_id`),
  KEY `personal_access_tokens_expires_at_index` (`expires_at`)
) ENGINE=InnoDB AUTO_INCREMENT=2519 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `puspen_media_shares` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `title` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `description` text COLLATE utf8mb4_unicode_ci,
  `share_token` varchar(64) COLLATE utf8mb4_unicode_ci NOT NULL,
  `is_public` tinyint(1) NOT NULL DEFAULT '1',
  `expires_at` timestamp NULL DEFAULT NULL,
  `download_count` int unsigned NOT NULL DEFAULT '0',
  `last_downloaded_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  `deleted_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `puspen_media_shares_share_token_unique` (`share_token`),
  KEY `puspen_media_shares_user_id_is_public_index` (`user_id`,`is_public`),
  KEY `puspen_media_shares_expires_at_index` (`expires_at`),
  CONSTRAINT `puspen_media_shares_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=16 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `puspen_progress_fisik` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `kontrak_id` bigint unsigned NOT NULL,
  `tahun_anggaran` smallint unsigned NOT NULL,
  `rencana` decimal(5,2) DEFAULT NULL,
  `realisasi` decimal(5,2) DEFAULT NULL,
  `pho_completed` tinyint(1) NOT NULL DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `puspen_progress_fisik_kontrak_id_tahun_anggaran_unique` (`kontrak_id`,`tahun_anggaran`),
  KEY `puspen_progress_fisik_tahun_anggaran_index` (`tahun_anggaran`),
  CONSTRAINT `puspen_progress_fisik_kontrak_id_foreign` FOREIGN KEY (`kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=98 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `puspen_progress_fisik_output` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `kontrak_id` bigint unsigned NOT NULL,
  `output_id` bigint unsigned NOT NULL,
  `tahun_anggaran` smallint unsigned NOT NULL,
  `realisasi` decimal(14,2) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `puspen_pf_output_unique` (`kontrak_id`,`output_id`,`tahun_anggaran`),
  KEY `puspen_progress_fisik_output_output_id_foreign` (`output_id`),
  KEY `puspen_progress_fisik_output_tahun_anggaran_index` (`tahun_anggaran`),
  CONSTRAINT `puspen_progress_fisik_output_kontrak_id_foreign` FOREIGN KEY (`kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE CASCADE,
  CONSTRAINT `puspen_progress_fisik_output_output_id_foreign` FOREIGN KEY (`output_id`) REFERENCES `tbl_output` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=55 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `puspen_review_notes` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `user_id` bigint unsigned NOT NULL,
  `content` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `puspen_review_notes_user_id_foreign` (`user_id`),
  KEY `puspen_review_notes_pekerjaan_id_created_at_index` (`pekerjaan_id`,`created_at`),
  CONSTRAINT `puspen_review_notes_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE,
  CONSTRAINT `puspen_review_notes_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `role_has_permissions` (
  `permission_id` bigint unsigned NOT NULL,
  `role_id` bigint unsigned NOT NULL,
  PRIMARY KEY (`permission_id`,`role_id`),
  KEY `role_has_permissions_role_id_foreign` (`role_id`),
  CONSTRAINT `role_has_permissions_permission_id_foreign` FOREIGN KEY (`permission_id`) REFERENCES `permissions` (`id`) ON DELETE CASCADE,
  CONSTRAINT `role_has_permissions_role_id_foreign` FOREIGN KEY (`role_id`) REFERENCES `roles` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `roles` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `name` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `guard_name` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `roles_name_guard_name_unique` (`name`,`guard_name`)
) ENGINE=InnoDB AUTO_INCREMENT=18 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `route_permissions` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `route_path` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `route_method` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'GET',
  `description` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `allowed_roles` json NOT NULL,
  `is_active` tinyint(1) NOT NULL DEFAULT '1',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `route_permissions_route_path_route_method_index` (`route_path`,`route_method`)
) ENGINE=InnoDB AUTO_INCREMENT=496 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `sessions` (
  `id` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `user_id` bigint unsigned DEFAULT NULL,
  `ip_address` varchar(45) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `user_agent` text COLLATE utf8mb4_unicode_ci,
  `payload` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `last_activity` int NOT NULL,
  PRIMARY KEY (`id`),
  KEY `sessions_user_id_index` (`user_id`),
  KEY `sessions_last_activity_index` (`last_activity`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `signature_libraries` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `name` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `mime_type` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `data_url` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `width` int unsigned NOT NULL,
  `height` int unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  `deleted_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `signature_libraries_user_id_name_index` (`user_id`,`name`),
  CONSTRAINT `signature_libraries_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=4 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `simulation_network_versions` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `simulation_network_id` bigint unsigned NOT NULL,
  `version` int unsigned NOT NULL,
  `network_data` json NOT NULL,
  `simulation_settings` json DEFAULT NULL,
  `change_description` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `changed_by` bigint unsigned NOT NULL,
  `created_at` timestamp NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `simulation_network_versions_simulation_network_id_version_unique` (`simulation_network_id`,`version`),
  KEY `simulation_network_versions_changed_by_foreign` (`changed_by`),
  CONSTRAINT `simulation_network_versions_changed_by_foreign` FOREIGN KEY (`changed_by`) REFERENCES `users` (`id`) ON DELETE CASCADE,
  CONSTRAINT `simulation_network_versions_simulation_network_id_foreign` FOREIGN KEY (`simulation_network_id`) REFERENCES `simulation_networks` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `simulation_networks` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `name` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `description` text COLLATE utf8mb4_unicode_ci,
  `user_id` bigint unsigned NOT NULL,
  `pekerjaan_id` bigint unsigned DEFAULT NULL,
  `network_data` json NOT NULL,
  `simulation_settings` json DEFAULT NULL,
  `last_results` json DEFAULT NULL,
  `last_simulated_at` timestamp NULL DEFAULT NULL,
  `version` int unsigned NOT NULL DEFAULT '1',
  `is_public` tinyint(1) NOT NULL DEFAULT '0',
  `deleted_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `simulation_networks_user_id_created_at_index` (`user_id`,`created_at`),
  KEY `simulation_networks_pekerjaan_id_index` (`pekerjaan_id`),
  KEY `simulation_networks_is_public_index` (`is_public`),
  CONSTRAINT `simulation_networks_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL,
  CONSTRAINT `simulation_networks_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `sk` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `nomor_sk` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `nama` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `tanggal_sk` date DEFAULT NULL,
  `uploaded_by` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `sk_uploaded_by_foreign` (`uploaded_by`),
  CONSTRAINT `sk_uploaded_by_foreign` FOREIGN KEY (`uploaded_by`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `spam_kelembagaan_share_links` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `unit_spam_id` bigint unsigned NOT NULL,
  `created_by` bigint unsigned DEFAULT NULL,
  `token` varchar(64) COLLATE utf8mb4_unicode_ci NOT NULL,
  `label` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `is_active` tinyint(1) NOT NULL DEFAULT '1',
  `expires_at` timestamp NULL DEFAULT NULL,
  `max_submissions` int unsigned DEFAULT NULL,
  `submission_count` int unsigned NOT NULL DEFAULT '0',
  `admin_note` text COLLATE utf8mb4_unicode_ci,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `spam_kelembagaan_share_links_token_unique` (`token`),
  KEY `spam_kelembagaan_share_links_created_by_foreign` (`created_by`),
  KEY `spam_kelembagaan_share_links_unit_spam_id_is_active_index` (`unit_spam_id`,`is_active`),
  CONSTRAINT `spam_kelembagaan_share_links_created_by_foreign` FOREIGN KEY (`created_by`) REFERENCES `users` (`id`) ON DELETE SET NULL,
  CONSTRAINT `spam_kelembagaan_share_links_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `spam_kelembagaan_submissions` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `share_link_id` bigint unsigned NOT NULL,
  `unit_spam_id` bigint unsigned NOT NULL,
  `payload` json NOT NULL,
  `snapshot_before` json DEFAULT NULL,
  `submitter_name` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `submitter_phone` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `submitter_instansi` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `submitter_note` text COLLATE utf8mb4_unicode_ci,
  `status` varchar(20) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'pending',
  `reviewed_by` bigint unsigned DEFAULT NULL,
  `reviewed_at` timestamp NULL DEFAULT NULL,
  `review_note` text COLLATE utf8mb4_unicode_ci,
  `submitter_ip` varchar(45) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `user_agent` varchar(500) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `spam_kelembagaan_submissions_share_link_id_foreign` (`share_link_id`),
  KEY `spam_kelembagaan_submissions_reviewed_by_foreign` (`reviewed_by`),
  KEY `spam_kelembagaan_submissions_status_created_at_index` (`status`,`created_at`),
  KEY `spam_kelembagaan_submissions_unit_spam_id_status_index` (`unit_spam_id`,`status`),
  CONSTRAINT `spam_kelembagaan_submissions_reviewed_by_foreign` FOREIGN KEY (`reviewed_by`) REFERENCES `users` (`id`) ON DELETE SET NULL,
  CONSTRAINT `spam_kelembagaan_submissions_share_link_id_foreign` FOREIGN KEY (`share_link_id`) REFERENCES `spam_kelembagaan_share_links` (`id`) ON DELETE CASCADE,
  CONSTRAINT `spam_kelembagaan_submissions_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `spam_wilayah_matches` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `source_type` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL,
  `source_id` bigint unsigned NOT NULL,
  `kecamatan_raw` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `desa_raw` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `kecamatan_id` bigint unsigned DEFAULT NULL,
  `desa_id` bigint unsigned DEFAULT NULL,
  `match_status` varchar(30) COLLATE utf8mb4_unicode_ci NOT NULL,
  `match_score` tinyint unsigned NOT NULL DEFAULT '0',
  `notes` text COLLATE utf8mb4_unicode_ci,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `spam_wilayah_matches_source_type_source_id_unique` (`source_type`,`source_id`),
  KEY `spam_wilayah_matches_kecamatan_id_foreign` (`kecamatan_id`),
  KEY `spam_wilayah_matches_desa_id_source_type_index` (`desa_id`,`source_type`),
  KEY `spam_wilayah_matches_match_status_index` (`match_status`),
  CONSTRAINT `spam_wilayah_matches_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE SET NULL,
  CONSTRAINT `spam_wilayah_matches_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=883 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `spm_air_minum` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `kecamatan_id` bigint unsigned NOT NULL,
  `desa_id` bigint unsigned NOT NULL,
  `target_total_jiwa` int DEFAULT NULL,
  `jp_jiwa_terlayani` int NOT NULL DEFAULT '0',
  `bjp_jiwa_terlayani` int NOT NULL DEFAULT '0',
  `total_jiwa_terlayani` int NOT NULL DEFAULT '0',
  `belum_terlayani` int DEFAULT NULL,
  `persentase_layanan` decimal(6,2) DEFAULT NULL,
  `status_spm` varchar(30) COLLATE utf8mb4_unicode_ci NOT NULL,
  `tahun_data` smallint DEFAULT NULL,
  `last_consolidated_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `spm_air_minum_desa_id_unique` (`desa_id`),
  KEY `spm_air_minum_kecamatan_id_status_spm_index` (`kecamatan_id`,`status_spm`),
  KEY `spm_air_minum_status_spm_index` (`status_spm`),
  KEY `spm_air_minum_tahun_data_index` (`tahun_data`),
  CONSTRAINT `spm_air_minum_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE CASCADE,
  CONSTRAINT `spm_air_minum_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=362 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `spm_air_minum_sources` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `spm_air_minum_id` bigint unsigned NOT NULL,
  `source_type` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL,
  `source_id` bigint unsigned NOT NULL,
  `jenis_jaringan` varchar(10) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sr_unit` int DEFAULT NULL,
  `kk_terlayani` int DEFAULT NULL,
  `jiwa_terlayani` int DEFAULT NULL,
  `kondisi` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nama_pengelola` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `tahun_pembangunan_raw` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sumber_dana_raw` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `anggaran_rp` decimal(18,2) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `spm_air_minum_sources_spm_air_minum_id_foreign` (`spm_air_minum_id`),
  KEY `spm_air_minum_sources_source_type_source_id_index` (`source_type`,`source_id`),
  CONSTRAINT `spm_air_minum_sources_spm_air_minum_id_foreign` FOREIGN KEY (`spm_air_minum_id`) REFERENCES `spm_air_minum` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=817 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_audit_logs` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned DEFAULT NULL,
  `event` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `auditable_type` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `auditable_id` bigint unsigned NOT NULL,
  `old_values` json DEFAULT NULL,
  `new_values` json DEFAULT NULL,
  `url` text COLLATE utf8mb4_unicode_ci,
  `ip_address` varchar(45) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `user_agent` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_audit_logs_auditable_type_auditable_id_index` (`auditable_type`,`auditable_id`),
  KEY `tbl_audit_logs_user_id_foreign` (`user_id`),
  CONSTRAINT `tbl_audit_logs_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=18032 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_berita_acara` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `data` json DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_berita_acara_pekerjaan_id_unique` (`pekerjaan_id`),
  CONSTRAINT `tbl_berita_acara_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=22 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_berkas` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `jenis_dokumen` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `uploaded_by` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_berkas_pekerjaan_id_foreign` (`pekerjaan_id`),
  KEY `tbl_berkas_uploaded_by_foreign` (`uploaded_by`),
  CONSTRAINT `tbl_berkas_uploaded_by_foreign` FOREIGN KEY (`uploaded_by`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=2573 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_blog` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `title` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `slug` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `content` longtext COLLATE utf8mb4_unicode_ci NOT NULL,
  `category` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `cover_image` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `user_id` bigint unsigned NOT NULL,
  `is_published` tinyint(1) NOT NULL DEFAULT '0',
  `is_internal` tinyint(1) NOT NULL DEFAULT '0',
  `is_featured` tinyint(1) NOT NULL DEFAULT '0',
  `published_at` timestamp NULL DEFAULT NULL,
  `featured_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_blog_slug_unique` (`slug`),
  KEY `tbl_blog_user_id_foreign` (`user_id`),
  CONSTRAINT `tbl_blog_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=8 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_blog_assets` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned DEFAULT NULL,
  `blog_id` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_blog_assets_user_id_foreign` (`user_id`),
  KEY `tbl_blog_assets_blog_id_foreign` (`blog_id`),
  CONSTRAINT `tbl_blog_assets_blog_id_foreign` FOREIGN KEY (`blog_id`) REFERENCES `tbl_blog` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_blog_assets_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=4 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_blog_comment` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `blog_id` bigint unsigned NOT NULL,
  `user_id` bigint unsigned NOT NULL,
  `parent_id` bigint unsigned DEFAULT NULL,
  `body` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `depth` tinyint unsigned NOT NULL DEFAULT '0',
  `deleted_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_blog_comment_user_id_foreign` (`user_id`),
  KEY `tbl_blog_comment_parent_id_foreign` (`parent_id`),
  KEY `tbl_blog_comment_blog_id_parent_id_index` (`blog_id`,`parent_id`),
  KEY `tbl_blog_comment_blog_id_created_at_index` (`blog_id`,`created_at`),
  CONSTRAINT `tbl_blog_comment_blog_id_foreign` FOREIGN KEY (`blog_id`) REFERENCES `tbl_blog` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_blog_comment_parent_id_foreign` FOREIGN KEY (`parent_id`) REFERENCES `tbl_blog_comment` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_blog_comment_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_checklist_items` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `name` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `description` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sort_order` int NOT NULL DEFAULT '0',
  `context` varchar(30) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'pekerjaan',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_checklist_items_context_index` (`context`)
) ENGINE=InnoDB AUTO_INCREMENT=14 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_desa` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `n_desa` varchar(100) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `luas` double DEFAULT NULL,
  `jumlah_penduduk` int DEFAULT NULL,
  `jumlah_kk` int unsigned DEFAULT NULL,
  `target` int NOT NULL DEFAULT '0',
  `bjp_master` int NOT NULL DEFAULT '0',
  `kecamatan_id` int DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `idx_desa_n_desa` (`n_desa`),
  KEY `idx_desa_kecamatan_id` (`kecamatan_id`),
  KEY `idx_desa_name` (`n_desa`),
  FULLTEXT KEY `ft_desa_search` (`n_desa`)
) ENGINE=InnoDB AUTO_INCREMENT=1004 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_document_logs` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `type` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `year` int NOT NULL,
  `sequence_number` int NOT NULL,
  `full_number` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `id_pekerjaan` bigint unsigned DEFAULT NULL,
  `id_user` bigint unsigned DEFAULT NULL,
  `status` enum('active','canceled') COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'active',
  `cancel_reason` text COLLATE utf8mb4_unicode_ci,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_document_logs_id_pekerjaan_foreign` (`id_pekerjaan`),
  KEY `tbl_document_logs_id_user_foreign` (`id_user`),
  CONSTRAINT `tbl_document_logs_id_pekerjaan_foreign` FOREIGN KEY (`id_pekerjaan`) REFERENCES `tbl_pekerjaan` (`id`),
  CONSTRAINT `tbl_document_logs_id_user_foreign` FOREIGN KEY (`id_user`) REFERENCES `users` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_document_registers` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `kontrak_id` bigint unsigned NOT NULL,
  `type_id` bigint unsigned NOT NULL,
  `addendum_id` bigint unsigned DEFAULT NULL,
  `attachment_type` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nomor` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `tanggal` date NOT NULL,
  `sequence_number` int NOT NULL,
  `year` int NOT NULL,
  `description` text COLLATE utf8mb4_unicode_ci,
  `nilai` decimal(18,2) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_document_registers_nomor_unique` (`nomor`),
  KEY `tbl_document_registers_type_id_foreign` (`type_id`),
  KEY `tbl_document_registers_year_index` (`year`),
  KEY `tbl_document_registers_kontrak_id_index` (`kontrak_id`),
  KEY `tbl_document_registers_addendum_id_foreign` (`addendum_id`),
  FULLTEXT KEY `ft_document_registers_search` (`nomor`,`description`),
  CONSTRAINT `tbl_document_registers_addendum_id_foreign` FOREIGN KEY (`addendum_id`) REFERENCES `tbl_kontrak_addendums` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_document_registers_kontrak_id_foreign` FOREIGN KEY (`kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_document_registers_type_id_foreign` FOREIGN KEY (`type_id`) REFERENCES `tbl_document_types` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=412 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_document_sequences` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `year` int NOT NULL,
  `type` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'global',
  `last_number` int NOT NULL DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_document_sequences_year_type_unique` (`year`,`type`)
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_document_types` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `name` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `code` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `format_template` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_document_types_code_unique` (`code`)
) ENGINE=InnoDB AUTO_INCREMENT=11 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_draft_pekerjaan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `penyedia_id` bigint unsigned DEFAULT NULL,
  `kode_rup` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `kode_paket` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nama_pelaksana` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nama_penyedia` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_draft_pekerjaan_pekerjaan_id_foreign` (`pekerjaan_id`),
  KEY `tbl_draft_pekerjaan_penyedia_id_foreign` (`penyedia_id`),
  CONSTRAINT `tbl_draft_pekerjaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_draft_pekerjaan_penyedia_id_foreign` FOREIGN KEY (`penyedia_id`) REFERENCES `tbl_penyedia` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=40 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_events` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `title` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `is_allday` tinyint(1) NOT NULL DEFAULT '0',
  `start` datetime NOT NULL,
  `end` datetime NOT NULL,
  `category` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'event',
  `location` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `description` text COLLATE utf8mb4_unicode_ci,
  `color` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `bg_color` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `border_color` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `attachments` json DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_events_user_id_foreign` (`user_id`),
  CONSTRAINT `tbl_events_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=10 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_foto` (
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
) ENGINE=InnoDB AUTO_INCREMENT=23000 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_kanban_boards` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `slug` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `title` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `description` text COLLATE utf8mb4_unicode_ci,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_kanban_boards_slug_unique` (`slug`)
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_kanban_cards` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `board_id` bigint unsigned NOT NULL,
  `column_id` bigint unsigned NOT NULL,
  `position` int unsigned NOT NULL DEFAULT '0',
  `title` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `description` text COLLATE utf8mb4_unicode_ci,
  `status_label` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `pekerjaan_id` bigint unsigned DEFAULT NULL,
  `tiket_id` bigint unsigned DEFAULT NULL,
  `source` enum('manual','tiket') COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'manual',
  `metadata` json DEFAULT NULL,
  `created_by` bigint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_kanban_cards_board_id_tiket_id_unique` (`board_id`,`tiket_id`),
  KEY `tbl_kanban_cards_column_id_foreign` (`column_id`),
  KEY `tbl_kanban_cards_pekerjaan_id_foreign` (`pekerjaan_id`),
  KEY `tbl_kanban_cards_tiket_id_foreign` (`tiket_id`),
  KEY `tbl_kanban_cards_created_by_foreign` (`created_by`),
  CONSTRAINT `tbl_kanban_cards_board_id_foreign` FOREIGN KEY (`board_id`) REFERENCES `tbl_kanban_boards` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_kanban_cards_column_id_foreign` FOREIGN KEY (`column_id`) REFERENCES `tbl_kanban_columns` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_kanban_cards_created_by_foreign` FOREIGN KEY (`created_by`) REFERENCES `users` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_kanban_cards_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_kanban_cards_tiket_id_foreign` FOREIGN KEY (`tiket_id`) REFERENCES `tbl_tiket` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=18 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_kanban_columns` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `board_id` bigint unsigned NOT NULL,
  `title` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `position` int unsigned NOT NULL DEFAULT '0',
  `tiket_status` enum('open','pending','closed') COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `color` varchar(7) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_kanban_columns_board_id_foreign` (`board_id`),
  CONSTRAINT `tbl_kanban_columns_board_id_foreign` FOREIGN KEY (`board_id`) REFERENCES `tbl_kanban_boards` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=4 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_kecamatan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `n_kec` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `idx_kecamatan_n_kec` (`n_kec`),
  KEY `idx_kecamatan_name` (`n_kec`),
  FULLTEXT KEY `ft_kecamatan_search` (`n_kec`)
) ENGINE=InnoDB AUTO_INCREMENT=100 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_kegiatan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `nama_program` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sub_bidang` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nama_kegiatan` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nama_sub_kegiatan` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `tahun_anggaran` varchar(50) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sumber_dana` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `pagu` decimal(15,2) DEFAULT NULL,
  `kode_rekening` json DEFAULT NULL,
  `nama_pptk` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nip_pptk` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sipd_id_sub_bl` bigint unsigned DEFAULT NULL,
  `kode_sub_giat` varchar(64) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_kegiatan_tahun_anggaran_index` (`tahun_anggaran`),
  KEY `tbl_kegiatan_sipd_id_sub_bl_index` (`sipd_id_sub_bl`),
  KEY `tbl_kegiatan_kode_sub_giat_index` (`kode_sub_giat`),
  FULLTEXT KEY `ft_kegiatan_search` (`nama_kegiatan`,`nama_sub_kegiatan`,`nama_program`)
) ENGINE=InnoDB AUTO_INCREMENT=35 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_kontrak` (
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
  `spse_sppbj_id` varchar(32) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `spse_spk_id` varchar(32) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `spse_rekanan_id` varchar(32) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `spse_pushed_at` timestamp NULL DEFAULT NULL,
  `spse_push_log` json DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `idx_kontrak_pekerjaan_id` (`id_pekerjaan`),
  FULLTEXT KEY `ft_kontrak_search` (`spk`,`spmk`,`kode_paket`)
) ENGINE=InnoDB AUTO_INCREMENT=480 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_kontrak_addendum_items` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `addendum_id` bigint unsigned NOT NULL,
  `nama_item` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `spesifikasi_sebelum` text COLLATE utf8mb4_unicode_ci,
  `spesifikasi_sesudah` text COLLATE utf8mb4_unicode_ci,
  `volume_sebelum` decimal(18,4) DEFAULT NULL,
  `volume_sesudah` decimal(18,4) DEFAULT NULL,
  `harga_sebelum` decimal(18,2) DEFAULT NULL,
  `harga_sesudah` decimal(18,2) DEFAULT NULL,
  `subtotal_sebelum` decimal(18,2) DEFAULT NULL,
  `subtotal_sesudah` decimal(18,2) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_kontrak_addendum_items_addendum_id_foreign` (`addendum_id`),
  CONSTRAINT `tbl_kontrak_addendum_items_addendum_id_foreign` FOREIGN KEY (`addendum_id`) REFERENCES `tbl_kontrak_addendums` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_kontrak_addendums` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `kontrak_id` bigint unsigned NOT NULL,
  `addendum_ke` int unsigned NOT NULL,
  `nomor_addendum` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `attachment_nomors` json DEFAULT NULL,
  `tanggal_addendum` date NOT NULL,
  `jenis_addendum` enum('teknis','biaya','waktu','teknis_biaya','lainnya') COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'lainnya',
  `alasan` text COLLATE utf8mb4_unicode_ci,
  `deskripsi_perubahan` text COLLATE utf8mb4_unicode_ci,
  `nilai_kontrak_sebelum` decimal(18,2) DEFAULT NULL,
  `nilai_kontrak_sesudah` decimal(18,2) DEFAULT NULL,
  `tgl_selesai_sebelum` date DEFAULT NULL,
  `tgl_selesai_sesudah` date DEFAULT NULL,
  `status` enum('draft','diajukan','diproses','disetujui','ditolak') COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'draft',
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
) ENGINE=InnoDB AUTO_INCREMENT=15 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_live_chat_message` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `thread_id` bigint unsigned NOT NULL,
  `user_id` bigint unsigned NOT NULL,
  `message` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `read_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_live_chat_message_user_id_foreign` (`user_id`),
  KEY `tbl_live_chat_message_thread_id_id_index` (`thread_id`,`id`),
  CONSTRAINT `tbl_live_chat_message_thread_id_foreign` FOREIGN KEY (`thread_id`) REFERENCES `tbl_live_chat_thread` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_live_chat_message_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=24 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_live_chat_thread` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `status` varchar(20) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'open',
  `last_message_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_live_chat_thread_user_id_unique` (`user_id`),
  KEY `tbl_live_chat_thread_status_last_message_at_index` (`status`,`last_message_at`),
  CONSTRAINT `tbl_live_chat_thread_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=16 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_output` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `komponen` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `satuan` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `volume` decimal(10,2) NOT NULL,
  `penerima_is_optional` tinyint(1) NOT NULL DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_output_pekerjaan_id_foreign` (`pekerjaan_id`),
  FULLTEXT KEY `ft_output_search` (`komponen`,`satuan`)
) ENGINE=InnoDB AUTO_INCREMENT=396 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_pekerjaan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `kode_rekening` varchar(225) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT '0',
  `nama_paket` varchar(225) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '0',
  `kecamatan_id` int DEFAULT '0',
  `desa_id` int DEFAULT '0',
  `kegiatan_id` bigint DEFAULT NULL,
  `pagu` float NOT NULL DEFAULT '0',
  `is_konsultan` tinyint(1) NOT NULL DEFAULT '0',
  `status` varchar(32) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'active',
  `catatan` text COLLATE utf8mb4_unicode_ci,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  `pengawas_id` bigint unsigned DEFAULT NULL,
  `pendamping_id` bigint unsigned DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `idx_pekerjaan_nama_paket` (`nama_paket`),
  KEY `idx_pekerjaan_kecamatan_id` (`kecamatan_id`),
  KEY `idx_pekerjaan_desa_id` (`desa_id`),
  KEY `idx_pekerjaan_kecamatan_desa` (`kecamatan_id`,`desa_id`),
  KEY `idx_pekerjaan_kegiatan_id` (`kegiatan_id`),
  KEY `tbl_pekerjaan_pengawas_id_foreign` (`pengawas_id`),
  KEY `tbl_pekerjaan_pendamping_id_foreign` (`pendamping_id`),
  KEY `tbl_pekerjaan_kegiatan_id_kecamatan_id_index` (`kegiatan_id`,`kecamatan_id`),
  FULLTEXT KEY `ft_pekerjaan_search` (`nama_paket`,`kode_rekening`),
  CONSTRAINT `tbl_pekerjaan_pendamping_id_foreign` FOREIGN KEY (`pendamping_id`) REFERENCES `pengawas` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_pekerjaan_pengawas_id_foreign` FOREIGN KEY (`pengawas_id`) REFERENCES `pengawas` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=703 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_penerima` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `nama` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL,
  `jumlah_jiwa` int DEFAULT NULL,
  `nik` text COLLATE utf8mb4_unicode_ci,
  `alamat` text COLLATE utf8mb4_unicode_ci,
  `is_komunal` tinyint(1) NOT NULL DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_penerima_pekerjaan_id_foreign` (`pekerjaan_id`),
  KEY `tbl_penerima_is_komunal_index` (`is_komunal`),
  FULLTEXT KEY `ft_penerima_search` (`nama`,`nik`,`alamat`)
) ENGINE=InnoDB AUTO_INCREMENT=2791 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_pengelola` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `unit_spam_id` bigint unsigned NOT NULL,
  `pokmas` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `perdes` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `kepala` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `bendahara` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sekretaris` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_pengelola_unit_spam_id_unique` (`unit_spam_id`),
  CONSTRAINT `tbl_pengelola_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=1442 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_penyedia` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `nama` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '0',
  `direktur` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '0',
  `no_akta` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '0',
  `notaris` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '0',
  `tanggal_akta` date DEFAULT NULL,
  `alamat` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT '0',
  `npwp` varchar(32) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `bank` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT '0',
  `norek` varchar(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  FULLTEXT KEY `ft_penyedia_search` (`nama`,`direktur`)
) ENGINE=InnoDB AUTO_INCREMENT=188 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_peta_peripaan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned DEFAULT NULL,
  `nama` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `geojson` json DEFAULT NULL,
  `uploaded_by` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_peta_peripaan_pekerjaan_id_foreign` (`pekerjaan_id`),
  KEY `tbl_peta_peripaan_uploaded_by_foreign` (`uploaded_by`),
  CONSTRAINT `tbl_peta_peripaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_peta_peripaan_uploaded_by_foreign` FOREIGN KEY (`uploaded_by`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=7 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_procurement_staging_paket` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `sync_run_id` bigint unsigned NOT NULL,
  `sumber` varchar(32) COLLATE utf8mb4_unicode_ci NOT NULL,
  `kode_paket` varchar(32) COLLATE utf8mb4_unicode_ci NOT NULL,
  `nama_paket` varchar(500) COLLATE utf8mb4_unicode_ci NOT NULL,
  `status_paket` varchar(128) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `metode_pengadaan` varchar(128) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `jenis_paket` varchar(64) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `matched_pekerjaan_id` bigint unsigned DEFAULT NULL,
  `matched_kontrak_id` bigint unsigned DEFAULT NULL,
  `match_status` varchar(32) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'unmatched',
  `raw_row` json DEFAULT NULL,
  `fetched_at` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (`id`),
  KEY `tbl_procurement_staging_paket_sync_run_id_foreign` (`sync_run_id`),
  KEY `tbl_procurement_staging_paket_matched_pekerjaan_id_foreign` (`matched_pekerjaan_id`),
  KEY `tbl_procurement_staging_paket_matched_kontrak_id_foreign` (`matched_kontrak_id`),
  KEY `tbl_procurement_staging_paket_kode_paket_index` (`kode_paket`),
  CONSTRAINT `tbl_procurement_staging_paket_matched_kontrak_id_foreign` FOREIGN KEY (`matched_kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_procurement_staging_paket_matched_pekerjaan_id_foreign` FOREIGN KEY (`matched_pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_procurement_staging_paket_sync_run_id_foreign` FOREIGN KEY (`sync_run_id`) REFERENCES `tbl_procurement_sync_runs` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=474 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_procurement_sync_runs` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned DEFAULT NULL,
  `status` varchar(32) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'running',
  `item_count` int unsigned NOT NULL DEFAULT '0',
  `matched_count` int unsigned NOT NULL DEFAULT '0',
  `error_log` text COLLATE utf8mb4_unicode_ci,
  `started_at` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
  `finished_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_procurement_sync_runs_user_id_foreign` (`user_id`),
  CONSTRAINT `tbl_procurement_sync_runs_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=5 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_progress` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `content` json DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_progress_pekerjaan_id_foreign` (`pekerjaan_id`),
  CONSTRAINT `tbl_progress_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=165 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_sipd_pekerjaan_links` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `id_sub_bl` bigint unsigned NOT NULL,
  `id_rinci_sub_bl` bigint unsigned NOT NULL,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_sipd_pekerjaan_links_id_sub_bl_id_rinci_sub_bl_unique` (`id_sub_bl`,`id_rinci_sub_bl`),
  KEY `tbl_sipd_pekerjaan_links_pekerjaan_id_index` (`pekerjaan_id`)
) ENGINE=InnoDB AUTO_INCREMENT=84 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_spam_achievements` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `unit_spam_id` bigint unsigned NOT NULL,
  `tahun` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `sumber` varchar(20) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'manual',
  `jumlah_sr` int NOT NULL DEFAULT '0',
  `jumlah_kk` int NOT NULL DEFAULT '0',
  `jumlah_jiwa` int NOT NULL DEFAULT '0',
  `jumlah_bjp_kk` int NOT NULL DEFAULT '0',
  `jumlah_bjp_jiwa` int NOT NULL DEFAULT '0',
  `catatan` text COLLATE utf8mb4_unicode_ci,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `spam_unit_tahun_sumber_unique` (`unit_spam_id`,`tahun`,`sumber`),
  CONSTRAINT `tbl_spam_achievements_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=2068 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_spam_budgets` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `unit_spam_id` bigint unsigned NOT NULL,
  `pekerjaan_id` bigint unsigned DEFAULT NULL,
  `nilai_kontrak` double NOT NULL,
  `tahun` varchar(4) COLLATE utf8mb4_unicode_ci NOT NULL,
  `nama_paket` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `sumber_dana` varchar(50) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'APBD',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `spam_budget_unit_pekerjaan_idx` (`unit_spam_id`,`pekerjaan_id`),
  CONSTRAINT `tbl_spam_budgets_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=1042 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_spam_kelembagaan_raw` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `jenis_jaringan` varchar(10) COLLATE utf8mb4_unicode_ci NOT NULL,
  `kecamatan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `desa_kelurahan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `desa_kelurahan_normalized` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `lokasi_key` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `tahun_pembangunan_raw` varchar(100) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `tahun_pembangunan_awal` smallint DEFAULT NULL,
  `tahun_pembangunan_akhir` smallint DEFAULT NULL,
  `sumber_dana_raw` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `program_pembangunan` text COLLATE utf8mb4_unicode_ci,
  `nama_pengelola` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `perdes_pembentukan_pokmas` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `pengurus_kepala` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `pengurus_bendahara` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `pengurus_sekretaris` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `kapasitas_mata_air_l_det` decimal(12,2) DEFAULT NULL,
  `sistem_aliran` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `kapasitas_air_tanah_l_det` decimal(12,2) DEFAULT NULL,
  `kapasitas_lain_l_det` decimal(12,2) DEFAULT NULL,
  `dasar_hukum_tarif` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `besaran_iuran` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `pendapatan_bulanan_rp` decimal(18,2) DEFAULT NULL,
  `biaya_operasional_bulanan_rp` decimal(18,2) DEFAULT NULL,
  `sr_unit` int DEFAULT NULL,
  `kk_terlayani` int DEFAULT NULL,
  `jiwa_terlayani` int DEFAULT NULL,
  `target_layanan` int DEFAULT NULL,
  `raw_payload` json DEFAULT NULL,
  `source_file` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `source_sheet` varchar(100) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `source_row` int unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_spam_kelembagaan_raw_source_sheet_source_row_index` (`source_sheet`,`source_row`),
  KEY `tbl_spam_kelembagaan_raw_jenis_jaringan_index` (`jenis_jaringan`),
  KEY `tbl_spam_kelembagaan_raw_kecamatan_index` (`kecamatan`),
  KEY `tbl_spam_kelembagaan_raw_desa_kelurahan_index` (`desa_kelurahan`),
  KEY `tbl_spam_kelembagaan_raw_lokasi_key_index` (`lokasi_key`),
  KEY `tbl_spam_kelembagaan_raw_tahun_pembangunan_awal_index` (`tahun_pembangunan_awal`),
  KEY `tbl_spam_kelembagaan_raw_sumber_dana_raw_index` (`sumber_dana_raw`),
  KEY `tbl_spam_kelembagaan_raw_nama_pengelola_index` (`nama_pengelola`)
) ENGINE=InnoDB AUTO_INCREMENT=506 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_spam_terbangun_raw` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `kecamatan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `jenis_wilayah` varchar(30) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `desa_kelurahan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nama_pengelola` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sumber_air_baku` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sistem_aliran` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `debit_sumber_l_det` decimal(12,2) DEFAULT NULL,
  `debit_diambil_l_det` decimal(12,2) DEFAULT NULL,
  `penduduk_terlayani` int DEFAULT NULL,
  `jumlah_penduduk` int DEFAULT NULL,
  `hu_ku_unit` int DEFAULT NULL,
  `sr_unit` int DEFAULT NULL,
  `tanpa_meteran_air_unit` int DEFAULT NULL,
  `sumber_dana_raw` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `asal_proyek` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nilai_dak_apbn_rp` decimal(18,2) DEFAULT NULL,
  `nilai_apbd_rp` decimal(18,2) DEFAULT NULL,
  `nilai_banprov_rp` decimal(18,2) DEFAULT NULL,
  `tahun_pembangunan_raw` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `tahun_pembangunan_awal` smallint DEFAULT NULL,
  `tahun_pembangunan_akhir` smallint DEFAULT NULL,
  `kondisi_raw` varchar(100) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `kondisi_normalized` varchar(50) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `tanggal_terakhir_laporan` date DEFAULT NULL,
  `keterangan` text COLLATE utf8mb4_unicode_ci,
  `raw_payload` json DEFAULT NULL,
  `source_file` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `source_sheet` varchar(100) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `source_row` int unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_spam_terbangun_raw_source_sheet_source_row_index` (`source_sheet`,`source_row`),
  KEY `tbl_spam_terbangun_raw_kecamatan_index` (`kecamatan`),
  KEY `tbl_spam_terbangun_raw_desa_kelurahan_index` (`desa_kelurahan`),
  KEY `tbl_spam_terbangun_raw_sumber_dana_raw_index` (`sumber_dana_raw`),
  KEY `tbl_spam_terbangun_raw_tahun_pembangunan_raw_index` (`tahun_pembangunan_raw`),
  KEY `tbl_spam_terbangun_raw_tahun_pembangunan_awal_index` (`tahun_pembangunan_awal`),
  KEY `tbl_spam_terbangun_raw_kondisi_normalized_index` (`kondisi_normalized`)
) ENGINE=InnoDB AUTO_INCREMENT=378 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_spm_sanitasi` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `jenis` varchar(30) COLLATE utf8mb4_unicode_ci NOT NULL,
  `desa_id` bigint unsigned DEFAULT NULL,
  `skala_pelayanan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `nama_infrastruktur` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `latitude` decimal(11,8) DEFAULT NULL,
  `longitude` decimal(11,8) DEFAULT NULL,
  `alamat_lengkap` text COLLATE utf8mb4_unicode_ci,
  `jumlah_pemanfaat_kk` int unsigned DEFAULT NULL,
  `jumlah_pemanfaat_jiwa` int unsigned DEFAULT NULL,
  `tahun_konstruksi` smallint unsigned DEFAULT NULL,
  `pembiayaan_apbn` decimal(18,2) DEFAULT NULL,
  `pembiayaan_apbd` decimal(18,2) DEFAULT NULL,
  `pembiayaan_dak` decimal(18,2) DEFAULT NULL,
  `pembiayaan_hibah` decimal(18,2) DEFAULT NULL,
  `pembiayaan_csr` decimal(18,2) DEFAULT NULL,
  `pembiayaan_lain` decimal(18,2) DEFAULT NULL,
  `pembiayaan_total` decimal(18,2) DEFAULT NULL,
  `status_keberfungsian` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `kualitas_keberfungsian` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `pengelola` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `kapasitas_desain` decimal(12,2) DEFAULT NULL,
  `kapasitas_terpakai` decimal(12,2) DEFAULT NULL,
  `kapasitas_tidak_terpakai` decimal(12,2) DEFAULT NULL,
  `jenis_pengolahan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `peta_cakupan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `status_lahan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `luas_lahan_ha` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `opsi_teknologi` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `jumlah_stasiun_pompa` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `biaya_operasional` decimal(18,2) DEFAULT NULL,
  `jenis_pengelola` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sistem_pengolahan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `truk_tinja_unit` smallint unsigned DEFAULT NULL,
  `kapasitas_truk_m3` decimal(10,2) DEFAULT NULL,
  `jumlah_ritasi` smallint unsigned DEFAULT NULL,
  `jarak_maksimal_pelayanan_km` decimal(10,2) DEFAULT NULL,
  `alokasi_biaya_operasional` decimal(18,2) DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  `pemanfaat_dari_integrasi` tinyint(1) NOT NULL DEFAULT '0',
  `pembiayaan_dari_integrasi` tinyint(1) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`),
  KEY `tbl_spm_sanitasi_desa_id_foreign` (`desa_id`),
  KEY `tbl_spm_sanitasi_jenis_desa_id_index` (`jenis`,`desa_id`),
  KEY `tbl_spm_sanitasi_tahun_konstruksi_index` (`tahun_konstruksi`),
  CONSTRAINT `tbl_spm_sanitasi_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=264 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_spm_sanitasi_pekerjaan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `spm_sanitasi_id` bigint unsigned NOT NULL,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `output_id` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `spm_sanitasi_pekerjaan_unique` (`spm_sanitasi_id`,`pekerjaan_id`),
  KEY `tbl_spm_sanitasi_pekerjaan_output_id_foreign` (`output_id`),
  KEY `tbl_spm_sanitasi_pekerjaan_pekerjaan_id_index` (`pekerjaan_id`),
  CONSTRAINT `tbl_spm_sanitasi_pekerjaan_output_id_foreign` FOREIGN KEY (`output_id`) REFERENCES `tbl_output` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_spm_sanitasi_pekerjaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_spm_sanitasi_pekerjaan_spm_sanitasi_id_foreign` FOREIGN KEY (`spm_sanitasi_id`) REFERENCES `tbl_spm_sanitasi` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=123 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_spse_sessions` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `encrypted_cookies` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `lpse_slug` varchar(64) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'cianjurkab',
  `expires_at` timestamp NULL DEFAULT NULL,
  `last_validated_at` timestamp NULL DEFAULT NULL,
  `is_active` tinyint(1) NOT NULL DEFAULT '1',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_spse_sessions_user_id_is_active_index` (`user_id`,`is_active`),
  CONSTRAINT `tbl_spse_sessions_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=25 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_survey_lokasi` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `tugas_id` bigint unsigned DEFAULT NULL,
  `jenis` enum('spam_perpipaan','spam_pengeboran','mck_individu','mck_komunal') COLLATE utf8mb4_unicode_ci NOT NULL,
  `nama_lokasi` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `kecamatan_id` bigint unsigned DEFAULT NULL,
  `desa_id` bigint unsigned DEFAULT NULL,
  `alamat` text COLLATE utf8mb4_unicode_ci,
  `latitude` decimal(10,7) DEFAULT NULL,
  `longitude` decimal(10,7) DEFAULT NULL,
  `detail` json DEFAULT NULL,
  `status` enum('diajukan','diverifikasi','ditolak') COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'diajukan',
  `catatan_verifikasi` text COLLATE utf8mb4_unicode_ci,
  `verified_by` bigint unsigned DEFAULT NULL,
  `verified_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_survey_lokasi_user_id_foreign` (`user_id`),
  KEY `tbl_survey_lokasi_kecamatan_id_foreign` (`kecamatan_id`),
  KEY `tbl_survey_lokasi_desa_id_foreign` (`desa_id`),
  KEY `tbl_survey_lokasi_verified_by_foreign` (`verified_by`),
  KEY `tbl_survey_lokasi_tugas_id_foreign` (`tugas_id`),
  CONSTRAINT `tbl_survey_lokasi_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_lokasi_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_lokasi_tugas_id_foreign` FOREIGN KEY (`tugas_id`) REFERENCES `tbl_survey_tugas` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_lokasi_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_survey_lokasi_verified_by_foreign` FOREIGN KEY (`verified_by`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_survey_tugas` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `pekerjaan_id` bigint unsigned DEFAULT NULL,
  `judul` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `tahun_anggaran` int NOT NULL,
  `jenis` enum('spam_perpipaan','spam_pengeboran','mck_individu','mck_komunal') COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `kecamatan_id` bigint unsigned DEFAULT NULL,
  `desa_id` bigint unsigned DEFAULT NULL,
  `lokasi_catatan` text COLLATE utf8mb4_unicode_ci,
  `assignee_id` bigint unsigned DEFAULT NULL,
  `status` enum('ditugaskan','dikerjakan','selesai') COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'ditugaskan',
  `batas_waktu` date DEFAULT NULL,
  `catatan_admin` text COLLATE utf8mb4_unicode_ci,
  `created_by` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_survey_tugas_pekerjaan_id_foreign` (`pekerjaan_id`),
  KEY `tbl_survey_tugas_kecamatan_id_foreign` (`kecamatan_id`),
  KEY `tbl_survey_tugas_desa_id_foreign` (`desa_id`),
  KEY `tbl_survey_tugas_created_by_foreign` (`created_by`),
  KEY `tbl_survey_tugas_assignee_id_status_index` (`assignee_id`,`status`),
  KEY `tbl_survey_tugas_tahun_anggaran_index` (`tahun_anggaran`),
  CONSTRAINT `tbl_survey_tugas_assignee_id_foreign` FOREIGN KEY (`assignee_id`) REFERENCES `users` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_tugas_created_by_foreign` FOREIGN KEY (`created_by`) REFERENCES `users` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_tugas_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_tugas_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_survey_tugas_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_survey_tugas_assignees` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `survey_tugas_id` bigint unsigned NOT NULL,
  `user_id` bigint unsigned NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tugas_assignee_unique` (`survey_tugas_id`,`user_id`),
  KEY `tbl_survey_tugas_assignees_user_id_foreign` (`user_id`),
  CONSTRAINT `tbl_survey_tugas_assignees_survey_tugas_id_foreign` FOREIGN KEY (`survey_tugas_id`) REFERENCES `tbl_survey_tugas` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_survey_tugas_assignees_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_tags` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `name` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `slug` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `color` varchar(7) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `tbl_tags_name_unique` (`name`),
  UNIQUE KEY `tbl_tags_slug_unique` (`slug`)
) ENGINE=InnoDB AUTO_INCREMENT=14 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_tiket` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `pekerjaan_id` bigint unsigned DEFAULT NULL,
  `subjek` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `deskripsi` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `kategori` enum('bug','request','lapangan','document','other') COLLATE utf8mb4_unicode_ci DEFAULT 'other',
  `prioritas` enum('low','medium','high') COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'medium',
  `status` enum('open','pending','closed') COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'open',
  `admin_notes` text COLLATE utf8mb4_unicode_ci,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_tiket_user_id_foreign` (`user_id`),
  KEY `tbl_tiket_pekerjaan_id_foreign` (`pekerjaan_id`),
  CONSTRAINT `tbl_tiket_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_tiket_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=81 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_tiket_comment` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `tiket_id` bigint unsigned NOT NULL,
  `user_id` bigint unsigned NOT NULL,
  `message` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_tiket_comment_tiket_id_foreign` (`tiket_id`),
  KEY `tbl_tiket_comment_user_id_foreign` (`user_id`),
  CONSTRAINT `tbl_tiket_comment_tiket_id_foreign` FOREIGN KEY (`tiket_id`) REFERENCES `tbl_tiket` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_tiket_comment_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=4 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_unit_checklists` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `unit_spam_id` bigint unsigned NOT NULL,
  `item` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `is_checked` tinyint(1) NOT NULL DEFAULT '0',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_unit_checklists_unit_spam_id_foreign` (`unit_spam_id`),
  CONSTRAINT `tbl_unit_checklists_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_unit_spam` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `desa_id` bigint unsigned NOT NULL,
  `name` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `is_simspam` tinyint(1) NOT NULL DEFAULT '0',
  `sistem_layanan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sumber_mata_air_kap` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sumber_air_tanah_kap` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `lain_lain_kap` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `tahun_pembangunan` varchar(10) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `sumber_dana` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `program` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `tarif_dasar_hukum` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `iuran_nominal` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `pendapatan_bulan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `biaya_operasional` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_unit_spam_desa_id_foreign` (`desa_id`),
  CONSTRAINT `tbl_unit_spam_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=1442 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_unit_spam_pekerjaan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `unit_spam_id` bigint unsigned NOT NULL,
  `pekerjaan_id` bigint unsigned NOT NULL,
  `output_id` bigint unsigned DEFAULT NULL,
  `capaian_metric` varchar(8) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'jp',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `unit_spam_pekerjaan_unique` (`unit_spam_id`,`pekerjaan_id`),
  KEY `tbl_unit_spam_pekerjaan_output_id_foreign` (`output_id`),
  KEY `tbl_unit_spam_pekerjaan_pekerjaan_id_index` (`pekerjaan_id`),
  CONSTRAINT `tbl_unit_spam_pekerjaan_output_id_foreign` FOREIGN KEY (`output_id`) REFERENCES `tbl_output` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tbl_unit_spam_pekerjaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE,
  CONSTRAINT `tbl_unit_spam_pekerjaan_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=68 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tbl_usulan_kegiatan` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `sub_bidang` enum('air minum','sanitasi') COLLATE utf8mb4_unicode_ci NOT NULL,
  `nama_pengusul` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `kecamatan_id` bigint unsigned NOT NULL,
  `desa_id` bigint unsigned NOT NULL,
  `perihal` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `ringkasan` text COLLATE utf8mb4_unicode_ci NOT NULL,
  `tanggal_surat_masuk` date NOT NULL,
  `nomor_surat_masuk` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `tanggal_surat` date NOT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tbl_usulan_kegiatan_user_id_foreign` (`user_id`),
  KEY `tbl_usulan_kegiatan_kecamatan_id_foreign` (`kecamatan_id`),
  KEY `tbl_usulan_kegiatan_desa_id_foreign` (`desa_id`),
  CONSTRAINT `tbl_usulan_kegiatan_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`),
  CONSTRAINT `tbl_usulan_kegiatan_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`),
  CONSTRAINT `tbl_usulan_kegiatan_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=34 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tool_pdf_signature_placements` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `tool_pdf_id` bigint unsigned NOT NULL,
  `signature_id` char(36) COLLATE utf8mb4_unicode_ci NOT NULL,
  `page_number` int unsigned NOT NULL,
  `x_ratio` decimal(8,6) NOT NULL,
  `y_ratio` decimal(8,6) NOT NULL,
  `scale` decimal(8,6) NOT NULL,
  `sort_order` int unsigned NOT NULL DEFAULT '0',
  `signature_name` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `signature_file_name` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `signature_mime_type` varchar(100) COLLATE utf8mb4_unicode_ci NOT NULL,
  `signature_width` int unsigned NOT NULL,
  `signature_height` int unsigned NOT NULL,
  `signature_data_url` longtext COLLATE utf8mb4_unicode_ci,
  `signature_source_type` enum('upload','library') COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `signature_source_id` varchar(64) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tool_pdf_signature_placements_tool_pdf_id_page_number_index` (`tool_pdf_id`,`page_number`),
  CONSTRAINT `tool_pdf_signature_placements_tool_pdf_id_foreign` FOREIGN KEY (`tool_pdf_id`) REFERENCES `tool_pdfs` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=112 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `tool_pdfs` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `parent_id` bigint unsigned DEFAULT NULL,
  `name` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `original_filename` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `kind` varchar(20) COLLATE utf8mb4_unicode_ci NOT NULL DEFAULT 'source',
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  `deleted_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `tool_pdfs_parent_id_foreign` (`parent_id`),
  KEY `tool_pdfs_user_id_kind_index` (`user_id`,`kind`),
  CONSTRAINT `tool_pdfs_parent_id_foreign` FOREIGN KEY (`parent_id`) REFERENCES `tool_pdfs` (`id`) ON DELETE SET NULL,
  CONSTRAINT `tool_pdfs_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=108 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `user_drive_items` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned NOT NULL,
  `parent_id` bigint unsigned DEFAULT NULL,
  `name` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `kind` varchar(20) COLLATE utf8mb4_unicode_ci NOT NULL,
  `original_filename` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  `deleted_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `user_drive_items_parent_id_foreign` (`parent_id`),
  KEY `user_drive_items_user_id_parent_id_kind_index` (`user_id`,`parent_id`,`kind`),
  CONSTRAINT `user_drive_items_parent_id_foreign` FOREIGN KEY (`parent_id`) REFERENCES `user_drive_items` (`id`) ON DELETE SET NULL,
  CONSTRAINT `user_drive_items_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=163 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `user_drive_shares` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `item_id` bigint unsigned NOT NULL,
  `shared_to_user_id` bigint unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `user_drive_shares_item_id_shared_to_user_id_unique` (`item_id`,`shared_to_user_id`),
  KEY `user_drive_shares_shared_to_user_id_foreign` (`shared_to_user_id`),
  CONSTRAINT `user_drive_shares_item_id_foreign` FOREIGN KEY (`item_id`) REFERENCES `user_drive_items` (`id`) ON DELETE CASCADE,
  CONSTRAINT `user_drive_shares_shared_to_user_id_foreign` FOREIGN KEY (`shared_to_user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE `user_pekerjaan` (
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

CREATE TABLE `users` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `google_id` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `name` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `email` varchar(255) COLLATE utf8mb4_unicode_ci NOT NULL,
  `avatar` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `gender` varchar(20) COLLATE utf8mb4_unicode_ci DEFAULT NULL COMMENT 'male, female, or other',
  `nip` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `jabatan` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `email_verified_at` timestamp NULL DEFAULT NULL,
  `password` varchar(255) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `remember_token` varchar(100) COLLATE utf8mb4_unicode_ci DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `users_email_unique` (`email`),
  UNIQUE KEY `users_google_id_unique` (`google_id`)
) ENGINE=InnoDB AUTO_INCREMENT=50 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

DROP TABLE IF EXISTS `migrations`;
CREATE TABLE `migrations` (`id` int unsigned NOT NULL AUTO_INCREMENT, `migration` varchar(255) NOT NULL, `batch` int NOT NULL, PRIMARY KEY (`id`)) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
INSERT INTO `migrations` (`id`, `migration`, `batch`) VALUES
  (1, '0001_01_01_000000_create_users_table', 1),
  (2, '0001_01_01_000001_create_cache_table', 1),
  (3, '0001_01_01_000002_create_jobs_table', 1),
  (4, '2025_11_30_101615_create_kecamatans_table', 1),
  (5, '2025_11_30_101651_create_desas_table', 1),
  (6, '2025_11_30_101711_create_penyedias_table', 1),
  (7, '2025_11_30_101733_create_kegiatans_table', 1),
  (8, '2025_11_30_103142_create_personal_access_tokens_table', 1),
  (9, '2025_12_02_044339_create_media_table', 1),
  (10, '2025_12_02_151248_create_permission_tables', 1),
  (11, '2025_12_03_073203_create_route_permissions_table', 1),
  (12, '2025_12_04_025049_create_menu_permissions_table', 1),
  (13, '2025_12_08_200000_create_progress_items_table', 2),
  (14, '2025_12_08_200001_create_progress_weekly_table', 2),
  (15, '2024_12_08_000001_create_progress_tables', 3),
  (16, '2024_12_08_220000_convert_progress_to_json', 4),
  (17, '2024_12_08_221000_refactor_progress_to_single_json', 5),
  (18, '2025_12_14_000001_create_app_settings_table', 6),
  (19, '2025_12_16_000001_create_berita_acara_table', 7),
  (20, '2025_12_23_083913_add_google_oauth_fields_to_users_table', 8),
  (21, '2025_12_25_000001_create_user_pekerjaan_table', 8),
  (22, '2025_12_25_124515_create_tikets_table', 9),
  (23, '2025_12_26_011349_create_notifications_table', 10),
  (24, '2025_12_26_012659_create_tiket_comments_table', 10),
  (25, '2025_12_26_080222_create_events_table', 11),
  (26, '2025_12_26_080934_add_calendar_to_menu_permissions', 11),
  (27, '2025_12_26_160000_register_map_menu_permission', 12),
  (28, '2025_12_27_171848_create_audit_logs_table', 12),
  (29, '2025_12_29_000001_create_simulation_networks_table', 13),
  (30, '2026_01_03_000001_create_tags_table', 13),
  (31, '2026_01_03_000002_create_pekerjaan_tag_table', 13),
  (32, '2026_01_03_000003_create_checklist_items_table', 13),
  (33, '2026_01_03_000004_create_pekerjaan_checklist_table', 13),
  (34, '2026_01_04_000001_add_nip_jabatan_to_users_table', 13),
  (35, '2026_01_29_010917_create_pengawas_table', 14),
  (36, '2026_01_29_010933_add_pengawas_to_pekerjaan_table', 14),
  (37, '2026_02_23_064702_create_draft_pekerjaan_table', 15),
  (38, '2026_02_23_144146_add_penyedia_id_to_draft_pekerjaan_table', 15),
  (39, '2026_02_23_144730_add_rup_paket_to_draft_pekerjaan_table', 15),
  (40, '2026_02_26_224145_create_document_sequences_table', 15),
  (41, '2026_04_13_154000_add_fulltext_indexes_to_search_tables', 16),
  (42, '2026_04_13_154029_add_fulltext_indexes_to_search_tables', 16),
  (43, '2026_04_26_105100_enhance_document_sequences_table', 17),
  (44, '2026_05_04_185723_create_document_types_and_registers_tables', 17),
  (45, '2026_05_04_200619_add_performance_indexes', 17),
  (46, '2026_05_04_200647_add_fulltext_to_document_registers', 17),
  (47, '2026_05_05_141538_add_attachments_to_tbl_events_table', 18),
  (48, '2026_05_08_150136_add_unit_index_to_tbl_foto', 19),
  (49, '2026_05_09_142543_change_penerima_columns_to_text_for_encryption', 20),
  (50, '2026_05_10_145032_update_kategori_enum_in_tbl_tiket', 21),
  (51, '2026_05_10_145521_add_document_to_kategori_enum_in_tbl_tiket', 21),
  (52, '2026_05_10_155922_create_chat_sessions_table', 21),
  (53, '2026_05_11_220000_add_gender_to_users_table', 22),
  (54, '2026_05_14_034653_create_broadcast_histories_table', 23),
  (55, '2026_05_15_025118_create_blog_table', 23),
  (56, '2026_05_15_043424_add_is_internal_to_blog_table', 23),
  (57, '2026_05_18_000001_add_featured_fields_to_blog_table', 24),
  (58, '2026_05_18_000002_create_blog_assets_table', 24),
  (59, '2026_05_18_000003_add_blog_id_to_blog_assets_table', 24),
  (60, '2026_05_18_154157_add_sub_bidang_to_tbl_kegiatan', 24),
  (61, '2026_05_21_000001_create_error_logs_table', 25),
  (62, '2026_05_21_010000_create_tbl_spam_terbangun_raw_table', 25),
  (63, '2026_05_22_010000_create_tbl_spam_kelembagaan_raw_table', 25),
  (64, '2026_05_22_020000_create_spm_air_minum_tables', 25),
  (65, '2026_05_22_021000_add_funding_fields_to_spm_air_minum_sources_table', 25),
  (66, '2026_05_24_152417_create_spam_normalized_tables', 26),
  (67, '2026_05_24_160000_add_bjp_master_to_tbl_desa', 26),
  (68, '2026_05_24_164938_drop_unused_budget_columns_from_tbl_unit_spam', 27),
  (69, '2026_05_24_170000_add_biaya_pembangunan_to_tbl_unit_spam', 28),
  (70, '2026_05_24_180000_create_tbl_spam_budgets', 28),
  (71, '2026_05_27_154820_create_master_fase_pekerjaans_table', 29),
  (72, '2026_05_28_000001_create_kontrak_addendums_table', 30),
  (73, '2026_05_29_000002_create_rka_tables', 30),
  (74, '2026_05_30_032416_create_kontrak_pekerjaan_table', 31),
  (75, '2026_06_03_000000_create_signature_libraries_table', 32),
  (76, '2026_06_03_010000_create_tool_pdfs_table', 32),
  (77, '2026_06_04_000001_create_puspen_progress_fisik_table', 32),
  (78, '2026_06_04_010000_create_puspen_media_shares_table', 33),
  (79, '2026_06_04_000000_create_tool_pdf_signature_placements_table', 34),
  (80, '2026_06_19_230000_add_chat_performance_indexes', 35),
  (81, '2026_06_25_000001_create_pekerjaan_progress_estimasi_table', 36),
  (82, '2026_06_25_000002_create_pekerjaan_progress_estimasi_history_table', 36),
  (83, '2026_06_26_000001_create_kanban_tables', 37),
  (84, '2026_06_26_000002_add_context_to_checklist_items_table', 37),
  (85, '2026_06_27_000001_drop_rka_tables', 38),
  (86, '2026_06_27_100000_create_tbl_spm_sanitasi', 38),
  (87, '2026_06_27_120000_extend_spm_sanitasi_jenis_and_pekerjaan_links', 38),
  (88, '2026_06_27_140000_create_unit_spam_pekerjaan_table', 38),
  (89, '2026_06_27_180000_add_capaian_metric_to_unit_spam_pekerjaan', 38),
  (90, '2026_06_27_000001_create_blog_comments_table', 39),
  (91, '2026_06_28_000001_add_spm_detail_page_active_setting', 40),
  (92, '2026_06_28_000001_add_unique_kontrak_type_to_document_registers', 41),
  (93, '2026_06_28_000001_create_puspen_review_notes_table', 41),
  (94, '2026_07_01_010000_create_user_drive_items_table', 42),
  (95, '2026_07_03_000001_create_puspen_progress_fisik_output_table', 42),
  (96, '2026_07_03_120000_create_live_chat_tables', 42),
  (97, '2026_07_03_130000_add_pho_completed_to_puspen_progress_fisik_table', 43),
  (98, '2026_07_03_150000_create_spse_procurement_tables', 44),
  (99, '2026_07_03_160000_add_spse_fields_to_kontrak_table', 44),
  (100, '2026_07_04_000001_add_nilai_to_document_registers', 45),
  (101, '2026_07_04_100000_add_npwp_to_tbl_penyedia_table', 46),
  (102, '2026_07_08_100000_add_pptk_to_tbl_kegiatan', 46),
  (103, '2026_07_10_100000_add_kelembagaan_fields_to_unit_spam', 47),
  (104, '2026_07_10_120000_create_spam_kelembagaan_share_tables', 47),
  (105, '2026_07_12_162300_add_uploaded_by_to_tbl_berkas_table', 48),
  (106, '2026_07_13_100000_add_is_konsultan_to_tbl_pekerjaan', 49),
  (107, '2026_07_14_100000_add_jumlah_kk_to_tbl_desa', 50),
  (108, '2026_07_14_120000_add_capaian_publik_section_active_setting', 50),
  (109, '2026_07_15_120000_change_nilai_kontrak_to_decimal_on_tbl_kontrak', 51),
  (110, '2026_07_15_130000_create_pekerjaan_checklist_histories_table', 52),
  (111, '2026_07_16_120000_drop_rka_tables_again', 53),
  (112, '2026_07_16_220000_add_sipd_link_to_tbl_kegiatan', 54),
  (113, '2026_07_16_230000_unique_master_fase_jenis_kode', 54),
  (114, '2026_07_18_100000_create_panduan_pages_table', 54),
  (115, '2026_07_18_120000_add_status_and_catatan_to_tbl_pekerjaan', 55),
  (116, '2026_07_26_000001_add_sp2d_fields_to_pekerjaan_progress_estimasi_history_table', 56),
  (117, '2026_07_29_042503_create_usulan_kegiatan_table', 56),
  (118, '2026_07_30_085340_drop_unique_kontrak_type_from_document_registers_table', 57),
  (119, '2026_08_05_000001_add_surat_fields_to_usulan_kegiatan_table', 58),
  (120, '2026_08_10_000001_make_surat_fields_not_nullable', 59),
  (121, '2026_08_10_000002_add_kelengkapan_override_to_kontrak_addendums_table', 59),
  (122, '2026_08_15_000001_create_sipd_pekerjaan_links_table', 59),
  (123, '2026_08_16_000001_add_addendum_id_to_document_registers_table', 60),
  (124, '2026_08_16_000002_add_diproses_and_attachment_type', 60),
  (125, '2026_08_17_000001_create_sk_table', 61),
  (126, '2026_08_17_000002_register_pengaturan_sk_menu_permission', 61),
  (127, '2026_08_29_023835_add_nilai_tanggal_pencairan_to_pekerjaan_progress_estimasi_history', 62),
  (128, '2026_08_30_000001_create_user_drive_shares_table', 63),
  (129, '2026_08_31_000001_add_attachment_nomors_to_kontrak_addendums', 64),
  (130, '2026_09_02_000001_create_tbl_peta_peripaan_table', 65),
  (131, '2026_09_06_000001_add_cost_to_chat_messages_table', 66),
  (132, '2026_09_06_000002_add_token_breakdown_to_chat_messages_table', 67),
  (133, '2026_09_11_000001_add_chat_memory_layers', 68),
  (134, '2026_10_05_000001_create_tbl_survey_lokasi_table', 69),
  (135, '2026_10_06_000001_create_tbl_survey_tugas_table', 69),
  (136, '2026_10_06_000002_add_tugas_id_to_tbl_survey_lokasi_table', 69),
  (137, '2026_10_07_000001_update_jenis_enum_survey', 69),
  (138, '2026_10_06_000003_create_tbl_survey_tugas_assignees_table', 70),
  (139, '2026_10_08_000001_separate_spm_integration_sources', 70);
SET FOREIGN_KEY_CHECKS=1;
