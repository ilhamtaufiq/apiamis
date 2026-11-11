-- Nomor surat pernyataan BMD per kontrak. Diisi otomatis dari penomoran surat (jenis KEU-SP) saat
-- pertama kali surat BMD dibuat, lalu dipakai ulang. Kolom kosong bila surat dihapus dari Penomoran Surat.
-- Tipe kolom harus UNSIGNED seperti surat.id, agar foreign key bisa dibuat.
ALTER TABLE tbl_kontrak
  ADD COLUMN surat_bmd_id BIGINT UNSIGNED NULL,
  ADD KEY tbl_kontrak_surat_bmd_id_foreign (surat_bmd_id),
  ADD CONSTRAINT tbl_kontrak_surat_bmd_id_foreign FOREIGN KEY (surat_bmd_id) REFERENCES surat (id) ON DELETE SET NULL;
