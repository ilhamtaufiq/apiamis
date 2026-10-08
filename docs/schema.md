# Inventaris Skema Database (Fase 0.3)

Sumber: `database/migrations/*.php` (133 file, branch `rust`, dari `main` SHA `c19fd06`). Disusun dengan parser statis dari urutan migrasi, bukan dari `information_schema`.
**Perlu verifikasi** dengan `information_schema.COLUMNS` di staging sebelum dipakai sebagai acuan. Migrasi yang memakai raw SQL, `DB::statement`, atau logika kondisional tidak ikut terbaca.

> **Peringatan: dokumen ini tidak lengkap.** Parser hanya membaca 33 tabel dari migrasi, sementara model merujuk sekitar 80 tabel. Tabel inti seperti `tbl_pekerjaan` dan `tbl_tiket` dibuat di luar migrasi di repo ini (kemungkinan dari dump database lama), sehingga kolomnya tidak terbaca di sini. Sumber kebenaran skema harus diambil dari database staging.

## Cara melengkapi (langkah 0.3 belum selesai)

1. Di staging, jalankan: `mysqldump --no-data --skip-comments apiamis > schema_staging.sql`
2. Ganti bagian "Daftar tabel dan kolom" di dokumen ini dengan hasil `information_schema.COLUMNS` (kolom, tipe, nullable, default, key).
3. Tandai kolom uang, JSON, dan soft delete dari hasil tersebut, lalu periksa ulang bagian "Kolom yang perlu perhatian khusus".

Daftar tabel yang dipakai model tetapi tidak terbaca dari migrasi (perlu dikonfirmasi di staging):
`app_settings`, `chat_knowledge_cache`, `panduan_pages`, `pekerjaan_checklist_histories`, `pekerjaan_progress_estimasi`, `puspen_progress_fisik_output`, `signature_libraries`, `simulation_network_versions`, `simulation_networks`, `sk`, `spam_kelembagaan_share_links`, `spam_kelembagaan_submissions`, `tbl_audit_logs`, `tbl_berita_acara`, `tbl_blog_comment`, `tbl_document_types`, `tbl_kanban_boards`, `tbl_kanban_cards`, `tbl_kanban_columns`, `tbl_kontrak_addendum_items`, `tbl_live_chat_message`, `tbl_live_chat_thread`, `tbl_pengelola`, `tbl_peta_peripaan`, `tbl_procurement_staging_paket`, `tbl_procurement_sync_runs`, `tbl_progress`, `tbl_sipd_pekerjaan_links`, `tbl_spse_sessions`, `tbl_survey_tugas`, `tbl_tags`, `tbl_tiket`, `tbl_tiket_comment`, `tbl_unit_checklists`, `tool_pdf_signature_placements`, `tool_pdfs`, `pengawas`, `kegiatan_role`.

Catatan: beberapa nama di atas bisa berupa hasil parser yang keliru (misalnya nama kolom terenkripsi). Konfirmasi dengan `SHOW TABLES` di staging.

## Ringkasan

- Tabel terbaca dari migrasi: **33**
- Kolom terbaca: **43**
- File migrasi tanpa `Schema::` (cek manual): 2025_12_26_080934_add_calendar_to_menu_permissions.php, 2025_12_26_160000_register_map_menu_permission.php, 2026_05_10_145032_update_kategori_enum_in_tbl_tiket.php, 2026_05_10_145521_add_document_to_kategori_enum_in_tbl_tiket.php, 2026_05_24_170000_add_biaya_pembangunan_to_tbl_unit_spam.php, 2026_06_28_000001_add_spm_detail_page_active_setting.php, 2026_07_14_120000_add_capaian_publik_section_active_setting.php, 2026_08_17_000002_register_pengaturan_sk_menu_permission.php, 2026_10_07_000001_update_jenis_enum_survey.php

## Kolom yang perlu perhatian khusus

Kolom uang, JSON, tanggal, dan kolom soft delete. Kolom terenkripsi ada di `models.md`.

| Tabel | Kolom | Tipe | Alasan |
| --- | --- | --- | --- |
| `media` | `manipulations` | json | JSON: validasi struktur |
| `media` | `custom_properties` | json | JSON: validasi struktur |
| `media` | `generated_conversions` | json | JSON: validasi struktur |
| `media` | `responsive_images` | json | JSON: validasi struktur |
| `tbl_kontrak` | `nilai_kontrak` | float NULL | uang/presisi: pakai rust_decimal |
| `tbl_desa` | `n_desa` | fullText | full-text index |
| `tbl_kecamatan` | `n_kec` | fullText | full-text index |

## Daftar tabel dan kolom

### `media` (13 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `model` | morphs |
| `collection_name` | string |
| `name` | string |
| `file_name` | string |
| `mime_type` | string NULL |
| `disk` | string |
| `conversions_disk` | string NULL |
| `size` | unsignedBigInteger |
| `manipulations` | json |
| `custom_properties` | json |
| `generated_conversions` | json |
| `responsive_images` | json |
| `order_column` | unsignedInteger NULL |

### `users` (1 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `password` | string NULL |

### `tbl_pekerjaan` (1 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `ft_pekerjaan_search` | dropFullText |

### `tbl_draft_pekerjaan` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_kontrak` (2 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `ft_kontrak_search` | dropFullText |
| `nilai_kontrak` | float NULL |

### `tbl_penyedia` (1 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `ft_penyedia_search` | dropFullText |

### `tbl_kegiatan` (1 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `ft_kegiatan_search` | dropFullText |

### `tbl_desa` (2 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `n_desa` | fullText |
| `ft_desa_search` | dropFullText |

### `tbl_kecamatan` (2 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `n_kec` | fullText |
| `ft_kecamatan_search` | dropFullText |

### `tbl_penerima` (3 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `ft_penerima_search` | dropFullText |
| `nik` | string NULL |
| `alamat` | string NULL |

### `tbl_output` (1 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `ft_output_search` | dropFullText |

### `search_tables` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_document_sequences` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_document_registers` (2 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `ft_document_registers_search` | dropFullText |
| `addendum_id` | dropConstrainedForeignId |

### `tbl_events` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_foto` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_blog` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_blog_assets` (1 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `blog_id` | dropConstrainedForeignId |

### `tbl_unit_spam` (8 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `sumber_dana` | string NULL |
| `program` | string NULL |
| `tarif_dasar_hukum` | string NULL |
| `iuran_nominal` | string NULL |
| `biaya_operasional` | string NULL |
| `biaya_pembangunan` | string NULL |
| `tahun_pembangunan` | string NULL |
| `pendapatan_bulan` | string NULL |

### `chat_sessions` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `chat_messages` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_checklist_items` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_unit_spam_pekerjaan` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `puspen_progress_fisik` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_berkas` (1 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `uploaded_by` | dropConstrainedForeignId |

### `master_fase_pekerjaans` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `pekerjaan_progress_estimasi_history` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_usulan_kegiatan` (3 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `tanggal_surat_masuk` | date NULL |
| `nomor_surat_masuk` | string NULL |
| `tanggal_surat` | date NULL |

### `tbl_kontrak_addendums` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_survey_lokasi` (1 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |
| `tugas_id` | dropConstrainedForeignId |

### `tbl_spam_achievements` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_spam_budgets` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

### `tbl_spm_sanitasi` (0 kolom)

| Kolom | Tipe (dari migrasi) |
| --- | --- |

