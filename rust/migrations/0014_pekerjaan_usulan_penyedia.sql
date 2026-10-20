-- Usulan penyedia per paket pekerjaan. Boleh kosong; bila penyedia dihapus, kolom dikosongkan.
ALTER TABLE tbl_pekerjaan
  ADD COLUMN usulan_penyedia_id BIGINT UNSIGNED NULL AFTER pendamping_id,
  ADD KEY tbl_pekerjaan_usulan_penyedia_id_foreign (usulan_penyedia_id),
  ADD CONSTRAINT tbl_pekerjaan_usulan_penyedia_id_foreign FOREIGN KEY (usulan_penyedia_id) REFERENCES tbl_penyedia (id) ON DELETE SET NULL;
