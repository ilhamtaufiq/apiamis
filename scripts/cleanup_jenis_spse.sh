#!/usr/bin/env bash
# Rapikan jenis_dokumen hasil sinkronisasi SPSE di tbl_berkas.
#
# Pemakaian:
#   DB_HOST=... DB_PORT=3306 DB_USER=... DB_PASS=... DB_NAME=... \
#     ./cleanup_jenis_spse.sh            # dry run: backup + hitung, tanpa UPDATE
#     ./cleanup_jenis_spse.sh --apply    # backup + UPDATE (minta konfirmasi)
#
# Opsi tambahan:
#   START_DATE=2026-07-16   batas created_at untuk baris SPSE
#   BACKUP_TABLE=...        nama tabel backup (default: tbl_berkas_backup_jenis_<tanggal>)
set -euo pipefail

: "${DB_HOST:?set DB_HOST}"
: "${DB_USER:?set DB_USER}"
: "${DB_PASS:?set DB_PASS}"
: "${DB_NAME:?set DB_NAME}"
DB_PORT="${DB_PORT:-3306}"
START_DATE="${START_DATE:-2026-07-16}"
BACKUP_TABLE="${BACKUP_TABLE:-tbl_berkas_backup_jenis_$(date +%Y%m%d%H%M%S)}"
APPLY=0
[[ "${1:-}" == "--apply" ]] && APPLY=1

# Jenis standar yang tidak boleh diubah (dibandingkan lowercase).
STD_LIST="'rab','gambar','nego','kontrak','spk','ba klarifikasi','hasil negosiasi','laporan harian','laporan mingguan','berita acara','dokumentasi','surat','lainnya'"
WHERE="uploaded_by IS NULL AND created_at >= '${START_DATE} 00:00:00' AND LOWER(jenis_dokumen) NOT IN (${STD_LIST})"

mysql_run() {
  MYSQL_PWD="$DB_PASS" mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" "$DB_NAME" -N -B -e "$1"
}

echo "== Cek koneksi"
mysql_run "SELECT 1" >/dev/null
echo "   OK"

echo "== Backup ke ${BACKUP_TABLE}"
mysql_run "CREATE TABLE \`${BACKUP_TABLE}\` AS SELECT * FROM tbl_berkas"
BACKUP_ROWS=$(mysql_run "SELECT COUNT(*) FROM \`${BACKUP_TABLE}\`")
TOTAL_ROWS=$(mysql_run "SELECT COUNT(*) FROM tbl_berkas")
if [[ "$BACKUP_ROWS" != "$TOTAL_ROWS" ]]; then
  echo "Backup tidak cocok: backup=${BACKUP_ROWS} sumber=${TOTAL_ROWS}" >&2
  exit 1
fi
echo "   ${BACKUP_ROWS} baris tersalin"

echo "== Dry run: baris yang akan berubah"
AFFECTED=$(mysql_run "SELECT COUNT(*) FROM tbl_berkas WHERE ${WHERE}")
echo "   ${AFFECTED} baris"
mysql_run "SELECT jenis_dokumen, COUNT(*) FROM tbl_berkas WHERE ${WHERE} GROUP BY jenis_dokumen ORDER BY COUNT(*) DESC LIMIT 15" \
  | sed 's/^/   /'

if [[ "$APPLY" -eq 0 ]]; then
  echo "Dry run selesai. Jalankan dengan --apply untuk mengubah data."
  echo "Backup tetap ada di ${BACKUP_TABLE}."
  exit 0
fi

read -r -p "Ubah ${AFFECTED} baris menjadi 'SPSE Dokumen'? (ketik 'ya'): " ANSWER
if [[ "$ANSWER" != "ya" ]]; then
  echo "Dibatalkan. Tidak ada perubahan."
  exit 0
fi

echo "== UPDATE"
mysql_run "UPDATE tbl_berkas SET jenis_dokumen = 'SPSE Dokumen', updated_at = NOW() WHERE ${WHERE}"
echo "== Verifikasi"
mysql_run "SELECT COUNT(*) AS spse_dokumen FROM tbl_berkas WHERE jenis_dokumen = 'SPSE Dokumen'" | sed 's/^/   /'
echo "Selesai. Rollback bila perlu: lihat query di cleanup_jenis_spse.sql."
