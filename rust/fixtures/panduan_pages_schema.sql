-- Skema panduan_pages dari database/migrations/2026_07_18_100000_create_panduan_pages_table.php.
CREATE TABLE IF NOT EXISTS `panduan_pages` (
  `id` bigint(20) unsigned NOT NULL AUTO_INCREMENT,
  `slug` varchar(120) NOT NULL,
  `title` varchar(255) NOT NULL,
  `description` varchar(500) DEFAULT NULL,
  `section` varchar(80) NOT NULL DEFAULT 'umum',
  `sort_order` int(10) unsigned NOT NULL DEFAULT 0,
  `body` longtext NOT NULL,
  `is_published` tinyint(1) NOT NULL DEFAULT 1,
  `updated_by` bigint(20) unsigned DEFAULT NULL,
  `created_at` timestamp NULL DEFAULT NULL,
  `updated_at` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `panduan_pages_slug_unique` (`slug`),
  KEY `panduan_pages_section_index` (`section`),
  KEY `panduan_pages_sort_order_index` (`sort_order`),
  KEY `panduan_pages_is_published_index` (`is_published`),
  KEY `panduan_pages_updated_by_foreign` (`updated_by`),
  CONSTRAINT `panduan_pages_updated_by_foreign` FOREIGN KEY (`updated_by`) REFERENCES `users` (`id`) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
