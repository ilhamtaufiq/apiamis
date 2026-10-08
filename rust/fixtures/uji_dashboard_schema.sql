-- Skema uji untuk rute dashboard dan pencarian (`dashboard_db`, `search_db`).
-- Basis data terpisah `apiamis_uji_dashboard` berisi salinan struktur tabel dari `apiamis` tanpa data.
-- Tidak mengubah tabel di `apiamis`. Satu-satunya perbedaan: `tbl_output` diberi FULLTEXT
-- `ft_output_search` (migrasi 2026_04_13_154000), yang belum ada di DB lokal `apiamis`.
-- Terapkan: mysql -uroot --socket=/run/mysqld/mysqld.sock < rust/fixtures/uji_dashboard_schema.sql
DROP DATABASE IF EXISTS apiamis_uji_dashboard;
CREATE DATABASE apiamis_uji_dashboard CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
USE apiamis_uji_dashboard;
CREATE TABLE `users` LIKE `apiamis`.`users`;
CREATE TABLE `personal_access_tokens` LIKE `apiamis`.`personal_access_tokens`;
CREATE TABLE `roles` LIKE `apiamis`.`roles`;
CREATE TABLE `model_has_roles` LIKE `apiamis`.`model_has_roles`;
CREATE TABLE `permissions` LIKE `apiamis`.`permissions`;
CREATE TABLE `model_has_permissions` LIKE `apiamis`.`model_has_permissions`;
CREATE TABLE `role_has_permissions` LIKE `apiamis`.`role_has_permissions`;
CREATE TABLE `route_permissions` LIKE `apiamis`.`route_permissions`;
CREATE TABLE `app_settings` LIKE `apiamis`.`app_settings`;
CREATE TABLE `user_pekerjaan` LIKE `apiamis`.`user_pekerjaan`;
CREATE TABLE `kegiatan_role` LIKE `apiamis`.`kegiatan_role`;
CREATE TABLE `tbl_kegiatan` LIKE `apiamis`.`tbl_kegiatan`;
CREATE TABLE `tbl_pekerjaan` LIKE `apiamis`.`tbl_pekerjaan`;
CREATE TABLE `tbl_kecamatan` LIKE `apiamis`.`tbl_kecamatan`;
CREATE TABLE `tbl_desa` LIKE `apiamis`.`tbl_desa`;
CREATE TABLE `tbl_penyedia` LIKE `apiamis`.`tbl_penyedia`;
CREATE TABLE `tbl_kontrak` LIKE `apiamis`.`tbl_kontrak`;
CREATE TABLE `kontrak_pekerjaan` LIKE `apiamis`.`kontrak_pekerjaan`;
CREATE TABLE `tbl_output` LIKE `apiamis`.`tbl_output`;
CREATE TABLE `tbl_penerima` LIKE `apiamis`.`tbl_penerima`;
CREATE TABLE `tbl_foto` LIKE `apiamis`.`tbl_foto`;
CREATE TABLE `media` LIKE `apiamis`.`media`;
CREATE TABLE `tbl_progress` LIKE `apiamis`.`tbl_progress`;
CREATE TABLE `pekerjaan_progress_estimasi_history` LIKE `apiamis`.`pekerjaan_progress_estimasi_history`;
CREATE TABLE `pekerjaan_tag` LIKE `apiamis`.`pekerjaan_tag`;
CREATE TABLE `tbl_tags` LIKE `apiamis`.`tbl_tags`;
ALTER TABLE tbl_output ADD FULLTEXT KEY ft_output_search (komponen, satuan);
