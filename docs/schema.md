# Inventaris Skema Database (Fase 0.3)

Sumber: dump HeidiSQL dari MySQL Community Server 8.0.30 (Win64), database `apiamis`, berisi **struktur saja** (`CREATE TABLE`, 0 `INSERT`). File dump tidak ikut di repo. Parsing dilakukan langsung dari `CREATE TABLE`, jadi nilainya akurat untuk dump ini. Dump ini harus dianggap sebagai snapshot; perbedaan dengan produksi perlu dicek sebelum cutover.

## Ringkasan

- Tabel: **111** (engine InnoDB)
- Kolom: **1068**
- Tabel dengan `deleted_at` (soft delete): **6**
- Kolom `json`: **30**
- Kolom `decimal`: **55**
- Kolom `float`/`double`: **6**

## Temuan untuk migrasi

1. **Uang di `tbl_pekerjaan.pagu` adalah `float NOT NULL`.** Sedangkan `tbl_kegiatan.pagu` adalah `decimal(15,2)`. Campuran ini berpotensi menghasilkan selisih pembulatan antar modul. Rust harus memakai `rust_decimal`, dan perilaku pembulatan `float` dicatat di fixture supaya respon tidak berubah.
2. **Kolom uang lain juga `float`/`double`**: `tbl_spam_budgets.nilai_kontrak` (double), `tbl_desa.luas` (double), `master_fase_pekerjaans.durasi_faktor` (double). Yang `decimal` cukup banyak di modul SPM dan kontrak addendum.
3. **Data pribadi terenkripsi** di `tbl_penerima.nik` dan `tbl_penerima.alamat` (keduanya `text`, berisi payload terenkripsi Laravel). Di `tbl_spse_sessions.encrypted_cookies` (`text`) tersimpan cookie sesi SPSE. Ketiganya butuh port dekripsi dengan `APP_KEY`.
4. **`personal_access_tokens.token` adalah `varchar(64)`**, sesuai dengan hash sha256 hex. Format ini sudah dipakai di `crates/auth`.
5. **Tabel relasi** `user_pekerjaan`, `tbl_unit_spam_pekerjaan`, `pekerjaan_tag`, `tbl_survey_tugas_assignees`, dan lainnya dipakai untuk relasi banyak-ke-banyak. Pastikan kunci asing dan indeks ikut dibaca saat membuat query di Rust.
6. **Tabel log dan antrean Laravel** (`jobs`, `failed_jobs`, `cache`, `cache_locks`, `sessions`, `migrations`, `job_batches`) ikut di database yang sama. Saat cutover perlu diputuskan apakah tabel-tabel ini dipertahankan atau dikosongkan.

## Tabel dan kolom

Tabel `NULL` = nullable. Indeks dicantumkan di bawah setiap tabel.

### `app_settings` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `key` | varchar(255) | tidak | - |
| `value` | text | ya | - |
| `type` | varchar(255) | tidak | 'text' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `app_settings_key_unique` (`key`)`

### `broadcast_histories` (10 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `title` | varchar(255) | tidak | - |
| `message` | text | tidak | - |
| `type` | varchar(255) | tidak | - |
| `notification_type` | varchar(255) | tidak | - |
| `url` | varchar(255) | ya | NULL |
| `is_banner` | tinyint(1) | tidak | '0' |
| `recipient_count` | int | tidak | '0' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`

### `cache` (3 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `key` | varchar(255) | tidak | - |
| `value` | mediumtext | tidak | - |
| `expiration` | int | tidak | - |

- `PRIMARY KEY (`key`)`

### `cache_locks` (3 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `key` | varchar(255) | tidak | - |
| `owner` | varchar(255) | tidak | - |
| `expiration` | int | tidak | - |

- `PRIMARY KEY (`key`)`

### `chat_knowledge_cache` (9 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `query_hash` | varchar(64) | tidak | - |
| `query` | text | tidak | - |
| `context_summary` | text | tidak | - |
| `response` | longtext | tidak | - |
| `hit_count` | int unsigned | tidak | '0' |
| `quality_score` | double unsigned | tidak | '0.5' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `chat_knowledge_cache_query_hash_unique` (`query_hash`)`
- `KEY `chat_knowledge_cache_query_hash_index` (`query_hash`)`
- `KEY `chat_knowledge_cache_hit_count_index` (`hit_count`)`

### `chat_messages` (11 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `chat_session_id` | bigint unsigned | tidak | - |
| `role` | enum('user','assistant') | tidak | - |
| `content` | longtext | tidak | - |
| `tool_calls` | json | ya | NULL |
| `tokens_used` | int unsigned | ya | NULL |
| `prompt_tokens` | int unsigned | ya | NULL |
| `completion_tokens` | int unsigned | ya | NULL |
| `cost_idr` | decimal(12,2) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `idx_chat_messages_session_role_id` (`chat_session_id`,`role`,`id`)`
- `CONSTRAINT `chat_messages_chat_session_id_foreign` FOREIGN KEY (`chat_session_id`) REFERENCES `chat_sessions` (`id`) ON DELETE CASCADE`

