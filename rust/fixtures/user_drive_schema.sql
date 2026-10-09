-- Tabel user drive (user_drive_items, user_drive_shares).
-- Disalin dari migrasi Laravel: 2026_07_01_010000_create_user_drive_items_table.php dan
-- 2026_08_30_000001_create_user_drive_shares_table.php.
-- Dipakai tes lokal bila tabel belum ada di database uji.

CREATE TABLE IF NOT EXISTS user_drive_items (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    user_id BIGINT UNSIGNED NOT NULL,
    parent_id BIGINT UNSIGNED NULL,
    name VARCHAR(255) NOT NULL,
    kind VARCHAR(20) NOT NULL,
    original_filename VARCHAR(255) NULL,
    created_at TIMESTAMP NULL DEFAULT NULL,
    updated_at TIMESTAMP NULL DEFAULT NULL,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    PRIMARY KEY (id),
    KEY user_drive_items_user_id_parent_id_kind_index (user_id, parent_id, kind),
    CONSTRAINT user_drive_items_user_id_foreign FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE,
    CONSTRAINT user_drive_items_parent_id_foreign FOREIGN KEY (parent_id) REFERENCES user_drive_items (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS user_drive_shares (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    item_id BIGINT UNSIGNED NOT NULL,
    shared_to_user_id BIGINT UNSIGNED NULL,
    created_at TIMESTAMP NULL DEFAULT NULL,
    updated_at TIMESTAMP NULL DEFAULT NULL,
    PRIMARY KEY (id),
    UNIQUE KEY user_drive_shares_item_id_shared_to_user_id_unique (item_id, shared_to_user_id),
    CONSTRAINT user_drive_shares_item_id_foreign FOREIGN KEY (item_id) REFERENCES user_drive_items (id) ON DELETE CASCADE,
    CONSTRAINT user_drive_shares_shared_to_user_id_foreign FOREIGN KEY (shared_to_user_id) REFERENCES users (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
