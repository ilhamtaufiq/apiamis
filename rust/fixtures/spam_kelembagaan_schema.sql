-- Tabel share form kelembagaan SPAM (spam_kelembagaan_share_links, spam_kelembagaan_submissions).
-- Disalin dari migrasi Laravel: 2026_07_10_120000_create_spam_kelembagaan_share_tables.php.
-- Dipakai tes lokal bila tabel belum ada di database uji. Membutuhkan tbl_unit_spam dan users.

CREATE TABLE IF NOT EXISTS spam_kelembagaan_share_links (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    unit_spam_id BIGINT UNSIGNED NOT NULL,
    created_by BIGINT UNSIGNED NULL,
    token VARCHAR(64) NOT NULL,
    label VARCHAR(255) NULL,
    is_active TINYINT(1) NOT NULL DEFAULT 1,
    expires_at TIMESTAMP NULL DEFAULT NULL,
    max_submissions INT UNSIGNED NULL,
    submission_count INT UNSIGNED NOT NULL DEFAULT 0,
    admin_note TEXT NULL,
    created_at TIMESTAMP NULL DEFAULT NULL,
    updated_at TIMESTAMP NULL DEFAULT NULL,
    PRIMARY KEY (id),
    UNIQUE KEY spam_kelembagaan_share_links_token_unique (token),
    KEY spam_kelembagaan_share_links_unit_spam_id_is_active_index (unit_spam_id, is_active),
    CONSTRAINT spam_kelembagaan_share_links_unit_spam_id_foreign FOREIGN KEY (unit_spam_id) REFERENCES tbl_unit_spam (id) ON DELETE CASCADE,
    CONSTRAINT spam_kelembagaan_share_links_created_by_foreign FOREIGN KEY (created_by) REFERENCES users (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS spam_kelembagaan_submissions (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    share_link_id BIGINT UNSIGNED NOT NULL,
    unit_spam_id BIGINT UNSIGNED NOT NULL,
    payload JSON NOT NULL,
    snapshot_before JSON NULL,
    submitter_name VARCHAR(255) NULL,
    submitter_phone VARCHAR(50) NULL,
    submitter_instansi VARCHAR(255) NULL,
    submitter_note TEXT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    reviewed_by BIGINT UNSIGNED NULL,
    reviewed_at TIMESTAMP NULL DEFAULT NULL,
    review_note TEXT NULL,
    submitter_ip VARCHAR(45) NULL,
    user_agent VARCHAR(500) NULL,
    created_at TIMESTAMP NULL DEFAULT NULL,
    updated_at TIMESTAMP NULL DEFAULT NULL,
    PRIMARY KEY (id),
    KEY spam_kelembagaan_submissions_status_created_at_index (status, created_at),
    KEY spam_kelembagaan_submissions_unit_spam_id_status_index (unit_spam_id, status),
    CONSTRAINT spam_kelembagaan_submissions_share_link_id_foreign FOREIGN KEY (share_link_id) REFERENCES spam_kelembagaan_share_links (id) ON DELETE CASCADE,
    CONSTRAINT spam_kelembagaan_submissions_unit_spam_id_foreign FOREIGN KEY (unit_spam_id) REFERENCES tbl_unit_spam (id) ON DELETE CASCADE,
    CONSTRAINT spam_kelembagaan_submissions_reviewed_by_foreign FOREIGN KEY (reviewed_by) REFERENCES users (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
