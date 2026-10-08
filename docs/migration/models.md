# Inventaris Model (Fase 0.2)

Sumber: `app/Models/*.php` (branch `rust`, dari `main` SHA `c19fd06`). Dibuat dengan parser statis, bukan dari runtime Eloquent. Perlu verifikasi untuk model yang punya logika dinamis.

## Ringkasan

- Jumlah model: **77**
- Model dengan `SoftDeletes`: **6**
- Model dengan kolom terenkripsi (`encrypted` cast): **3**
- Model dengan hook/scope/observer (`boot`, `scope*`, `creating`, dll): **14**
- Model dengan `$guarded` (bukan `$fillable`): **1**

## Kolom terenkripsi (wajib diport dekripsinya jika dipakai)

| Model | Kolom | Cast |
| --- | --- | --- |
| Penerima | `nik` | `encrypted` |
| Penerima | `alamat` | `encrypted` |
| SpseSession | `encrypted_cookies` | `encrypted:array` |

## Hook, scope, dan observer (logika tersembunyi)

| Model | Scope | Hook Eloquent | Catatan |
| --- | --- | --- | --- |
| Desa | RealWilayah | - | perlu dibaca di fase 0 |
| Kecamatan | RealWilayah | - | perlu dibaca di fase 0 |
| Kontrak | LinkedToPekerjaan | - | perlu dibaca di fase 0 |
| PanduanPage | Published, Ordered | - | perlu dibaca di fase 0 |
| Pekerjaan | ByUserRole, NotCanceled, WithKontrak, WithoutKontrak | deleted, deleting | perlu dibaca di fase 0 |
| Penerima | Komunal, SearchNama | - | perlu dibaca di fase 0 |
| PuspenMediaShare | OwnedBy | - | perlu dibaca di fase 0 |
| SignatureLibrary | OwnedBy | - | perlu dibaca di fase 0 |
| SimulationNetwork | OwnedBy, AccessibleBy, ForPekerjaan | - | perlu dibaca di fase 0 |
| SimulationNetworkVersion | - | creating | perlu dibaca di fase 0 |
| SpseSession | ActiveForUser | - | perlu dibaca di fase 0 |
| Tag | - | creating, updating | perlu dibaca di fase 0 |
| ToolPdf | OwnedBy | - | perlu dibaca di fase 0 |
| UserDriveItem | OwnedBy | - | perlu dibaca di fase 0 |

## Daftar model

