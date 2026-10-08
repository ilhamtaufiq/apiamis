-- Tabel SPAM (unit, pengelola, checklist, achievement, budget, pivot pekerjaan).
-- Disalin dari migrasi Laravel: 2026_05_24_152417, 2026_05_24_180000, 2026_06_27_140000,
-- 2026_06_27_180000, 2026_07_10_100000, 2026_10_08_000001 (kolom akhir sesudah semua migrasi).
-- Dipakai tes lokal bila tabel belum ada di database uji.

CREATE TABLE IF NOT EXISTS tbl_unit_spam (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    desa_id BIGINT UNSIGNED NOT NULL,
    name VARCHAR(255) NULL,
    is_simspam TINYINT(1) NOT NULL DEFAULT 0,
    sistem_layanan VARCHAR(255) NULL,
    sumber_mata_air_kap VARCHAR(255) NULL,
    sumber_air_tanah_kap VARCHAR(255) NULL,
    lain_lain_kap VARCHAR(255) NULL,
    tahun_pembangunan VARCHAR(10) NULL,
    sumber_dana VARCHAR(255) NULL,
    program VARCHAR(255) NULL,
    tarif_dasar_hukum VARCHAR(255) NULL,
    iuran_nominal VARCHAR(255) NULL,
    pendapatan_bulan VARCHAR(255) NULL,
    biaya_operasional VARCHAR(255) NULL,
    created_at TIMESTAMP NULL DEFAULT NULL,
    updated_at TIMESTAMP NULL DEFAULT NULL,
    PRIMARY KEY (id),
    CONSTRAINT tbl_unit_spam_desa_id_foreign FOREIGN KEY (desa_id) REFERENCES tbl_desa (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS tbl_pengelola (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    unit_spam_id BIGINT UNSIGNED NOT NULL,
    pokmas VARCHAR(255) NULL,
    perdes VARCHAR(255) NULL,
    kepala VARCHAR(255) NULL,
    bendahara VARCHAR(255) NULL,
    sekretaris VARCHAR(255) NULL,
    created_at TIMESTAMP NULL DEFAULT NULL,
    updated_at TIMESTAMP NULL DEFAULT NULL,
    PRIMARY KEY (id),
    UNIQUE KEY tbl_pengelola_unit_spam_id_unique (unit_spam_id),
    CONSTRAINT tbl_pengelola_unit_spam_id_foreign FOREIGN KEY (unit_spam_id) REFERENCES tbl_unit_spam (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS tbl_unit_checklists (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    unit_spam_id BIGINT UNSIGNED NOT NULL,
    item VARCHAR(255) NOT NULL,
    is_checked TINYINT(1) NOT NULL DEFAULT 0,
    created_at TIMESTAMP NULL DEFAULT NULL,
    updated_at TIMESTAMP NULL DEFAULT NULL,
    PRIMARY KEY (id),
    CONSTRAINT tbl_unit_checklists_unit_spam_id_foreign FOREIGN KEY (unit_spam_id) REFERENCES tbl_unit_spam (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS tbl_spam_achievements (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    unit_spam_id BIGINT UNSIGNED NOT NULL,
    tahun VARCHAR(255) NOT NULL,
    sumber VARCHAR(20) NOT NULL DEFAULT 'manual',
    jumlah_sr INT NOT NULL DEFAULT 0,
    jumlah_kk INT NOT NULL DEFAULT 0,
    jumlah_jiwa INT NOT NULL DEFAULT 0,
    jumlah_bjp_kk INT NOT NULL DEFAULT 0,
    jumlah_bjp_jiwa INT NOT NULL DEFAULT 0,
    catatan TEXT NULL,
    created_at TIMESTAMP NULL DEFAULT NULL,
    updated_at TIMESTAMP NULL DEFAULT NULL,
    PRIMARY KEY (id),
    UNIQUE KEY spam_unit_tahun_sumber_unique (unit_spam_id, tahun, sumber),
    CONSTRAINT tbl_spam_achievements_unit_spam_id_foreign FOREIGN KEY (unit_spam_id) REFERENCES tbl_unit_spam (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS tbl_spam_budgets (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    unit_spam_id BIGINT UNSIGNED NOT NULL,
    pekerjaan_id BIGINT UNSIGNED NULL,
    nilai_kontrak DOUBLE NOT NULL,
    tahun VARCHAR(4) NOT NULL,
    nama_paket VARCHAR(255) NOT NULL,
    sumber_dana VARCHAR(50) NOT NULL DEFAULT 'APBD',
    created_at TIMESTAMP NULL DEFAULT NULL,
    updated_at TIMESTAMP NULL DEFAULT NULL,
    PRIMARY KEY (id),
    KEY spam_budget_unit_pekerjaan_idx (unit_spam_id, pekerjaan_id),
    CONSTRAINT tbl_spam_budgets_unit_spam_id_foreign FOREIGN KEY (unit_spam_id) REFERENCES tbl_unit_spam (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS tbl_unit_spam_pekerjaan (
    id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,
    unit_spam_id BIGINT UNSIGNED NOT NULL,
    pekerjaan_id BIGINT UNSIGNED NOT NULL,
    output_id BIGINT UNSIGNED NULL,
    capaian_metric VARCHAR(8) NOT NULL DEFAULT 'jp',
    created_at TIMESTAMP NULL DEFAULT NULL,
    updated_at TIMESTAMP NULL DEFAULT NULL,
    PRIMARY KEY (id),
    UNIQUE KEY unit_spam_pekerjaan_unique (unit_spam_id, pekerjaan_id),
    KEY tbl_unit_spam_pekerjaan_pekerjaan_id_index (pekerjaan_id),
    CONSTRAINT tbl_unit_spam_pekerjaan_unit_spam_id_foreign FOREIGN KEY (unit_spam_id) REFERENCES tbl_unit_spam (id) ON DELETE CASCADE,
    CONSTRAINT tbl_unit_spam_pekerjaan_pekerjaan_id_foreign FOREIGN KEY (pekerjaan_id) REFERENCES tbl_pekerjaan (id) ON DELETE CASCADE,
    CONSTRAINT tbl_unit_spam_pekerjaan_output_id_foreign FOREIGN KEY (output_id) REFERENCES tbl_output (id) ON DELETE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
