-- Isi informasi kini berupa HTML dari editor (sama seperti publikasi), sehingga TEXT (64 KB) tidak cukup.
ALTER TABLE informasi MODIFY isi MEDIUMTEXT COLLATE utf8mb4_unicode_ci NOT NULL;