| Model | Tabel | SoftDeletes | Relasi | Cast non-default | $fillable |
| --- | --- | --- | --- | --- | --- |
| AppSetting | `app_settings` | tidak | - | - | 3 |
| AuditLog | `tbl_audit_logs` | tidak | auditable(morphTo), user(belongsTo) | old_values:array, new_values:array | 9 |
| BeritaAcara | `tbl_berita_acara` | tidak | pekerjaan(belongsTo) | pekerjaan_id:integer, data:array | 2 |
| Berkas | `tbl_berkas` | tidak | pekerjaan(belongsTo), uploader(belongsTo) | - | 3 |
| Blog | `tbl_blog` | tidak | comments(hasMany), user(belongsTo) | is_published:boolean, is_internal:boolean, is_featured:boolean, published_at:datetime, featured_at:datetime | 11 |
| BlogAsset | `tbl_blog_assets` | tidak | blog(belongsTo) | - | 2 |
| BlogComment | `tbl_blog_comment` | ya | blog(belongsTo), parent(belongsTo), replies(hasMany), user(belongsTo) | depth:integer | 5 |
| BroadcastHistory | `(default: broadcast_historys)` | tidak | - | - | 7 |
| ChatKnowledgeCache | `chat_knowledge_cache` | tidak | - | - | 6 |
| ChatMessage | `(default: chat_messages)` | tidak | session(belongsTo) | tool_calls:array, cost_idr:float | 8 |
| ChatSession | `(default: chat_sessions)` | tidak | messages(hasMany), user(belongsTo) | - | 4 |
| ChatUserMemory | `(default: chat_user_memorys)` | tidak | user(belongsTo) | - | 5 |
| ChecklistItem | `tbl_checklist_items` | tidak | pekerjaan(belongsToMany) | sort_order:integer | 4 |
| Desa | `tbl_desa` | tidak | kecamatan(belongsTo), spmSanitasi(hasMany) | jumlah_penduduk:integer, jumlah_kk:integer, target:integer, bjp_master:integer | 7 |
| DocumentRegister | `tbl_document_registers` | tidak | addendum(belongsTo), kontrak(belongsTo), type(belongsTo) | tanggal:date, nilai:float | 10 |
| DocumentType | `tbl_document_types` | tidak | - | - | 3 |
| DraftPekerjaan | `tbl_draft_pekerjaan` | tidak | pekerjaan(belongsTo), penyedia(belongsTo) | - | 5 |
| ErrorLog | `(default: error_logs)` | tidak | user(belongsTo) | metadata:array, resolved_at:datetime | 10 |
| Event | `tbl_events` | tidak | user(belongsTo) | is_allday:boolean, start:datetime, end:datetime, attachments:array | 12 |
| Foto | `tbl_foto` | tidak | komponen(belongsTo), pekerjaan(belongsTo), penerima(belongsTo) | pekerjaan_id:integer, komponen_id:integer, penerima_id:integer, validasi_koordinat:boolean, unit_index:integer | 8 |
| KanbanBoard | `tbl_kanban_boards` | tidak | cards(hasMany), columns(hasMany) | - | 3 |
| KanbanCard | `tbl_kanban_cards` | tidak | board(belongsTo), column(belongsTo), creator(belongsTo), pekerjaan(belongsTo), tiket(belongsTo) | metadata:array | 11 |
| KanbanColumn | `tbl_kanban_columns` | tidak | board(belongsTo), cards(hasMany) | - | 5 |
| Kecamatan | `tbl_kecamatan` | tidak | desa(hasMany) | - | 1 |
| Kegiatan | `tbl_kegiatan` | tidak | - | pagu:decimal:2, kode_rekening:array, sipd_id_sub_bl:integer | 12 |
| KegiatanRole | `kegiatan_role` | tidak | kegiatan(belongsTo), role(belongsTo) | - | 2 |
| Kontrak | `tbl_kontrak` | tidak | kegiatan(belongsTo), pekerjaan(belongsTo), penyedia(belongsTo) | id_kegiatan:integer, id_pekerjaan:integer, id_penyedia:integer, tanggal_penawaran:date, tgl_sppbj:date, tgl_spk:date, tgl_spmk:date, tgl_selesai:date, nilai_kontrak:float, spse_pushed_at:datetime, spse_push_log:array | 20 |
| KontrakAddendum | `tbl_kontrak_addendums` | tidak | approver(belongsTo), creator(belongsTo), items(hasMany), kontrak(belongsTo) | tanggal_addendum:date, tgl_selesai_sebelum:date, tgl_selesai_sesudah:date, nilai_kontrak_sebelum:float, nilai_kontrak_sesudah:float, approved_at:datetime, kelengkapan_override:boolean, attachment_nomors:array | 17 |
| KontrakAddendumItem | `tbl_kontrak_addendum_items` | tidak | addendum(belongsTo) | volume_sebelum:float, volume_sesudah:float, harga_sebelum:float, harga_sesudah:float, subtotal_sebelum:float, subtotal_sesudah:float | 10 |
| LiveChatMessage | `tbl_live_chat_message` | tidak | thread(belongsTo), user(belongsTo) | read_at:datetime | 4 |
| LiveChatThread | `tbl_live_chat_thread` | tidak | latestMessage(hasOne), messages(hasMany), user(belongsTo) | last_message_at:datetime | 3 |
| MasterFasePekerjaan | `(default: master_fase_pekerjaans)` | tidak | - | keywords:array, is_active:boolean | 9 |
| MenuPermission | `(default: menu_permissions)` | tidak | - | allowed_roles:array, is_active:boolean | 5 |
| Output | `tbl_output` | tidak | pekerjaan(belongsTo) | pekerjaan_id:integer, volume:decimal:2, penerima_is_optional:boolean | 5 |
| PanduanPage | `panduan_pages` | tidak | editor(belongsTo) | is_published:boolean, sort_order:integer | 8 |
| Pekerjaan | `tbl_pekerjaan` | tidak | assignedUsers(belongsToMany), berkas(hasMany), checklistItems(belongsToMany), desa(belongsTo), foto(hasMany), kecamatan(belongsTo), kegiatan(belongsTo), kontrakLegacy(hasMany), output(hasMany), pendamping(belongsTo), penerima(hasMany), pengawas(belongsTo), progressEstimasi(hasMany), progressEstimasiHistory(hasMany), sipdLinks(hasMany), spmSanitasi(belongsToMany), tags(belongsToMany), tiket(hasMany), unitSpam(belongsToMany) | pagu:float, is_konsultan:boolean, kecamatan_id:integer, desa_id:integer, kegiatan_id:integer, pengawas_id:integer, pendamping_id:integer | 11 |
| PekerjaanChecklistHistory | `pekerjaan_checklist_histories` | tidak | checklistItem(belongsTo), pekerjaan(belongsTo), user(belongsTo) | is_checked:boolean, created_at:datetime | 6 |
| PekerjaanProgressEstimasi | `pekerjaan_progress_estimasi` | tidak | pekerjaan(belongsTo) | pekerjaan_id:integer, tahun_anggaran:integer, fisik_rencana_tanggal:date, fisik_rencana_persen:float, fisik_realisasi_tanggal:date, fisik_realisasi_persen:float, keuangan_rencana_tanggal:date, keuangan_rencana_persen:float, keuangan_realisasi_tanggal:date, keuangan_realisasi_persen:float | 10 |
| PekerjaanProgressEstimasiHistory | `pekerjaan_progress_estimasi_history` | tidak | pekerjaan(belongsTo) | pekerjaan_id:integer, tahun_anggaran:integer, tanggal:date, tanggal_pembuatan:date, tanggal_pencairan:date, persen:float, nilai:float | 10 |
| Penerima | `tbl_penerima` | tidak | pekerjaan(belongsTo) | pekerjaan_id:integer, jumlah_jiwa:integer, is_komunal:boolean, nik:encrypted, alamat:encrypted | 6 |
| Pengawas | `pengawas` | tidak | pekerjaanAsPendamping(hasMany), pekerjaanAsPengawas(hasMany) | - | 4 |
| Pengelola | `tbl_pengelola` | tidak | unitSpam(belongsTo) | - | 6 |
| Penyedia | `tbl_penyedia` | tidak | - | tanggal_akta:date | 9 |
| PetaPeripaan | `tbl_peta_peripaan` | tidak | pekerjaan(belongsTo), uploader(belongsTo) | geojson:array | 4 |
| ProcurementStagingPaket | `tbl_procurement_staging_paket` | tidak | kontrak(belongsTo), pekerjaan(belongsTo), syncRun(belongsTo) | raw_row:array, fetched_at:datetime | 12 |
| ProcurementSyncRun | `tbl_procurement_sync_runs` | tidak | stagingPakets(hasMany), user(belongsTo) | started_at:datetime, finished_at:datetime | 7 |
| Progress | `tbl_progress` | tidak | pekerjaan(belongsTo) | content:array | 2 |
| PuspenMediaShare | `(default: puspen_media_shares)` | ya | user(belongsTo) | is_public:boolean, expires_at:datetime, last_downloaded_at:datetime | 8 |
| PuspenProgressFisik | `puspen_progress_fisik` | tidak | kontrak(belongsTo) | kontrak_id:integer, tahun_anggaran:integer, rencana:float, realisasi:float, pho_completed:boolean | 5 |
| PuspenProgressFisikOutput | `puspen_progress_fisik_output` | tidak | kontrak(belongsTo), output(belongsTo) | kontrak_id:integer, output_id:integer, tahun_anggaran:integer, realisasi:float | 4 |
| PuspenReviewNote | `(default: puspen_review_notes)` | tidak | pekerjaan(belongsTo), user(belongsTo) | pekerjaan_id:integer, user_id:integer | 3 |
| RoutePermission | `(default: route_permissions)` | tidak | - | allowed_roles:array, is_active:boolean | 5 |
| SignatureLibrary | `signature_libraries` | ya | user(belongsTo) | width:integer, height:integer | 6 |
| SimulationNetwork | `simulation_networks` | ya | pekerjaan(belongsTo), user(belongsTo), versions(hasMany) | network_data:array, simulation_settings:array, last_results:array, last_simulated_at:datetime, version:integer, is_public:boolean | 10 |
| SimulationNetworkVersion | `simulation_network_versions` | tidak | changedBy(belongsTo), network(belongsTo) | network_data:array, simulation_settings:array, version:integer, created_at:datetime | 7 |
| SipdPekerjaanLink | `tbl_sipd_pekerjaan_links` | tidak | pekerjaan(belongsTo) | - | 3 |
| Sk | `sk` | tidak | uploader(belongsTo) | tanggal_sk:date | 4 |
| SpamAchievement | `tbl_spam_achievements` | tidak | unitSpam(belongsTo) | jumlah_sr:integer, jumlah_kk:integer, jumlah_jiwa:integer, jumlah_bjp_kk:integer, jumlah_bjp_jiwa:integer | 9 |
| SpamBudget | `tbl_spam_budgets` | tidak | unitSpam(belongsTo) | unit_spam_id:integer | 6 |
| SpamKelembagaanShareLink | `spam_kelembagaan_share_links` | tidak | creator(belongsTo), submissions(hasMany), unitSpam(belongsTo) | is_active:boolean, expires_at:datetime, max_submissions:integer, submission_count:integer | 9 |
| SpamKelembagaanSubmission | `spam_kelembagaan_submissions` | tidak | reviewer(belongsTo), shareLink(belongsTo), unitSpam(belongsTo) | payload:array, snapshot_before:array, reviewed_at:datetime | 14 |
| SpamWilayahMatch | `(default: spam_wilayah_matchs)` | tidak | desa(belongsTo), kecamatan(belongsTo) | - | $guarded |
| SpmSanitasi | `tbl_spm_sanitasi` | tidak | desa(belongsTo), pekerjaan(belongsToMany) | latitude:float, longitude:float, jumlah_pemanfaat_kk:integer, jumlah_pemanfaat_jiwa:integer, tahun_konstruksi:integer, pembiayaan_apbn:float, pembiayaan_apbd:float, pembiayaan_dak:float, pembiayaan_hibah:float, pembiayaan_csr:float, pembiayaan_lain:float, pembiayaan_total:float, kapasitas_desain:float, kapasitas_terpakai:float, kapasitas_tidak_terpakai:float, biaya_operasional:float, truk_tinja_unit:integer, kapasitas_truk_m3:float, jumlah_ritasi:integer, jarak_maksimal_pelayanan_km:float, alokasi_biaya_operasional:float, pemanfaat_dari_integrasi:boolean, pembiayaan_dari_integrasi:boolean | 39 |
| SpseSession | `tbl_spse_sessions` | tidak | user(belongsTo) | encrypted_cookies:encrypted:array, expires_at:datetime, last_validated_at:datetime, is_active:boolean | 6 |
| SurveyLokasi | `tbl_survey_lokasi` | tidak | desa(belongsTo), kecamatan(belongsTo), tugas(belongsTo), user(belongsTo), verifier(belongsTo) | detail:array, verified_at:datetime | 14 |
| SurveyTugas | `tbl_survey_tugas` | tidak | assignee(belongsTo), assignees(belongsToMany), creator(belongsTo), desa(belongsTo), kecamatan(belongsTo), pekerjaan(belongsTo), surveys(hasMany) | tahun_anggaran:integer, batas_waktu:date | 12 |
| Tag | `tbl_tags` | tidak | pekerjaan(belongsToMany) | - | 3 |
| Tiket | `tbl_tiket` | tidak | comments(hasMany), pekerjaan(belongsTo), user(belongsTo) | - | 8 |
| TiketComment | `tbl_tiket_comment` | tidak | tiket(belongsTo), user(belongsTo) | - | 3 |
| ToolPdf | `tool_pdfs` | ya | children(hasMany), parent(belongsTo), signaturePlacements(hasMany), user(belongsTo) | - | 5 |
| ToolPdfSignaturePlacement | `tool_pdf_signature_placements` | tidak | toolPdf(belongsTo) | page_number:integer, sort_order:integer, signature_width:integer, signature_height:integer, x_ratio:float, y_ratio:float, scale:float | 15 |
| UnitChecklist | `tbl_unit_checklists` | tidak | unitSpam(belongsTo) | is_checked:boolean | 3 |
| UnitSpam | `tbl_unit_spam` | tidak | achievements(hasMany), budgets(hasMany), checklists(hasMany), desa(belongsTo), pekerjaan(belongsToMany), pengelola(hasOne) | is_simspam:boolean | 14 |
| User | `(default: users)` | tidak | - | email_verified_at:datetime, password:hashed | 8 |
| UserDriveItem | `(default: user_drive_items)` | ya | children(hasMany), parent(belongsTo), shares(hasMany), user(belongsTo) | - | 5 |
| UserDriveShare | `(default: user_drive_shares)` | tidak | item(belongsTo), sharedToUser(belongsTo) | - | 2 |
| UsulanKegiatan | `tbl_usulan_kegiatan` | tidak | desa(belongsTo), kecamatan(belongsTo), user(belongsTo) | tanggal_surat_masuk:date, tanggal_surat:date | 10 |

