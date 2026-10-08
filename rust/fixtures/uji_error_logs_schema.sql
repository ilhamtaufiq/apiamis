-- Skema uji untuk rute admin `/api/error-logs` (`error_logs_db`, `error_logs_empty_db`).
-- Dua basis data terpisah, tidak menyentuh `apiamis`:
--   apiamis_uji_error_logs        : tes daftar, detail, selesai/buka, dan hapus massal (paralel).
--   apiamis_uji_error_logs_empty  : tes `/empty` yang menghapus seluruh tabel (satu tes saja).
-- Tabel auth dan `app_settings` (maintenance) disalin struktur dari `apiamis` tanpa data. `error_logs` memakai DDL dari
-- rust/fixtures/error_logs_schema.sql.
-- Terapkan: mysql -uroot --socket=/run/mysqld/mysqld.sock < rust/fixtures/uji_error_logs_schema.sql
DROP DATABASE IF EXISTS apiamis_uji_error_logs;
CREATE DATABASE apiamis_uji_error_logs CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
USE apiamis_uji_error_logs;
CREATE TABLE users LIKE apiamis.users;
CREATE TABLE personal_access_tokens LIKE apiamis.personal_access_tokens;
CREATE TABLE roles LIKE apiamis.roles;
CREATE TABLE model_has_roles LIKE apiamis.model_has_roles;
CREATE TABLE model_has_permissions LIKE apiamis.model_has_permissions;
CREATE TABLE permissions LIKE apiamis.permissions;
CREATE TABLE role_has_permissions LIKE apiamis.role_has_permissions;
CREATE TABLE route_permissions LIKE apiamis.route_permissions;
CREATE TABLE media LIKE apiamis.media;
CREATE TABLE app_settings LIKE apiamis.app_settings;
CREATE TABLE `error_logs` (
  `id` bigint unsigned NOT NULL AUTO_INCREMENT,
  `user_id` bigint unsigned DEFAULT NULL,
  `source` varchar(50) NOT NULL,
  `message` text NOT NULL,
  `stack` longtext,
  `component_stack` longtext,
  `url` text,
  `user_agent` text,
  `ip_address` varchar(45) DEFAULT NULL,
  `metadata` json DEFAULT NULL,
  `resolved_at` timestamp NULL DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `error_logs_source_created_at_index` (`source`,`created_at`),
  KEY `error_logs_user_id_created_at_index` (`user_id`,`created_at`),
  KEY `error_logs_resolved_at_index` (`resolved_at`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

DROP DATABASE IF EXISTS apiamis_uji_error_logs_empty;
CREATE DATABASE apiamis_uji_error_logs_empty CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
USE apiamis_uji_error_logs_empty;
CREATE TABLE users LIKE apiamis.users;
CREATE TABLE personal_access_tokens LIKE apiamis.personal_access_tokens;
CREATE TABLE roles LIKE apiamis.roles;
CREATE TABLE model_has_roles LIKE apiamis.model_has_roles;
CREATE TABLE model_has_permissions LIKE apiamis.model_has_permissions;
CREATE TABLE permissions LIKE apiamis.permissions;
CREATE TABLE role_has_permissions LIKE apiamis.role_has_permissions;
CREATE TABLE route_permissions LIKE apiamis.route_permissions;
CREATE TABLE media LIKE apiamis.media;
CREATE TABLE app_settings LIKE apiamis.app_settings;
CREATE TABLE `error_logs` LIKE apiamis_uji_error_logs.error_logs;
