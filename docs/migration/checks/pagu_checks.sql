-- Pengecekan data uang (pagu) untuk fase 0.5 dan keputusan T2 di log migrasi.
-- Jalankan terhadap database uji: mysql -uroot apiamis < pagu_checks.sql
-- Tidak menampilkan data pribadi: hanya agregat dan id.

-- 1. Jumlah baris dan kolom tipe uang
SELECT 'tbl_kegiatan' AS tabel, COUNT(*) AS baris FROM tbl_kegiatan
UNION ALL SELECT 'tbl_pekerjaan', COUNT(*) FROM tbl_pekerjaan;

-- 2. Nilai di atas 2^24: di sini FLOAT (single precision) mulai kehilangan presisi
SELECT 'pekerjaan_pagu_di_atas_2pangkat24' AS cek, COUNT(*) AS n
FROM tbl_pekerjaan WHERE pagu > 16777216;

-- 3. Nilai pagu yang punya digit bermakna lebih dari tiga digit nol di belakang
--    (nilai ini berisiko berubah jika disimpan sebagai FLOAT)
SELECT 'pekerjaan_pagu_bukan_kelipatan_1000' AS cek, COUNT(*) AS n
FROM tbl_pekerjaan
WHERE pagu >= 10000000 AND CAST(pagu AS DECIMAL(20,0)) MOD 1000 <> 0;

-- 4. Contoh pembulatan FLOAT (harus menghasilkan 123456792, bukan 123456789)
SELECT CAST(CAST(123456789 AS FLOAT) AS DECIMAL(20,0)) AS float_123456789;

-- 5. Kualitas data: pagu nol
SELECT 'kegiatan_pagu_nol' AS cek, COUNT(*) AS n FROM tbl_kegiatan WHERE pagu = 0
UNION ALL SELECT 'pekerjaan_pagu_nol', COUNT(*) FROM tbl_pekerjaan WHERE pagu = 0;

-- 6. Jumlah pagu pekerjaan per kegiatan melebihi pagu kegiatan (selisih > 1)
SELECT COUNT(*) AS kegiatan_jumlah_pekerjaan_melebihi_pagu FROM (
  SELECT k.id, k.pagu AS pagu_kegiatan, SUM(p.pagu) AS total_pekerjaan
  FROM tbl_kegiatan k JOIN tbl_pekerjaan p ON p.kegiatan_id = k.id
  GROUP BY k.id, k.pagu
  HAVING total_pekerjaan > pagu_kegiatan + 1
) x;