### `chat_sessions` (7 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `title` | varchar(255) | tidak | 'Percakapan Baru' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |
| `context_summary` | text | ya | - |
| `summary_upto_id` | bigint unsigned | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `idx_chat_sessions_user_updated` (`user_id`,`updated_at`)`
- `CONSTRAINT `chat_sessions_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `chat_user_memories` (8 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `fact_hash` | varchar(64) | tidak | - |
| `fact` | text | tidak | - |
| `score` | double | tidak | '1' |
| `hit_count` | int unsigned | tidak | '0' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `chat_user_memories_user_id_fact_hash_unique` (`user_id`,`fact_hash`)`
- `CONSTRAINT `chat_user_memories_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `desas` (3 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`

### `document_sequences` (3 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`

### `error_logs` (13 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | ya | NULL |
| `source` | varchar(50) | tidak | - |
| `message` | text | tidak | - |
| `stack` | longtext | ya | - |
| `component_stack` | longtext | ya | - |
| `url` | text | ya | - |
| `user_agent` | text | ya | - |
| `ip_address` | varchar(45) | ya | NULL |
| `metadata` | json | ya | NULL |
| `resolved_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `error_logs_source_created_at_index` (`source`,`created_at`)`
- `KEY `error_logs_user_id_created_at_index` (`user_id`,`created_at`)`
- `KEY `error_logs_resolved_at_index` (`resolved_at`)`
- `CONSTRAINT `error_logs_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL`

### `failed_jobs` (7 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `uuid` | varchar(255) | tidak | - |
| `connection` | text | tidak | - |
| `queue` | text | tidak | - |
| `payload` | longtext | tidak | - |
| `exception` | longtext | tidak | - |
| `failed_at` | timestamp | tidak | CURRENT_TIMESTAMP |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `failed_jobs_uuid_unique` (`uuid`)`

### `jobs` (7 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `queue` | varchar(255) | tidak | - |
| `payload` | longtext | tidak | - |
| `attempts` | tinyint unsigned | tidak | - |
| `reserved_at` | int unsigned | ya | NULL |
| `available_at` | int unsigned | tidak | - |
| `created_at` | int unsigned | tidak | - |

- `PRIMARY KEY (`id`)`
- `KEY `jobs_queue_index` (`queue`)`

### `job_batches` (10 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | varchar(255) | tidak | - |
| `name` | varchar(255) | tidak | - |
| `total_jobs` | int | tidak | - |
| `pending_jobs` | int | tidak | - |
| `failed_jobs` | int | tidak | - |
| `failed_job_ids` | longtext | tidak | - |
| `options` | mediumtext | ya | - |
| `cancelled_at` | int | ya | NULL |
| `created_at` | int | tidak | - |
| `finished_at` | int | ya | NULL |

- `PRIMARY KEY (`id`)`

### `kecamatans` (3 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`

### `kegiatans` (3 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`

### `kegiatan_role` (5 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `role_id` | bigint unsigned | tidak | - |
| `kegiatan_id` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `kegiatan_role_role_id_foreign` (`role_id`)`
- `KEY `kegiatan_role_kegiatan_id_foreign` (`kegiatan_id`)`
- `CONSTRAINT `kegiatan_role_kegiatan_id_foreign` FOREIGN KEY (`kegiatan_id`) REFERENCES `tbl_kegiatan` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `kegiatan_role_role_id_foreign` FOREIGN KEY (`role_id`) REFERENCES `roles` (`id`) ON DELETE CASCADE`

### `kontrak_pekerjaan` (5 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `kontrak_id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `kontrak_pekerjaan_kontrak_id_pekerjaan_id_unique` (`kontrak_id`,`pekerjaan_id`)`
- `KEY `kontrak_pekerjaan_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `CONSTRAINT `kontrak_pekerjaan_kontrak_id_foreign` FOREIGN KEY (`kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `kontrak_pekerjaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`

### `master_fase_pekerjaans` (12 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `jenis_proyek` | varchar(50) | tidak | - |
| `kode_fase` | varchar(30) | tidak | - |
| `nama_fase` | varchar(100) | tidak | - |
| `prioritas` | int | tidak | - |
| `overlap_persen` | int | tidak | '0' |
| `durasi_faktor` | double | tidak | '1' |
| `keywords` | json | tidak | - |
| `deskripsi` | text | ya | - |
| `is_active` | tinyint(1) | tidak | '1' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `master_fase_jenis_kode_unique` (`jenis_proyek`,`kode_fase`)`
- `KEY `master_fase_pekerjaans_is_active_index` (`is_active`)`

### `media` (18 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `model_type` | varchar(255) | tidak | - |
| `model_id` | bigint unsigned | tidak | - |
| `uuid` | char(36) | ya | NULL |
| `collection_name` | varchar(255) | tidak | - |
| `name` | varchar(255) | tidak | - |
| `file_name` | varchar(255) | tidak | - |
| `mime_type` | varchar(255) | ya | NULL |
| `disk` | varchar(255) | tidak | - |
| `conversions_disk` | varchar(255) | ya | NULL |
| `size` | bigint unsigned | tidak | - |
| `manipulations` | json | tidak | - |
| `custom_properties` | json | tidak | - |
| `generated_conversions` | json | tidak | - |
| `responsive_images` | json | tidak | - |
| `order_column` | int unsigned | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `media_uuid_unique` (`uuid`)`
- `KEY `media_model_type_model_id_index` (`model_type`,`model_id`)`
- `KEY `media_order_column_index` (`order_column`)`

### `menu_permissions` (8 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `menu_key` | varchar(255) | tidak | - |
| `menu_label` | varchar(255) | tidak | - |
| `menu_parent` | varchar(255) | ya | NULL |
| `allowed_roles` | json | ya | NULL |
| `is_active` | tinyint(1) | tidak | '1' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `menu_permissions_menu_key_unique` (`menu_key`)`

### `migrations` (3 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | int unsigned | tidak | - |
| `migration` | varchar(255) | tidak | - |
| `batch` | int | tidak | - |

- `PRIMARY KEY (`id`)`

### `model_has_permissions` (3 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `permission_id` | bigint unsigned | tidak | - |
| `model_type` | varchar(255) | tidak | - |
| `model_id` | bigint unsigned | tidak | - |

- `PRIMARY KEY (`permission_id`,`model_id`,`model_type`)`
- `KEY `model_has_permissions_model_id_model_type_index` (`model_id`,`model_type`)`
- `CONSTRAINT `model_has_permissions_permission_id_foreign` FOREIGN KEY (`permission_id`) REFERENCES `permissions` (`id`) ON DELETE CASCADE`

### `model_has_roles` (3 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `role_id` | bigint unsigned | tidak | - |
| `model_type` | varchar(255) | tidak | - |
| `model_id` | bigint unsigned | tidak | - |

- `PRIMARY KEY (`role_id`,`model_id`,`model_type`)`
- `KEY `model_has_roles_model_id_model_type_index` (`model_id`,`model_type`)`
- `CONSTRAINT `model_has_roles_role_id_foreign` FOREIGN KEY (`role_id`) REFERENCES `roles` (`id`) ON DELETE CASCADE`

### `notifications` (8 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | char(36) | tidak | - |
| `type` | varchar(255) | tidak | - |
| `notifiable_type` | varchar(255) | tidak | - |
| `notifiable_id` | bigint unsigned | tidak | - |
| `data` | text | tidak | - |
| `read_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `notifications_notifiable_type_notifiable_id_index` (`notifiable_type`,`notifiable_id`)`

### `panduan_pages` (11 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `slug` | varchar(120) | tidak | - |
| `title` | varchar(255) | tidak | - |
| `description` | varchar(500) | ya | NULL |
| `section` | varchar(80) | tidak | 'umum' |
| `sort_order` | int unsigned | tidak | '0' |
| `body` | longtext | tidak | - |
| `is_published` | tinyint(1) | tidak | '1' |
| `updated_by` | bigint unsigned | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `panduan_pages_slug_unique` (`slug`)`
- `KEY `panduan_pages_updated_by_foreign` (`updated_by`)`
- `KEY `panduan_pages_section_index` (`section`)`
- `KEY `panduan_pages_sort_order_index` (`sort_order`)`
- `KEY `panduan_pages_is_published_index` (`is_published`)`
- `CONSTRAINT `panduan_pages_updated_by_foreign` FOREIGN KEY (`updated_by`) REFERENCES `users` (`id`) ON DELETE SET NULL`

### `password_reset_tokens` (3 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `email` | varchar(255) | tidak | - |
| `token` | varchar(255) | tidak | - |
| `created_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`email`)`

### `pekerjaan_checklist` (9 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `checklist_item_id` | bigint unsigned | tidak | - |
| `is_checked` | tinyint(1) | tidak | '0' |
| `checked_at` | timestamp | ya | NULL |
| `checked_by` | bigint unsigned | ya | NULL |
| `notes` | text | ya | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `pekerjaan_checklist_pekerjaan_id_checklist_item_id_unique` (`pekerjaan_id`,`checklist_item_id`)`
- `KEY `pekerjaan_checklist_checklist_item_id_foreign` (`checklist_item_id`)`
- `KEY `pekerjaan_checklist_checked_by_foreign` (`checked_by`)`
- `CONSTRAINT `pekerjaan_checklist_checked_by_foreign` FOREIGN KEY (`checked_by`) REFERENCES `users` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `pekerjaan_checklist_checklist_item_id_foreign` FOREIGN KEY (`checklist_item_id`) REFERENCES `tbl_checklist_items` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `pekerjaan_checklist_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`

### `pekerjaan_checklist_histories` (7 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `checklist_item_id` | bigint unsigned | tidak | - |
| `is_checked` | tinyint(1) | tidak | - |
| `notes` | text | ya | - |
| `user_id` | bigint unsigned | ya | NULL |
| `created_at` | timestamp | tidak | CURRENT_TIMESTAMP |

- `PRIMARY KEY (`id`)`
- `KEY `pekerjaan_checklist_histories_pekerjaan_id_created_at_index` (`pekerjaan_id`,`created_at`)`
- `KEY `pekerjaan_checklist_histories_checklist_item_id_created_at_index` (`checklist_item_id`,`created_at`)`
- `KEY `pekerjaan_checklist_histories_user_id_created_at_index` (`user_id`,`created_at`)`
- `CONSTRAINT `pekerjaan_checklist_histories_checklist_item_id_foreign` FOREIGN KEY (`checklist_item_id`) REFERENCES `tbl_checklist_items` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `pekerjaan_checklist_histories_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `pekerjaan_checklist_histories_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL`

### `pekerjaan_progress_estimasi` (13 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `tahun_anggaran` | smallint unsigned | tidak | - |
| `fisik_rencana_tanggal` | date | ya | NULL |
| `fisik_rencana_persen` | decimal(5,2) | ya | NULL |
| `fisik_realisasi_tanggal` | date | ya | NULL |
| `fisik_realisasi_persen` | decimal(5,2) | ya | NULL |
| `keuangan_rencana_tanggal` | date | ya | NULL |
| `keuangan_rencana_persen` | decimal(5,2) | ya | NULL |
| `keuangan_realisasi_tanggal` | date | ya | NULL |
| `keuangan_realisasi_persen` | decimal(5,2) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `pekerjaan_progress_estimasi_pekerjaan_id_tahun_anggaran_unique` (`pekerjaan_id`,`tahun_anggaran`)`
- `KEY `pekerjaan_progress_estimasi_tahun_anggaran_index` (`tahun_anggaran`)`
- `CONSTRAINT `pekerjaan_progress_estimasi_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`

### `pekerjaan_progress_estimasi_history` (13 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `tahun_anggaran` | smallint unsigned | tidak | - |
| `jenis` | enum('fisik','keuangan') | tidak | - |
| `tipe` | enum('rencana','realisasi') | tidak | - |
| `tanggal` | date | tidak | - |
| `persen` | decimal(5,2) | tidak | - |
| `nilai` | decimal(18,2) | ya | NULL |
| `nomor_sp2d` | varchar(255) | ya | NULL |
| `tanggal_pembuatan` | date | ya | NULL |
| `tanggal_pencairan` | date | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `ppe_history_lookup_idx` (`pekerjaan_id`,`tahun_anggaran`,`jenis`,`tipe`)`
- `KEY `pekerjaan_progress_estimasi_history_tanggal_index` (`tanggal`)`
- `CONSTRAINT `pekerjaan_progress_estimasi_history_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`

### `pekerjaan_tag` (5 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `tag_id` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `pekerjaan_tag_pekerjaan_id_tag_id_unique` (`pekerjaan_id`,`tag_id`)`
- `KEY `pekerjaan_tag_tag_id_foreign` (`tag_id`)`
- `CONSTRAINT `pekerjaan_tag_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `pekerjaan_tag_tag_id_foreign` FOREIGN KEY (`tag_id`) REFERENCES `tbl_tags` (`id`) ON DELETE CASCADE`

### `pengawas` (7 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `nama` | varchar(255) | tidak | - |
| `nip` | varchar(255) | ya | NULL |
| `jabatan` | varchar(255) | ya | NULL |
| `telepon` | varchar(255) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`

### `penyedias` (3 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`

### `permissions` (5 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `name` | varchar(255) | tidak | - |
| `guard_name` | varchar(255) | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `permissions_name_guard_name_unique` (`name`,`guard_name`)`

### `personal_access_tokens` (10 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `tokenable_type` | varchar(255) | tidak | - |
| `tokenable_id` | bigint unsigned | tidak | - |
| `name` | text | tidak | - |
| `token` | varchar(64) | tidak | - |
| `abilities` | text | ya | - |
| `last_used_at` | timestamp | ya | NULL |
| `expires_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `personal_access_tokens_token_unique` (`token`)`
- `KEY `personal_access_tokens_tokenable_type_tokenable_id_index` (`tokenable_type`,`tokenable_id`)`
- `KEY `personal_access_tokens_expires_at_index` (`expires_at`)`

### `puspen_media_shares` (12 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `title` | varchar(255) | tidak | - |
| `description` | text | ya | - |
| `share_token` | varchar(64) | tidak | - |
| `is_public` | tinyint(1) | tidak | '1' |
| `expires_at` | timestamp | ya | NULL |
| `download_count` | int unsigned | tidak | '0' |
| `last_downloaded_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |
| `deleted_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `puspen_media_shares_share_token_unique` (`share_token`)`
- `KEY `puspen_media_shares_user_id_is_public_index` (`user_id`,`is_public`)`
- `KEY `puspen_media_shares_expires_at_index` (`expires_at`)`
- `CONSTRAINT `puspen_media_shares_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `puspen_progress_fisik` (8 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `kontrak_id` | bigint unsigned | tidak | - |
| `tahun_anggaran` | smallint unsigned | tidak | - |
| `rencana` | decimal(5,2) | ya | NULL |
| `realisasi` | decimal(5,2) | ya | NULL |
| `pho_completed` | tinyint(1) | tidak | '0' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `puspen_progress_fisik_kontrak_id_tahun_anggaran_unique` (`kontrak_id`,`tahun_anggaran`)`
- `KEY `puspen_progress_fisik_tahun_anggaran_index` (`tahun_anggaran`)`
- `CONSTRAINT `puspen_progress_fisik_kontrak_id_foreign` FOREIGN KEY (`kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE CASCADE`

### `puspen_progress_fisik_output` (7 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `kontrak_id` | bigint unsigned | tidak | - |
| `output_id` | bigint unsigned | tidak | - |
| `tahun_anggaran` | smallint unsigned | tidak | - |
| `realisasi` | decimal(14,2) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `puspen_pf_output_unique` (`kontrak_id`,`output_id`,`tahun_anggaran`)`
- `KEY `puspen_progress_fisik_output_output_id_foreign` (`output_id`)`
- `KEY `puspen_progress_fisik_output_tahun_anggaran_index` (`tahun_anggaran`)`
- `CONSTRAINT `puspen_progress_fisik_output_kontrak_id_foreign` FOREIGN KEY (`kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `puspen_progress_fisik_output_output_id_foreign` FOREIGN KEY (`output_id`) REFERENCES `tbl_output` (`id`) ON DELETE CASCADE`

### `puspen_review_notes` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `content` | text | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `puspen_review_notes_user_id_foreign` (`user_id`)`
- `KEY `puspen_review_notes_pekerjaan_id_created_at_index` (`pekerjaan_id`,`created_at`)`
- `CONSTRAINT `puspen_review_notes_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `puspen_review_notes_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `roles` (5 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `name` | varchar(255) | tidak | - |
| `guard_name` | varchar(255) | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `roles_name_guard_name_unique` (`name`,`guard_name`)`

### `role_has_permissions` (2 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `permission_id` | bigint unsigned | tidak | - |
| `role_id` | bigint unsigned | tidak | - |

- `PRIMARY KEY (`permission_id`,`role_id`)`
- `KEY `role_has_permissions_role_id_foreign` (`role_id`)`
- `CONSTRAINT `role_has_permissions_permission_id_foreign` FOREIGN KEY (`permission_id`) REFERENCES `permissions` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `role_has_permissions_role_id_foreign` FOREIGN KEY (`role_id`) REFERENCES `roles` (`id`) ON DELETE CASCADE`

### `route_permissions` (8 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `route_path` | varchar(255) | tidak | - |
| `route_method` | varchar(255) | tidak | 'GET' |
| `description` | varchar(255) | ya | NULL |
| `allowed_roles` | json | tidak | - |
| `is_active` | tinyint(1) | tidak | '1' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `route_permissions_route_path_route_method_index` (`route_path`,`route_method`)`

### `sessions` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | varchar(255) | tidak | - |
| `user_id` | bigint unsigned | ya | NULL |
| `ip_address` | varchar(45) | ya | NULL |
| `user_agent` | text | ya | - |
| `payload` | longtext | tidak | - |
| `last_activity` | int | tidak | - |

- `PRIMARY KEY (`id`)`
- `KEY `sessions_user_id_index` (`user_id`)`
- `KEY `sessions_last_activity_index` (`last_activity`)`

### `signature_libraries` (10 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `name` | varchar(255) | tidak | - |
| `mime_type` | varchar(100) | tidak | - |
| `data_url` | longtext | tidak | - |
| `width` | int unsigned | tidak | - |
| `height` | int unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |
| `deleted_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `signature_libraries_user_id_name_index` (`user_id`,`name`)`
- `CONSTRAINT `signature_libraries_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `simulation_networks` (14 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `name` | varchar(255) | tidak | - |
| `description` | text | ya | - |
| `user_id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | ya | NULL |
| `network_data` | json | tidak | - |
| `simulation_settings` | json | ya | NULL |
| `last_results` | json | ya | NULL |
| `last_simulated_at` | timestamp | ya | NULL |
| `version` | int unsigned | tidak | '1' |
| `is_public` | tinyint(1) | tidak | '0' |
| `deleted_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `simulation_networks_user_id_created_at_index` (`user_id`,`created_at`)`
- `KEY `simulation_networks_pekerjaan_id_index` (`pekerjaan_id`)`
- `KEY `simulation_networks_is_public_index` (`is_public`)`
- `CONSTRAINT `simulation_networks_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `simulation_networks_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `simulation_network_versions` (8 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `simulation_network_id` | bigint unsigned | tidak | - |
| `version` | int unsigned | tidak | - |
| `network_data` | json | tidak | - |
| `simulation_settings` | json | ya | NULL |
| `change_description` | varchar(255) | ya | NULL |
| `changed_by` | bigint unsigned | tidak | - |
| `created_at` | timestamp | tidak | - |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `simulation_network_versions_simulation_network_id_version_unique` (`simulation_network_id`,`version`)`
- `KEY `simulation_network_versions_changed_by_foreign` (`changed_by`)`
- `CONSTRAINT `simulation_network_versions_changed_by_foreign` FOREIGN KEY (`changed_by`) REFERENCES `users` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `simulation_network_versions_simulation_network_id_foreign` FOREIGN KEY (`simulation_network_id`) REFERENCES `simulation_networks` (`id`) ON DELETE CASCADE`

### `sk` (7 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `nomor_sk` | varchar(255) | tidak | - |
| `nama` | varchar(255) | tidak | - |
| `tanggal_sk` | date | ya | NULL |
| `uploaded_by` | bigint unsigned | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `sk_uploaded_by_foreign` (`uploaded_by`)`
- `CONSTRAINT `sk_uploaded_by_foreign` FOREIGN KEY (`uploaded_by`) REFERENCES `users` (`id`) ON DELETE SET NULL`

### `spam_kelembagaan_share_links` (12 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `unit_spam_id` | bigint unsigned | tidak | - |
| `created_by` | bigint unsigned | ya | NULL |
| `token` | varchar(64) | tidak | - |
| `label` | varchar(255) | ya | NULL |
| `is_active` | tinyint(1) | tidak | '1' |
| `expires_at` | timestamp | ya | NULL |
| `max_submissions` | int unsigned | ya | NULL |
| `submission_count` | int unsigned | tidak | '0' |
| `admin_note` | text | ya | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `spam_kelembagaan_share_links_token_unique` (`token`)`
- `KEY `spam_kelembagaan_share_links_created_by_foreign` (`created_by`)`
- `KEY `spam_kelembagaan_share_links_unit_spam_id_is_active_index` (`unit_spam_id`,`is_active`)`
- `CONSTRAINT `spam_kelembagaan_share_links_created_by_foreign` FOREIGN KEY (`created_by`) REFERENCES `users` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `spam_kelembagaan_share_links_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE`

### `spam_kelembagaan_submissions` (17 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `share_link_id` | bigint unsigned | tidak | - |
| `unit_spam_id` | bigint unsigned | tidak | - |
| `payload` | json | tidak | - |
| `snapshot_before` | json | ya | NULL |
| `submitter_name` | varchar(255) | ya | NULL |
| `submitter_phone` | varchar(50) | ya | NULL |
| `submitter_instansi` | varchar(255) | ya | NULL |
| `submitter_note` | text | ya | - |
| `status` | varchar(20) | tidak | 'pending' |
| `reviewed_by` | bigint unsigned | ya | NULL |
| `reviewed_at` | timestamp | ya | NULL |
| `review_note` | text | ya | - |
| `submitter_ip` | varchar(45) | ya | NULL |
| `user_agent` | varchar(500) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `spam_kelembagaan_submissions_share_link_id_foreign` (`share_link_id`)`
- `KEY `spam_kelembagaan_submissions_reviewed_by_foreign` (`reviewed_by`)`
- `KEY `spam_kelembagaan_submissions_status_created_at_index` (`status`,`created_at`)`
- `KEY `spam_kelembagaan_submissions_unit_spam_id_status_index` (`unit_spam_id`,`status`)`
- `CONSTRAINT `spam_kelembagaan_submissions_reviewed_by_foreign` FOREIGN KEY (`reviewed_by`) REFERENCES `users` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `spam_kelembagaan_submissions_share_link_id_foreign` FOREIGN KEY (`share_link_id`) REFERENCES `spam_kelembagaan_share_links` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `spam_kelembagaan_submissions_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE`

### `spam_wilayah_matches` (12 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `source_type` | varchar(50) | tidak | - |
| `source_id` | bigint unsigned | tidak | - |
| `kecamatan_raw` | varchar(255) | ya | NULL |
| `desa_raw` | varchar(255) | ya | NULL |
| `kecamatan_id` | bigint unsigned | ya | NULL |
| `desa_id` | bigint unsigned | ya | NULL |
| `match_status` | varchar(30) | tidak | - |
| `match_score` | tinyint unsigned | tidak | '0' |
| `notes` | text | ya | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `spam_wilayah_matches_source_type_source_id_unique` (`source_type`,`source_id`)`
- `KEY `spam_wilayah_matches_kecamatan_id_foreign` (`kecamatan_id`)`
- `KEY `spam_wilayah_matches_desa_id_source_type_index` (`desa_id`,`source_type`)`
- `KEY `spam_wilayah_matches_match_status_index` (`match_status`)`
- `CONSTRAINT `spam_wilayah_matches_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `spam_wilayah_matches_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`) ON DELETE SET NULL`

### `spm_air_minum` (14 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `kecamatan_id` | bigint unsigned | tidak | - |
| `desa_id` | bigint unsigned | tidak | - |
| `target_total_jiwa` | int | ya | NULL |
| `jp_jiwa_terlayani` | int | tidak | '0' |
| `bjp_jiwa_terlayani` | int | tidak | '0' |
| `total_jiwa_terlayani` | int | tidak | '0' |
| `belum_terlayani` | int | ya | NULL |
| `persentase_layanan` | decimal(6,2) | ya | NULL |
| `status_spm` | varchar(30) | tidak | - |
| `tahun_data` | smallint | ya | NULL |
| `last_consolidated_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `spm_air_minum_desa_id_unique` (`desa_id`)`
- `KEY `spm_air_minum_kecamatan_id_status_spm_index` (`kecamatan_id`,`status_spm`)`
- `KEY `spm_air_minum_status_spm_index` (`status_spm`)`
- `KEY `spm_air_minum_tahun_data_index` (`tahun_data`)`
- `CONSTRAINT `spm_air_minum_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `spm_air_minum_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`) ON DELETE CASCADE`

### `spm_air_minum_sources` (15 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `spm_air_minum_id` | bigint unsigned | tidak | - |
| `source_type` | varchar(50) | tidak | - |
| `source_id` | bigint unsigned | tidak | - |
| `jenis_jaringan` | varchar(10) | ya | NULL |
| `sr_unit` | int | ya | NULL |
| `kk_terlayani` | int | ya | NULL |
| `jiwa_terlayani` | int | ya | NULL |
| `kondisi` | varchar(255) | ya | NULL |
| `nama_pengelola` | varchar(255) | ya | NULL |
| `tahun_pembangunan_raw` | varchar(255) | ya | NULL |
| `sumber_dana_raw` | varchar(255) | ya | NULL |
| `anggaran_rp` | decimal(18,2) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `spm_air_minum_sources_spm_air_minum_id_foreign` (`spm_air_minum_id`)`
- `KEY `spm_air_minum_sources_source_type_source_id_index` (`source_type`,`source_id`)`
- `CONSTRAINT `spm_air_minum_sources_spm_air_minum_id_foreign` FOREIGN KEY (`spm_air_minum_id`) REFERENCES `spm_air_minum` (`id`) ON DELETE CASCADE`

### `tbl_audit_logs` (12 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | ya | NULL |
| `event` | varchar(255) | tidak | - |
| `auditable_type` | varchar(255) | tidak | - |
| `auditable_id` | bigint unsigned | tidak | - |
| `old_values` | json | ya | NULL |
| `new_values` | json | ya | NULL |
| `url` | text | ya | - |
| `ip_address` | varchar(45) | ya | NULL |
| `user_agent` | varchar(255) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_audit_logs_auditable_type_auditable_id_index` (`auditable_type`,`auditable_id`)`
- `KEY `tbl_audit_logs_user_id_foreign` (`user_id`)`
- `CONSTRAINT `tbl_audit_logs_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL`

### `tbl_berita_acara` (5 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `data` | json | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_berita_acara_pekerjaan_id_unique` (`pekerjaan_id`)`
- `CONSTRAINT `tbl_berita_acara_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`

### `tbl_berkas` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `jenis_dokumen` | varchar(255) | tidak | - |
| `uploaded_by` | bigint unsigned | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_berkas_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `KEY `tbl_berkas_uploaded_by_foreign` (`uploaded_by`)`
- `CONSTRAINT `tbl_berkas_uploaded_by_foreign` FOREIGN KEY (`uploaded_by`) REFERENCES `users` (`id`) ON DELETE SET NULL`

### `tbl_blog` (14 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `title` | varchar(255) | tidak | - |
| `slug` | varchar(255) | tidak | - |
| `content` | longtext | tidak | - |
| `category` | varchar(255) | ya | NULL |
| `cover_image` | varchar(255) | ya | NULL |
| `user_id` | bigint unsigned | tidak | - |
| `is_published` | tinyint(1) | tidak | '0' |
| `is_internal` | tinyint(1) | tidak | '0' |
| `is_featured` | tinyint(1) | tidak | '0' |
| `published_at` | timestamp | ya | NULL |
| `featured_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_blog_slug_unique` (`slug`)`
- `KEY `tbl_blog_user_id_foreign` (`user_id`)`
- `CONSTRAINT `tbl_blog_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `tbl_blog_assets` (5 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | ya | NULL |
| `blog_id` | bigint unsigned | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_blog_assets_user_id_foreign` (`user_id`)`
- `KEY `tbl_blog_assets_blog_id_foreign` (`blog_id`)`
- `CONSTRAINT `tbl_blog_assets_blog_id_foreign` FOREIGN KEY (`blog_id`) REFERENCES `tbl_blog` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_blog_assets_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL`

### `tbl_blog_comment` (9 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `blog_id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `parent_id` | bigint unsigned | ya | NULL |
| `body` | text | tidak | - |
| `depth` | tinyint unsigned | tidak | '0' |
| `deleted_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_blog_comment_user_id_foreign` (`user_id`)`
- `KEY `tbl_blog_comment_parent_id_foreign` (`parent_id`)`
- `KEY `tbl_blog_comment_blog_id_parent_id_index` (`blog_id`,`parent_id`)`
- `KEY `tbl_blog_comment_blog_id_created_at_index` (`blog_id`,`created_at`)`
- `CONSTRAINT `tbl_blog_comment_blog_id_foreign` FOREIGN KEY (`blog_id`) REFERENCES `tbl_blog` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_blog_comment_parent_id_foreign` FOREIGN KEY (`parent_id`) REFERENCES `tbl_blog_comment` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_blog_comment_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `tbl_checklist_items` (7 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `name` | varchar(100) | tidak | - |
| `description` | varchar(255) | ya | NULL |
| `sort_order` | int | tidak | '0' |
| `context` | varchar(30) | tidak | 'pekerjaan' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_checklist_items_context_index` (`context`)`

### `tbl_desa` (10 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `n_desa` | varchar(100) | ya | NULL |
| `luas` | double | ya | NULL |
| `jumlah_penduduk` | int | ya | NULL |
| `jumlah_kk` | int unsigned | ya | NULL |
| `target` | int | tidak | '0' |
| `bjp_master` | int | tidak | '0' |
| `kecamatan_id` | int | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `idx_desa_n_desa` (`n_desa`)`
- `KEY `idx_desa_kecamatan_id` (`kecamatan_id`)`
- `KEY `idx_desa_name` (`n_desa`)`
- `FULLTEXT KEY `ft_desa_search` (`n_desa`)`

### `tbl_document_logs` (11 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `type` | varchar(255) | tidak | - |
| `year` | int | tidak | - |
| `sequence_number` | int | tidak | - |
| `full_number` | varchar(255) | tidak | - |
| `id_pekerjaan` | bigint unsigned | ya | NULL |
| `id_user` | bigint unsigned | ya | NULL |
| `status` | enum('active','canceled') | tidak | 'active' |
| `cancel_reason` | text | ya | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_document_logs_id_pekerjaan_foreign` (`id_pekerjaan`)`
- `KEY `tbl_document_logs_id_user_foreign` (`id_user`)`
- `CONSTRAINT `tbl_document_logs_id_pekerjaan_foreign` FOREIGN KEY (`id_pekerjaan`) REFERENCES `tbl_pekerjaan` (`id`)`
- `CONSTRAINT `tbl_document_logs_id_user_foreign` FOREIGN KEY (`id_user`) REFERENCES `users` (`id`)`

### `tbl_document_registers` (13 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `kontrak_id` | bigint unsigned | tidak | - |
| `type_id` | bigint unsigned | tidak | - |
| `addendum_id` | bigint unsigned | ya | NULL |
| `attachment_type` | varchar(255) | ya | NULL |
| `nomor` | varchar(255) | tidak | - |
| `tanggal` | date | tidak | - |
| `sequence_number` | int | tidak | - |
| `year` | int | tidak | - |
| `description` | text | ya | - |
| `nilai` | decimal(18,2) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_document_registers_nomor_unique` (`nomor`)`
- `KEY `tbl_document_registers_type_id_foreign` (`type_id`)`
- `KEY `tbl_document_registers_year_index` (`year`)`
- `KEY `tbl_document_registers_kontrak_id_index` (`kontrak_id`)`
- `KEY `tbl_document_registers_addendum_id_foreign` (`addendum_id`)`
- `FULLTEXT KEY `ft_document_registers_search` (`nomor`,`description`)`
- `CONSTRAINT `tbl_document_registers_addendum_id_foreign` FOREIGN KEY (`addendum_id`) REFERENCES `tbl_kontrak_addendums` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_document_registers_kontrak_id_foreign` FOREIGN KEY (`kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_document_registers_type_id_foreign` FOREIGN KEY (`type_id`) REFERENCES `tbl_document_types` (`id`) ON DELETE CASCADE`

### `tbl_document_sequences` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `year` | int | tidak | - |
| `type` | varchar(255) | tidak | 'global' |
| `last_number` | int | tidak | '0' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_document_sequences_year_type_unique` (`year`,`type`)`

### `tbl_document_types` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `name` | varchar(255) | tidak | - |
| `code` | varchar(255) | tidak | - |
| `format_template` | varchar(255) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_document_types_code_unique` (`code`)`

### `tbl_draft_pekerjaan` (9 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `penyedia_id` | bigint unsigned | ya | NULL |
| `kode_rup` | varchar(255) | ya | NULL |
| `kode_paket` | varchar(255) | ya | NULL |
| `nama_pelaksana` | varchar(255) | ya | NULL |
| `nama_penyedia` | varchar(255) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_draft_pekerjaan_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `KEY `tbl_draft_pekerjaan_penyedia_id_foreign` (`penyedia_id`)`
- `CONSTRAINT `tbl_draft_pekerjaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_draft_pekerjaan_penyedia_id_foreign` FOREIGN KEY (`penyedia_id`) REFERENCES `tbl_penyedia` (`id`) ON DELETE SET NULL`

### `tbl_events` (15 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `title` | varchar(255) | tidak | - |
| `is_allday` | tinyint(1) | tidak | '0' |
| `start` | datetime | tidak | - |
| `end` | datetime | tidak | - |
| `category` | varchar(255) | tidak | 'event' |
| `location` | varchar(255) | ya | NULL |
| `description` | text | ya | - |
| `color` | varchar(255) | ya | NULL |
| `bg_color` | varchar(255) | ya | NULL |
| `border_color` | varchar(255) | ya | NULL |
| `attachments` | json | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_events_user_id_foreign` (`user_id`)`
- `CONSTRAINT `tbl_events_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `tbl_foto` (11 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | ya | NULL |
| `komponen_id` | bigint unsigned | tidak | - |
| `penerima_id` | bigint unsigned | ya | NULL |
| `unit_index` | int | ya | NULL |
| `keterangan` | enum('0%','25%','50%','75%','100%') | tidak | - |
| `koordinat` | varchar(255) | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |
| `validasi_koordinat` | tinyint(1) | tidak | '0' |
| `validasi_koordinat_message` | varchar(255) | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_foto_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `KEY `tbl_foto_komponen_id_foreign` (`komponen_id`)`
- `KEY `tbl_foto_penerima_id_foreign` (`penerima_id`)`
- `KEY `idx_foto_pekerjaan_koordinat` (`pekerjaan_id`,`koordinat`)`
- `KEY `idx_foto_pekerjaan_koordinat_created` (`pekerjaan_id`,`koordinat`,`created_at`)`

### `tbl_kanban_boards` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `slug` | varchar(255) | tidak | - |
| `title` | varchar(255) | tidak | - |
| `description` | text | ya | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_kanban_boards_slug_unique` (`slug`)`

### `tbl_kanban_cards` (14 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `board_id` | bigint unsigned | tidak | - |
| `column_id` | bigint unsigned | tidak | - |
| `position` | int unsigned | tidak | '0' |
| `title` | varchar(255) | tidak | - |
| `description` | text | ya | - |
| `status_label` | varchar(255) | ya | NULL |
| `pekerjaan_id` | bigint unsigned | ya | NULL |
| `tiket_id` | bigint unsigned | ya | NULL |
| `source` | enum('manual','tiket') | tidak | 'manual' |
| `metadata` | json | ya | NULL |
| `created_by` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_kanban_cards_board_id_tiket_id_unique` (`board_id`,`tiket_id`)`
- `KEY `tbl_kanban_cards_column_id_foreign` (`column_id`)`
- `KEY `tbl_kanban_cards_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `KEY `tbl_kanban_cards_tiket_id_foreign` (`tiket_id`)`
- `KEY `tbl_kanban_cards_created_by_foreign` (`created_by`)`
- `CONSTRAINT `tbl_kanban_cards_board_id_foreign` FOREIGN KEY (`board_id`) REFERENCES `tbl_kanban_boards` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_kanban_cards_column_id_foreign` FOREIGN KEY (`column_id`) REFERENCES `tbl_kanban_columns` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_kanban_cards_created_by_foreign` FOREIGN KEY (`created_by`) REFERENCES `users` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_kanban_cards_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_kanban_cards_tiket_id_foreign` FOREIGN KEY (`tiket_id`) REFERENCES `tbl_tiket` (`id`) ON DELETE SET NULL`

### `tbl_kanban_columns` (8 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `board_id` | bigint unsigned | tidak | - |
| `title` | varchar(255) | tidak | - |
| `position` | int unsigned | tidak | '0' |
| `tiket_status` | enum('open','pending','closed') | ya | NULL |
| `color` | varchar(7) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_kanban_columns_board_id_foreign` (`board_id`)`
- `CONSTRAINT `tbl_kanban_columns_board_id_foreign` FOREIGN KEY (`board_id`) REFERENCES `tbl_kanban_boards` (`id`) ON DELETE CASCADE`

### `tbl_kecamatan` (4 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `n_kec` | varchar(255) | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `idx_kecamatan_n_kec` (`n_kec`)`
- `KEY `idx_kecamatan_name` (`n_kec`)`
- `FULLTEXT KEY `ft_kecamatan_search` (`n_kec`)`

### `tbl_kegiatan` (15 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `nama_program` | varchar(255) | ya | NULL |
| `sub_bidang` | varchar(255) | ya | NULL |
| `nama_kegiatan` | varchar(255) | ya | NULL |
| `nama_sub_kegiatan` | varchar(255) | ya | NULL |
| `tahun_anggaran` | varchar(50) | ya | NULL |
| `sumber_dana` | varchar(255) | ya | NULL |
| `pagu` | decimal(15,2) | ya | NULL |
| `kode_rekening` | json | ya | NULL |
| `nama_pptk` | varchar(255) | ya | NULL |
| `nip_pptk` | varchar(255) | ya | NULL |
| `sipd_id_sub_bl` | bigint unsigned | ya | NULL |
| `kode_sub_giat` | varchar(64) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_kegiatan_tahun_anggaran_index` (`tahun_anggaran`)`
- `KEY `tbl_kegiatan_sipd_id_sub_bl_index` (`sipd_id_sub_bl`)`
- `KEY `tbl_kegiatan_kode_sub_giat_index` (`kode_sub_giat`)`
- `FULLTEXT KEY `ft_kegiatan_search` (`nama_kegiatan`,`nama_sub_kegiatan`,`nama_program`)`

### `tbl_kontrak` (23 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `id_kegiatan` | bigint unsigned | ya | '0' |
| `id_pekerjaan` | bigint unsigned | ya | '0' |
| `id_penyedia` | bigint unsigned | ya | NULL |
| `kode_rup` | varchar(50) | ya | NULL |
| `kode_paket` | varchar(50) | ya | NULL |
| `nomor_penawaran` | varchar(50) | ya | NULL |
| `tanggal_penawaran` | date | ya | NULL |
| `nilai_kontrak` | decimal(18,2) | ya | NULL |
| `tgl_sppbj` | date | ya | NULL |
| `tgl_spk` | date | ya | NULL |
| `tgl_spmk` | date | ya | NULL |
| `tgl_selesai` | date | ya | NULL |
| `sppbj` | varchar(50) | ya | '0' |
| `spk` | varchar(50) | ya | '0' |
| `spmk` | varchar(50) | ya | '0' |
| `spse_sppbj_id` | varchar(32) | ya | NULL |
| `spse_spk_id` | varchar(32) | ya | NULL |
| `spse_rekanan_id` | varchar(32) | ya | NULL |
| `spse_pushed_at` | timestamp | ya | NULL |
| `spse_push_log` | json | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `idx_kontrak_pekerjaan_id` (`id_pekerjaan`)`
- `FULLTEXT KEY `ft_kontrak_search` (`spk`,`spmk`,`kode_paket`)`

### `tbl_kontrak_addendums` (20 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `kontrak_id` | bigint unsigned | tidak | - |
| `addendum_ke` | int unsigned | tidak | - |
| `nomor_addendum` | varchar(255) | ya | NULL |
| `attachment_nomors` | json | ya | NULL |
| `tanggal_addendum` | date | tidak | - |
| `jenis_addendum` | enum('teknis','biaya','waktu','teknis_biaya','lainnya') | tidak | 'lainnya' |
| `alasan` | text | ya | - |
| `deskripsi_perubahan` | text | ya | - |
| `nilai_kontrak_sebelum` | decimal(18,2) | ya | NULL |
| `nilai_kontrak_sesudah` | decimal(18,2) | ya | NULL |
| `tgl_selesai_sebelum` | date | ya | NULL |
| `tgl_selesai_sesudah` | date | ya | NULL |
| `status` | enum('draft','diajukan','diproses','disetujui','ditolak') | tidak | 'draft' |
| `kelengkapan_override` | tinyint(1) | tidak | '0' |
| `created_by` | bigint unsigned | ya | NULL |
| `approved_by` | bigint unsigned | ya | NULL |
| `approved_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_kontrak_addendums_kontrak_id_addendum_ke_unique` (`kontrak_id`,`addendum_ke`)`
- `KEY `tbl_kontrak_addendums_created_by_foreign` (`created_by`)`
- `KEY `tbl_kontrak_addendums_approved_by_foreign` (`approved_by`)`
- `KEY `tbl_kontrak_addendums_status_tanggal_addendum_index` (`status`,`tanggal_addendum`)`
- `CONSTRAINT `tbl_kontrak_addendums_approved_by_foreign` FOREIGN KEY (`approved_by`) REFERENCES `users` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_kontrak_addendums_created_by_foreign` FOREIGN KEY (`created_by`) REFERENCES `users` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_kontrak_addendums_kontrak_id_foreign` FOREIGN KEY (`kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE CASCADE`

### `tbl_kontrak_addendum_items` (13 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `addendum_id` | bigint unsigned | tidak | - |
| `nama_item` | varchar(255) | ya | NULL |
| `spesifikasi_sebelum` | text | ya | - |
| `spesifikasi_sesudah` | text | ya | - |
| `volume_sebelum` | decimal(18,4) | ya | NULL |
| `volume_sesudah` | decimal(18,4) | ya | NULL |
| `harga_sebelum` | decimal(18,2) | ya | NULL |
| `harga_sesudah` | decimal(18,2) | ya | NULL |
| `subtotal_sebelum` | decimal(18,2) | ya | NULL |
| `subtotal_sesudah` | decimal(18,2) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_kontrak_addendum_items_addendum_id_foreign` (`addendum_id`)`
- `CONSTRAINT `tbl_kontrak_addendum_items_addendum_id_foreign` FOREIGN KEY (`addendum_id`) REFERENCES `tbl_kontrak_addendums` (`id`) ON DELETE CASCADE`

### `tbl_live_chat_message` (7 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `thread_id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `message` | text | tidak | - |
| `read_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_live_chat_message_user_id_foreign` (`user_id`)`
- `KEY `tbl_live_chat_message_thread_id_id_index` (`thread_id`,`id`)`
- `CONSTRAINT `tbl_live_chat_message_thread_id_foreign` FOREIGN KEY (`thread_id`) REFERENCES `tbl_live_chat_thread` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_live_chat_message_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `tbl_live_chat_thread` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `status` | varchar(20) | tidak | 'open' |
| `last_message_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_live_chat_thread_user_id_unique` (`user_id`)`
- `KEY `tbl_live_chat_thread_status_last_message_at_index` (`status`,`last_message_at`)`
- `CONSTRAINT `tbl_live_chat_thread_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `tbl_output` (8 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `komponen` | varchar(255) | tidak | - |
| `satuan` | varchar(255) | tidak | - |
| `volume` | decimal(10,2) | tidak | - |
| `penerima_is_optional` | tinyint(1) | tidak | '0' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_output_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `FULLTEXT KEY `ft_output_search` (`komponen`,`satuan`)`

### `tbl_pekerjaan` (14 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `kode_rekening` | varchar(225) | ya | '0' |
| `nama_paket` | varchar(225) | tidak | '0' |
| `kecamatan_id` | int | ya | '0' |
| `desa_id` | int | ya | '0' |
| `kegiatan_id` | bigint | ya | NULL |
| `pagu` | float | tidak | '0' |
| `is_konsultan` | tinyint(1) | tidak | '0' |
| `status` | varchar(32) | tidak | 'active' |
| `catatan` | text | ya | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |
| `pengawas_id` | bigint unsigned | ya | NULL |
| `pendamping_id` | bigint unsigned | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `idx_pekerjaan_nama_paket` (`nama_paket`)`
- `KEY `idx_pekerjaan_kecamatan_id` (`kecamatan_id`)`
- `KEY `idx_pekerjaan_desa_id` (`desa_id`)`
- `KEY `idx_pekerjaan_kecamatan_desa` (`kecamatan_id`,`desa_id`)`
- `KEY `idx_pekerjaan_kegiatan_id` (`kegiatan_id`)`
- `KEY `tbl_pekerjaan_pengawas_id_foreign` (`pengawas_id`)`
- `KEY `tbl_pekerjaan_pendamping_id_foreign` (`pendamping_id`)`
- `KEY `tbl_pekerjaan_kegiatan_id_kecamatan_id_index` (`kegiatan_id`,`kecamatan_id`)`
- `FULLTEXT KEY `ft_pekerjaan_search` (`nama_paket`,`kode_rekening`)`
- `CONSTRAINT `tbl_pekerjaan_pendamping_id_foreign` FOREIGN KEY (`pendamping_id`) REFERENCES `pengawas` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_pekerjaan_pengawas_id_foreign` FOREIGN KEY (`pengawas_id`) REFERENCES `pengawas` (`id`) ON DELETE SET NULL`

### `tbl_penerima` (9 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `nama` | varchar(255) | tidak | - |
| `jumlah_jiwa` | int | ya | NULL |
| `nik` | text | ya | - |
| `alamat` | text | ya | - |
| `is_komunal` | tinyint(1) | tidak | '0' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_penerima_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `KEY `tbl_penerima_is_komunal_index` (`is_komunal`)`
- `FULLTEXT KEY `ft_penerima_search` (`nama`,`nik`,`alamat`)`

### `tbl_pengelola` (9 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `unit_spam_id` | bigint unsigned | tidak | - |
| `pokmas` | varchar(255) | ya | NULL |
| `perdes` | varchar(255) | ya | NULL |
| `kepala` | varchar(255) | ya | NULL |
| `bendahara` | varchar(255) | ya | NULL |
| `sekretaris` | varchar(255) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_pengelola_unit_spam_id_unique` (`unit_spam_id`)`
- `CONSTRAINT `tbl_pengelola_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE`

### `tbl_penyedia` (12 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `nama` | varchar(255) | tidak | '0' |
| `direktur` | varchar(255) | tidak | '0' |
| `no_akta` | varchar(255) | tidak | '0' |
| `notaris` | varchar(255) | tidak | '0' |
| `tanggal_akta` | date | ya | NULL |
| `alamat` | varchar(255) | tidak | '0' |
| `npwp` | varchar(32) | ya | NULL |
| `bank` | varchar(255) | ya | '0' |
| `norek` | varchar(255) | ya | '0' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `FULLTEXT KEY `ft_penyedia_search` (`nama`,`direktur`)`

### `tbl_peta_peripaan` (7 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | ya | NULL |
| `nama` | varchar(255) | tidak | - |
| `geojson` | json | ya | NULL |
| `uploaded_by` | bigint unsigned | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_peta_peripaan_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `KEY `tbl_peta_peripaan_uploaded_by_foreign` (`uploaded_by`)`
- `CONSTRAINT `tbl_peta_peripaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_peta_peripaan_uploaded_by_foreign` FOREIGN KEY (`uploaded_by`) REFERENCES `users` (`id`) ON DELETE SET NULL`

### `tbl_procurement_staging_paket` (13 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `sync_run_id` | bigint unsigned | tidak | - |
| `sumber` | varchar(32) | tidak | - |
| `kode_paket` | varchar(32) | tidak | - |
| `nama_paket` | varchar(500) | tidak | - |
| `status_paket` | varchar(128) | ya | NULL |
| `metode_pengadaan` | varchar(128) | ya | NULL |
| `jenis_paket` | varchar(64) | ya | NULL |
| `matched_pekerjaan_id` | bigint unsigned | ya | NULL |
| `matched_kontrak_id` | bigint unsigned | ya | NULL |
| `match_status` | varchar(32) | tidak | 'unmatched' |
| `raw_row` | json | ya | NULL |
| `fetched_at` | timestamp | tidak | CURRENT_TIMESTAMP |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_procurement_staging_paket_sync_run_id_foreign` (`sync_run_id`)`
- `KEY `tbl_procurement_staging_paket_matched_pekerjaan_id_foreign` (`matched_pekerjaan_id`)`
- `KEY `tbl_procurement_staging_paket_matched_kontrak_id_foreign` (`matched_kontrak_id`)`
- `KEY `tbl_procurement_staging_paket_kode_paket_index` (`kode_paket`)`
- `CONSTRAINT `tbl_procurement_staging_paket_matched_kontrak_id_foreign` FOREIGN KEY (`matched_kontrak_id`) REFERENCES `tbl_kontrak` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_procurement_staging_paket_matched_pekerjaan_id_foreign` FOREIGN KEY (`matched_pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_procurement_staging_paket_sync_run_id_foreign` FOREIGN KEY (`sync_run_id`) REFERENCES `tbl_procurement_sync_runs` (`id`) ON DELETE CASCADE`

### `tbl_procurement_sync_runs` (8 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | ya | NULL |
| `status` | varchar(32) | tidak | 'running' |
| `item_count` | int unsigned | tidak | '0' |
| `matched_count` | int unsigned | tidak | '0' |
| `error_log` | text | ya | - |
| `started_at` | timestamp | tidak | CURRENT_TIMESTAMP |
| `finished_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_procurement_sync_runs_user_id_foreign` (`user_id`)`
- `CONSTRAINT `tbl_procurement_sync_runs_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE SET NULL`

### `tbl_progress` (5 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `content` | json | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_progress_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `CONSTRAINT `tbl_progress_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`

### `tbl_sipd_pekerjaan_links` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `id_sub_bl` | bigint unsigned | tidak | - |
| `id_rinci_sub_bl` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_sipd_pekerjaan_links_id_sub_bl_id_rinci_sub_bl_unique` (`id_sub_bl`,`id_rinci_sub_bl`)`
- `KEY `tbl_sipd_pekerjaan_links_pekerjaan_id_index` (`pekerjaan_id`)`

### `tbl_spam_achievements` (11 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `unit_spam_id` | bigint unsigned | tidak | - |
| `tahun` | varchar(255) | tidak | - |
| `jumlah_sr` | int | tidak | '0' |
| `jumlah_kk` | int | tidak | '0' |
| `jumlah_jiwa` | int | tidak | '0' |
| `jumlah_bjp_kk` | int | tidak | '0' |
| `jumlah_bjp_jiwa` | int | tidak | '0' |
| `catatan` | text | ya | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `spam_unit_tahun_unique` (`unit_spam_id`,`tahun`)`
- `CONSTRAINT `tbl_spam_achievements_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE`

### `tbl_spam_budgets` (8 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `unit_spam_id` | bigint unsigned | tidak | - |
| `nilai_kontrak` | double | tidak | - |
| `tahun` | varchar(4) | tidak | - |
| `nama_paket` | varchar(255) | tidak | - |
| `sumber_dana` | varchar(50) | tidak | 'APBD' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_spam_budgets_unit_spam_id_foreign` (`unit_spam_id`)`
- `CONSTRAINT `tbl_spam_budgets_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE`

### `tbl_spam_kelembagaan_raw` (34 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `jenis_jaringan` | varchar(10) | tidak | - |
| `kecamatan` | varchar(255) | ya | NULL |
| `desa_kelurahan` | varchar(255) | ya | NULL |
| `desa_kelurahan_normalized` | varchar(255) | ya | NULL |
| `lokasi_key` | varchar(255) | ya | NULL |
| `tahun_pembangunan_raw` | varchar(100) | ya | NULL |
| `tahun_pembangunan_awal` | smallint | ya | NULL |
| `tahun_pembangunan_akhir` | smallint | ya | NULL |
| `sumber_dana_raw` | varchar(255) | ya | NULL |
| `program_pembangunan` | text | ya | - |
| `nama_pengelola` | varchar(255) | ya | NULL |
| `perdes_pembentukan_pokmas` | varchar(255) | ya | NULL |
| `pengurus_kepala` | varchar(255) | ya | NULL |
| `pengurus_bendahara` | varchar(255) | ya | NULL |
| `pengurus_sekretaris` | varchar(255) | ya | NULL |
| `kapasitas_mata_air_l_det` | decimal(12,2) | ya | NULL |
| `sistem_aliran` | varchar(255) | ya | NULL |
| `kapasitas_air_tanah_l_det` | decimal(12,2) | ya | NULL |
| `kapasitas_lain_l_det` | decimal(12,2) | ya | NULL |
| `dasar_hukum_tarif` | varchar(255) | ya | NULL |
| `besaran_iuran` | varchar(255) | ya | NULL |
| `pendapatan_bulanan_rp` | decimal(18,2) | ya | NULL |
| `biaya_operasional_bulanan_rp` | decimal(18,2) | ya | NULL |
| `sr_unit` | int | ya | NULL |
| `kk_terlayani` | int | ya | NULL |
| `jiwa_terlayani` | int | ya | NULL |
| `target_layanan` | int | ya | NULL |
| `raw_payload` | json | ya | NULL |
| `source_file` | varchar(255) | ya | NULL |
| `source_sheet` | varchar(100) | ya | NULL |
| `source_row` | int unsigned | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_spam_kelembagaan_raw_source_sheet_source_row_index` (`source_sheet`,`source_row`)`
- `KEY `tbl_spam_kelembagaan_raw_jenis_jaringan_index` (`jenis_jaringan`)`
- `KEY `tbl_spam_kelembagaan_raw_kecamatan_index` (`kecamatan`)`
- `KEY `tbl_spam_kelembagaan_raw_desa_kelurahan_index` (`desa_kelurahan`)`
- `KEY `tbl_spam_kelembagaan_raw_lokasi_key_index` (`lokasi_key`)`
- `KEY `tbl_spam_kelembagaan_raw_tahun_pembangunan_awal_index` (`tahun_pembangunan_awal`)`
- `KEY `tbl_spam_kelembagaan_raw_sumber_dana_raw_index` (`sumber_dana_raw`)`
- `KEY `tbl_spam_kelembagaan_raw_nama_pengelola_index` (`nama_pengelola`)`

### `tbl_spam_terbangun_raw` (32 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `kecamatan` | varchar(255) | ya | NULL |
| `jenis_wilayah` | varchar(30) | ya | NULL |
| `desa_kelurahan` | varchar(255) | ya | NULL |
| `nama_pengelola` | varchar(255) | ya | NULL |
| `sumber_air_baku` | varchar(255) | ya | NULL |
| `sistem_aliran` | varchar(255) | ya | NULL |
| `debit_sumber_l_det` | decimal(12,2) | ya | NULL |
| `debit_diambil_l_det` | decimal(12,2) | ya | NULL |
| `penduduk_terlayani` | int | ya | NULL |
| `jumlah_penduduk` | int | ya | NULL |
| `hu_ku_unit` | int | ya | NULL |
| `sr_unit` | int | ya | NULL |
| `tanpa_meteran_air_unit` | int | ya | NULL |
| `sumber_dana_raw` | varchar(255) | ya | NULL |
| `asal_proyek` | varchar(255) | ya | NULL |
| `nilai_dak_apbn_rp` | decimal(18,2) | ya | NULL |
| `nilai_apbd_rp` | decimal(18,2) | ya | NULL |
| `nilai_banprov_rp` | decimal(18,2) | ya | NULL |
| `tahun_pembangunan_raw` | varchar(50) | ya | NULL |
| `tahun_pembangunan_awal` | smallint | ya | NULL |
| `tahun_pembangunan_akhir` | smallint | ya | NULL |
| `kondisi_raw` | varchar(100) | ya | NULL |
| `kondisi_normalized` | varchar(50) | ya | NULL |
| `tanggal_terakhir_laporan` | date | ya | NULL |
| `keterangan` | text | ya | - |
| `raw_payload` | json | ya | NULL |
| `source_file` | varchar(255) | ya | NULL |
| `source_sheet` | varchar(100) | ya | NULL |
| `source_row` | int unsigned | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_spam_terbangun_raw_source_sheet_source_row_index` (`source_sheet`,`source_row`)`
- `KEY `tbl_spam_terbangun_raw_kecamatan_index` (`kecamatan`)`
- `KEY `tbl_spam_terbangun_raw_desa_kelurahan_index` (`desa_kelurahan`)`
- `KEY `tbl_spam_terbangun_raw_sumber_dana_raw_index` (`sumber_dana_raw`)`
- `KEY `tbl_spam_terbangun_raw_tahun_pembangunan_raw_index` (`tahun_pembangunan_raw`)`
- `KEY `tbl_spam_terbangun_raw_tahun_pembangunan_awal_index` (`tahun_pembangunan_awal`)`
- `KEY `tbl_spam_terbangun_raw_kondisi_normalized_index` (`kondisi_normalized`)`

### `tbl_spm_sanitasi` (40 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `jenis` | varchar(30) | tidak | - |
| `desa_id` | bigint unsigned | ya | NULL |
| `skala_pelayanan` | varchar(255) | ya | NULL |
| `nama_infrastruktur` | varchar(255) | tidak | - |
| `latitude` | decimal(11,8) | ya | NULL |
| `longitude` | decimal(11,8) | ya | NULL |
| `alamat_lengkap` | text | ya | - |
| `jumlah_pemanfaat_kk` | int unsigned | ya | NULL |
| `jumlah_pemanfaat_jiwa` | int unsigned | ya | NULL |
| `tahun_konstruksi` | smallint unsigned | ya | NULL |
| `pembiayaan_apbn` | decimal(18,2) | ya | NULL |
| `pembiayaan_apbd` | decimal(18,2) | ya | NULL |
| `pembiayaan_dak` | decimal(18,2) | ya | NULL |
| `pembiayaan_hibah` | decimal(18,2) | ya | NULL |
| `pembiayaan_csr` | decimal(18,2) | ya | NULL |
| `pembiayaan_lain` | decimal(18,2) | ya | NULL |
| `pembiayaan_total` | decimal(18,2) | ya | NULL |
| `status_keberfungsian` | varchar(255) | ya | NULL |
| `kualitas_keberfungsian` | varchar(255) | ya | NULL |
| `pengelola` | varchar(255) | ya | NULL |
| `kapasitas_desain` | decimal(12,2) | ya | NULL |
| `kapasitas_terpakai` | decimal(12,2) | ya | NULL |
| `kapasitas_tidak_terpakai` | decimal(12,2) | ya | NULL |
| `jenis_pengolahan` | varchar(255) | ya | NULL |
| `peta_cakupan` | varchar(255) | ya | NULL |
| `status_lahan` | varchar(255) | ya | NULL |
| `luas_lahan_ha` | varchar(255) | ya | NULL |
| `opsi_teknologi` | varchar(255) | ya | NULL |
| `jumlah_stasiun_pompa` | varchar(255) | ya | NULL |
| `biaya_operasional` | decimal(18,2) | ya | NULL |
| `jenis_pengelola` | varchar(255) | ya | NULL |
| `sistem_pengolahan` | varchar(255) | ya | NULL |
| `truk_tinja_unit` | smallint unsigned | ya | NULL |
| `kapasitas_truk_m3` | decimal(10,2) | ya | NULL |
| `jumlah_ritasi` | smallint unsigned | ya | NULL |
| `jarak_maksimal_pelayanan_km` | decimal(10,2) | ya | NULL |
| `alokasi_biaya_operasional` | decimal(18,2) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_spm_sanitasi_desa_id_foreign` (`desa_id`)`
- `KEY `tbl_spm_sanitasi_jenis_desa_id_index` (`jenis`,`desa_id`)`
- `KEY `tbl_spm_sanitasi_tahun_konstruksi_index` (`tahun_konstruksi`)`
- `CONSTRAINT `tbl_spm_sanitasi_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE SET NULL`

### `tbl_spm_sanitasi_pekerjaan` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `spm_sanitasi_id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `output_id` | bigint unsigned | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `spm_sanitasi_pekerjaan_unique` (`spm_sanitasi_id`,`pekerjaan_id`)`
- `KEY `tbl_spm_sanitasi_pekerjaan_output_id_foreign` (`output_id`)`
- `KEY `tbl_spm_sanitasi_pekerjaan_pekerjaan_id_index` (`pekerjaan_id`)`
- `CONSTRAINT `tbl_spm_sanitasi_pekerjaan_output_id_foreign` FOREIGN KEY (`output_id`) REFERENCES `tbl_output` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_spm_sanitasi_pekerjaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_spm_sanitasi_pekerjaan_spm_sanitasi_id_foreign` FOREIGN KEY (`spm_sanitasi_id`) REFERENCES `tbl_spm_sanitasi` (`id`) ON DELETE CASCADE`

### `tbl_spse_sessions` (9 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `encrypted_cookies` | text | tidak | - |
| `lpse_slug` | varchar(64) | tidak | 'cianjurkab' |
| `expires_at` | timestamp | ya | NULL |
| `last_validated_at` | timestamp | ya | NULL |
| `is_active` | tinyint(1) | tidak | '1' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_spse_sessions_user_id_is_active_index` (`user_id`,`is_active`)`
- `CONSTRAINT `tbl_spse_sessions_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `tbl_survey_lokasi` (17 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `tugas_id` | bigint unsigned | ya | NULL |
| `jenis` | enum('spam_perpipaan','spam_pengeboran','mck_individu','mck_komunal') | tidak | - |
| `nama_lokasi` | varchar(255) | tidak | - |
| `kecamatan_id` | bigint unsigned | ya | NULL |
| `desa_id` | bigint unsigned | ya | NULL |
| `alamat` | text | ya | - |
| `latitude` | decimal(10,7) | ya | NULL |
| `longitude` | decimal(10,7) | ya | NULL |
| `detail` | json | ya | NULL |
| `status` | enum('diajukan','diverifikasi','ditolak') | tidak | 'diajukan' |
| `catatan_verifikasi` | text | ya | - |
| `verified_by` | bigint unsigned | ya | NULL |
| `verified_at` | timestamp | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_survey_lokasi_user_id_foreign` (`user_id`)`
- `KEY `tbl_survey_lokasi_kecamatan_id_foreign` (`kecamatan_id`)`
- `KEY `tbl_survey_lokasi_desa_id_foreign` (`desa_id`)`
- `KEY `tbl_survey_lokasi_verified_by_foreign` (`verified_by`)`
- `KEY `tbl_survey_lokasi_tugas_id_foreign` (`tugas_id`)`
- `CONSTRAINT `tbl_survey_lokasi_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_survey_lokasi_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_survey_lokasi_tugas_id_foreign` FOREIGN KEY (`tugas_id`) REFERENCES `tbl_survey_tugas` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_survey_lokasi_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_survey_lokasi_verified_by_foreign` FOREIGN KEY (`verified_by`) REFERENCES `users` (`id`) ON DELETE SET NULL`

### `tbl_survey_tugas` (15 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | ya | NULL |
| `judul` | varchar(255) | tidak | - |
| `tahun_anggaran` | int | tidak | - |
| `jenis` | enum('spam_perpipaan','spam_pengeboran','mck_individu','mck_komunal') | ya | NULL |
| `kecamatan_id` | bigint unsigned | ya | NULL |
| `desa_id` | bigint unsigned | ya | NULL |
| `lokasi_catatan` | text | ya | - |
| `assignee_id` | bigint unsigned | ya | NULL |
| `status` | enum('ditugaskan','dikerjakan','selesai') | tidak | 'ditugaskan' |
| `batas_waktu` | date | ya | NULL |
| `catatan_admin` | text | ya | - |
| `created_by` | bigint unsigned | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_survey_tugas_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `KEY `tbl_survey_tugas_kecamatan_id_foreign` (`kecamatan_id`)`
- `KEY `tbl_survey_tugas_desa_id_foreign` (`desa_id`)`
- `KEY `tbl_survey_tugas_created_by_foreign` (`created_by`)`
- `KEY `tbl_survey_tugas_assignee_id_status_index` (`assignee_id`,`status`)`
- `KEY `tbl_survey_tugas_tahun_anggaran_index` (`tahun_anggaran`)`
- `CONSTRAINT `tbl_survey_tugas_assignee_id_foreign` FOREIGN KEY (`assignee_id`) REFERENCES `users` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_survey_tugas_created_by_foreign` FOREIGN KEY (`created_by`) REFERENCES `users` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_survey_tugas_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_survey_tugas_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_survey_tugas_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL`

### `tbl_survey_tugas_assignees` (5 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `survey_tugas_id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tugas_assignee_unique` (`survey_tugas_id`,`user_id`)`
- `KEY `tbl_survey_tugas_assignees_user_id_foreign` (`user_id`)`
- `CONSTRAINT `tbl_survey_tugas_assignees_survey_tugas_id_foreign` FOREIGN KEY (`survey_tugas_id`) REFERENCES `tbl_survey_tugas` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_survey_tugas_assignees_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `tbl_tags` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `name` | varchar(100) | tidak | - |
| `slug` | varchar(100) | tidak | - |
| `color` | varchar(7) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `tbl_tags_name_unique` (`name`)`
- `UNIQUE KEY `tbl_tags_slug_unique` (`slug`)`

### `tbl_tiket` (11 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | ya | NULL |
| `subjek` | varchar(255) | tidak | - |
| `deskripsi` | text | tidak | - |
| `kategori` | enum('bug','request','lapangan','document','other') | ya | 'other' |
| `prioritas` | enum('low','medium','high') | tidak | 'medium' |
| `status` | enum('open','pending','closed') | tidak | 'open' |
| `admin_notes` | text | ya | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_tiket_user_id_foreign` (`user_id`)`
- `KEY `tbl_tiket_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `CONSTRAINT `tbl_tiket_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_tiket_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `tbl_tiket_comment` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `tiket_id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `message` | text | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_tiket_comment_tiket_id_foreign` (`tiket_id`)`
- `KEY `tbl_tiket_comment_user_id_foreign` (`user_id`)`
- `CONSTRAINT `tbl_tiket_comment_tiket_id_foreign` FOREIGN KEY (`tiket_id`) REFERENCES `tbl_tiket` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_tiket_comment_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `tbl_unit_checklists` (6 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `unit_spam_id` | bigint unsigned | tidak | - |
| `item` | varchar(255) | tidak | - |
| `is_checked` | tinyint(1) | tidak | '0' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_unit_checklists_unit_spam_id_foreign` (`unit_spam_id`)`
- `CONSTRAINT `tbl_unit_checklists_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE`

### `tbl_unit_spam` (17 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `desa_id` | bigint unsigned | tidak | - |
| `name` | varchar(255) | ya | NULL |
| `is_simspam` | tinyint(1) | tidak | '0' |
| `sistem_layanan` | varchar(255) | ya | NULL |
| `sumber_mata_air_kap` | varchar(255) | ya | NULL |
| `sumber_air_tanah_kap` | varchar(255) | ya | NULL |
| `lain_lain_kap` | varchar(255) | ya | NULL |
| `tahun_pembangunan` | varchar(10) | ya | NULL |
| `sumber_dana` | varchar(255) | ya | NULL |
| `program` | varchar(255) | ya | NULL |
| `tarif_dasar_hukum` | varchar(255) | ya | NULL |
| `iuran_nominal` | varchar(255) | ya | NULL |
| `pendapatan_bulan` | varchar(255) | ya | NULL |
| `biaya_operasional` | varchar(255) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_unit_spam_desa_id_foreign` (`desa_id`)`
- `CONSTRAINT `tbl_unit_spam_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`) ON DELETE CASCADE`

### `tbl_unit_spam_pekerjaan` (7 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `unit_spam_id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `output_id` | bigint unsigned | ya | NULL |
| `capaian_metric` | varchar(8) | tidak | 'jp' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `unit_spam_pekerjaan_unique` (`unit_spam_id`,`pekerjaan_id`)`
- `KEY `tbl_unit_spam_pekerjaan_output_id_foreign` (`output_id`)`
- `KEY `tbl_unit_spam_pekerjaan_pekerjaan_id_index` (`pekerjaan_id`)`
- `CONSTRAINT `tbl_unit_spam_pekerjaan_output_id_foreign` FOREIGN KEY (`output_id`) REFERENCES `tbl_output` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tbl_unit_spam_pekerjaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `tbl_unit_spam_pekerjaan_unit_spam_id_foreign` FOREIGN KEY (`unit_spam_id`) REFERENCES `tbl_unit_spam` (`id`) ON DELETE CASCADE`

### `tbl_usulan_kegiatan` (13 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `sub_bidang` | enum('air minum','sanitasi') | tidak | - |
| `nama_pengusul` | varchar(255) | tidak | - |
| `kecamatan_id` | bigint unsigned | tidak | - |
| `desa_id` | bigint unsigned | tidak | - |
| `perihal` | varchar(255) | tidak | - |
| `ringkasan` | text | tidak | - |
| `tanggal_surat_masuk` | date | tidak | - |
| `nomor_surat_masuk` | varchar(100) | tidak | - |
| `tanggal_surat` | date | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tbl_usulan_kegiatan_user_id_foreign` (`user_id`)`
- `KEY `tbl_usulan_kegiatan_kecamatan_id_foreign` (`kecamatan_id`)`
- `KEY `tbl_usulan_kegiatan_desa_id_foreign` (`desa_id`)`
- `CONSTRAINT `tbl_usulan_kegiatan_desa_id_foreign` FOREIGN KEY (`desa_id`) REFERENCES `tbl_desa` (`id`)`
- `CONSTRAINT `tbl_usulan_kegiatan_kecamatan_id_foreign` FOREIGN KEY (`kecamatan_id`) REFERENCES `tbl_kecamatan` (`id`)`
- `CONSTRAINT `tbl_usulan_kegiatan_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `tool_pdfs` (9 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `parent_id` | bigint unsigned | ya | NULL |
| `name` | varchar(255) | tidak | - |
| `original_filename` | varchar(255) | ya | NULL |
| `kind` | varchar(20) | tidak | 'source' |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |
| `deleted_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tool_pdfs_parent_id_foreign` (`parent_id`)`
- `KEY `tool_pdfs_user_id_kind_index` (`user_id`,`kind`)`
- `CONSTRAINT `tool_pdfs_parent_id_foreign` FOREIGN KEY (`parent_id`) REFERENCES `tool_pdfs` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `tool_pdfs_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `tool_pdf_signature_placements` (18 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `tool_pdf_id` | bigint unsigned | tidak | - |
| `signature_id` | char(36) | tidak | - |
| `page_number` | int unsigned | tidak | - |
| `x_ratio` | decimal(8,6) | tidak | - |
| `y_ratio` | decimal(8,6) | tidak | - |
| `scale` | decimal(8,6) | tidak | - |
| `sort_order` | int unsigned | tidak | '0' |
| `signature_name` | varchar(255) | tidak | - |
| `signature_file_name` | varchar(255) | tidak | - |
| `signature_mime_type` | varchar(100) | tidak | - |
| `signature_width` | int unsigned | tidak | - |
| `signature_height` | int unsigned | tidak | - |
| `signature_data_url` | longtext | ya | - |
| `signature_source_type` | enum('upload','library') | ya | NULL |
| `signature_source_id` | varchar(64) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `tool_pdf_signature_placements_tool_pdf_id_page_number_index` (`tool_pdf_id`,`page_number`)`
- `CONSTRAINT `tool_pdf_signature_placements_tool_pdf_id_foreign` FOREIGN KEY (`tool_pdf_id`) REFERENCES `tool_pdfs` (`id`) ON DELETE CASCADE`

### `users` (13 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `google_id` | varchar(255) | ya | NULL |
| `name` | varchar(255) | tidak | - |
| `email` | varchar(255) | tidak | - |
| `avatar` | varchar(255) | ya | NULL |
| `gender` | varchar(20) | ya | NULL |
| `nip` | varchar(255) | ya | NULL |
| `jabatan` | varchar(255) | ya | NULL |
| `email_verified_at` | timestamp | ya | NULL |
| `password` | varchar(255) | ya | NULL |
| `remember_token` | varchar(100) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `users_email_unique` (`email`)`
- `UNIQUE KEY `users_google_id_unique` (`google_id`)`

### `user_drive_items` (9 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `parent_id` | bigint unsigned | ya | NULL |
| `name` | varchar(255) | tidak | - |
| `kind` | varchar(20) | tidak | - |
| `original_filename` | varchar(255) | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |
| `deleted_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `KEY `user_drive_items_parent_id_foreign` (`parent_id`)`
- `KEY `user_drive_items_user_id_parent_id_kind_index` (`user_id`,`parent_id`,`kind`)`
- `CONSTRAINT `user_drive_items_parent_id_foreign` FOREIGN KEY (`parent_id`) REFERENCES `user_drive_items` (`id`) ON DELETE SET NULL`
- `CONSTRAINT `user_drive_items_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `user_drive_shares` (5 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `item_id` | bigint unsigned | tidak | - |
| `shared_to_user_id` | bigint unsigned | ya | NULL |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `user_drive_shares_item_id_shared_to_user_id_unique` (`item_id`,`shared_to_user_id`)`
- `KEY `user_drive_shares_shared_to_user_id_foreign` (`shared_to_user_id`)`
- `CONSTRAINT `user_drive_shares_item_id_foreign` FOREIGN KEY (`item_id`) REFERENCES `user_drive_items` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `user_drive_shares_shared_to_user_id_foreign` FOREIGN KEY (`shared_to_user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

### `user_pekerjaan` (5 kolom)

| Kolom | Tipe | Null | Default |
| --- | --- | --- | --- |
| `id` | bigint unsigned | tidak | - |
| `user_id` | bigint unsigned | tidak | - |
| `pekerjaan_id` | bigint unsigned | tidak | - |
| `created_at` | timestamp | ya | NULL |
| `updated_at` | timestamp | ya | NULL |

- `PRIMARY KEY (`id`)`
- `UNIQUE KEY `user_pekerjaan_user_id_pekerjaan_id_unique` (`user_id`,`pekerjaan_id`)`
- `KEY `user_pekerjaan_pekerjaan_id_foreign` (`pekerjaan_id`)`
- `CONSTRAINT `user_pekerjaan_pekerjaan_id_foreign` FOREIGN KEY (`pekerjaan_id`) REFERENCES `tbl_pekerjaan` (`id`) ON DELETE CASCADE`
- `CONSTRAINT `user_pekerjaan_user_id_foreign` FOREIGN KEY (`user_id`) REFERENCES `users` (`id`) ON DELETE CASCADE`

