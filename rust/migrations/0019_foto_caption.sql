-- Caption bebas untuk foto lapangan (fitur posting Lapangan). Kolom `keterangan` tetap tahap progres.
ALTER TABLE tbl_foto ADD COLUMN caption VARCHAR(500) NULL AFTER keterangan;
