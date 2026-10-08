-- Tabel blog, aset blog, dan komentar blog (disalin dari migrasi Laravel
-- create_blog_table + add_is_internal + add_featured_fields + create_blog_assets_table
-- + add_blog_id_to_blog_assets + create_blog_comments_table). Aman dijalankan ulang.
CREATE TABLE IF NOT EXISTS tbl_blog (
  id bigint unsigned NOT NULL AUTO_INCREMENT,
  title varchar(255) NOT NULL,
  slug varchar(255) NOT NULL,
  content longtext NOT NULL,
  category varchar(255) NULL,
  cover_image varchar(255) NULL,
  user_id bigint unsigned NOT NULL,
  is_published tinyint(1) NOT NULL DEFAULT 0,
  is_internal tinyint(1) NOT NULL DEFAULT 0,
  is_featured tinyint(1) NOT NULL DEFAULT 0,
  published_at timestamp NULL DEFAULT NULL,
  featured_at timestamp NULL DEFAULT NULL,
  created_at timestamp NULL DEFAULT NULL,
  updated_at timestamp NULL DEFAULT NULL,
  PRIMARY KEY (id),
  UNIQUE KEY tbl_blog_slug_unique (slug),
  KEY tbl_blog_user_id_foreign (user_id),
  CONSTRAINT tbl_blog_user_id_foreign FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS tbl_blog_assets (
  id bigint unsigned NOT NULL AUTO_INCREMENT,
  user_id bigint unsigned NULL,
  blog_id bigint unsigned NULL,
  created_at timestamp NULL DEFAULT NULL,
  updated_at timestamp NULL DEFAULT NULL,
  PRIMARY KEY (id),
  CONSTRAINT tbl_blog_assets_user_id_foreign FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE SET NULL,
  CONSTRAINT tbl_blog_assets_blog_id_foreign FOREIGN KEY (blog_id) REFERENCES tbl_blog (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS tbl_blog_comment (
  id bigint unsigned NOT NULL AUTO_INCREMENT,
  blog_id bigint unsigned NOT NULL,
  user_id bigint unsigned NOT NULL,
  parent_id bigint unsigned NULL,
  body text NOT NULL,
  depth tinyint unsigned NOT NULL DEFAULT 0,
  deleted_at timestamp NULL DEFAULT NULL,
  created_at timestamp NULL DEFAULT NULL,
  updated_at timestamp NULL DEFAULT NULL,
  PRIMARY KEY (id),
  KEY tbl_blog_comment_blog_id_parent_id_index (blog_id, parent_id),
  KEY tbl_blog_comment_blog_id_created_at_index (blog_id, created_at),
  CONSTRAINT tbl_blog_comment_blog_id_foreign FOREIGN KEY (blog_id) REFERENCES tbl_blog (id) ON DELETE CASCADE,
  CONSTRAINT tbl_blog_comment_user_id_foreign FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE,
  CONSTRAINT tbl_blog_comment_parent_id_foreign FOREIGN KEY (parent_id) REFERENCES tbl_blog_comment (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
