#!/usr/bin/env bash
# Membandingkan schema dua database MySQL/MariaDB (tabel, kolom, index, dan foreign key).
# Pemakaian: schema-drift.sh <database_uji> <database_referensi>
# Keluaran kosong dan kode keluar 0 berarti schema sama. Selisih dicetak dengan awalan "<" (hanya di uji)
# dan ">" (hanya di referensi). Hanya membaca metadata information_schema, tidak mengubah data.
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "pemakaian: $0 <database_uji> <database_referensi>" >&2
    exit 2
fi

MYSQL=(mysql -uroot -N -B)

snapshot() {
    local db="$1"
    "${MYSQL[@]}" "$db" -e "
        SELECT CONCAT('TABLE ', table_name) FROM information_schema.tables
            WHERE table_schema = '$db' AND table_type = 'BASE TABLE';
        SELECT CONCAT('COLUMN ', table_name, '.', column_name, ' ', column_type, ' null=', is_nullable,
                      ' default=', IFNULL(column_default, 'NULL'), ' extra=', extra)
            FROM information_schema.columns WHERE table_schema = '$db';
        SELECT CONCAT('INDEX ', table_name, '.', index_name, ' ', index_type, ' non_unique=', non_unique,
                      ' cols=', GROUP_CONCAT(column_name ORDER BY seq_in_index))
            FROM information_schema.statistics WHERE table_schema = '$db'
            GROUP BY table_name, index_name, index_type, non_unique;
        SELECT CONCAT('FK ', table_name, '.', constraint_name, ' -> ', referenced_table_name, '.', referenced_column_name)
            FROM information_schema.key_column_usage
            WHERE table_schema = '$db' AND referenced_table_name IS NOT NULL;" | sort
}

diff <(snapshot "$1") <(snapshot "$2") | grep -E '^[<>]' | sed -e 's/^</< /' -e 's/^>/> /' || true