## Lapisan lain (non-model)

Dicatat sebagai bagian dari inventaris logika tersembunyi.

- `app/Services`: 60 file
- `app/Events`: 4 file
- `app/Listeners`: 1 file
- `app/Jobs`: 1 file
- `app/Notifications`: 1 file
- `app/Exports`: 7 file
- `app/Imports`: 2 file
- `app/Traits`: 3 file
- `app/Support`: 1 file
- `app/Providers`: 1 file
- `app/Console`: 9 file
- `app/Exceptions`: 1 file

## Temuan dari inventaris model

1. **Uang disimpan sebagai `float` di sebagian model.** `Pekerjaan.pagu` dan `Kontrak.nilai_kontrak` memakai cast `float`, sedangkan `Kegiatan.pagu` memakai `decimal:2`. Inkonsisten, dan float tidak aman untuk uang. Di Rust pakai `rust_decimal`. Perilaku pembulatan dan format JSON yang dikembalikan harus dicatat di fixture supaya kontrak frontend tetap sama.
2. **Data pribadi terenkripsi.** `Penerima.nik` dan `Penerima.alamat` memakai cast `encrypted`. Rust wajib bisa mendekripsi dengan `APP_KEY`, dan kolom ini perlu penanganan khusus saat ditulis (enkripsi ulang dengan format yang sama).
3. **`SpseSession.encrypted_cookies`** memakai `encrypted:array`. Ini kredensial sesi SPSE, jadi perlu dipindahkan dengan hati-hati dan tidak boleh tercatat di log.
4. **Logika tersembunyi di `Pekerjaan`.** Model ini punya scope `ByUserRole` (RLS), `NotCanceled`, `WithKontrak`, dan hook `deleting` serta `deleted`. Hook `deleting` harus dibaca sebelum modul Pekerjaan dipindah, karena bisa memicu penghapusan turunan.
5. **`Tag` dan `SimulationNetworkVersion`** punya hook `creating`/`updating`. Perilaku otomatisnya (misalnya generasi slug atau nomor versi) harus dicatat.
6. **Kolom `deleted_at`** hanya di model dengan `SoftDeletes`. Model lain yang dihapus permanen tidak punya jejak.

## Catatan parser

- Kolom "Catatan" di tabel hook masih generik ("perlu dibaca di fase 0"). Isi detailnya saat modul yang bersangkutan dikerjakan.
- Relasi dibaca dengan regex, jadi relasi yang dibungkus helper atau trait bisa terlewat.
