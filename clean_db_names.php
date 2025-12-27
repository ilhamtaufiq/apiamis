<?php

require 'vendor/autoload.php';
$app = require_once 'bootstrap/app.php';
$app->make('Illuminate\Contracts\Console\Kernel')->bootstrap();

use Illuminate\Support\Facades\DB;

echo "--- DATABASE NAME CLEANUP START ---\n";

try {
    DB::beginTransaction();

    // Clean Districts (tbl_kecamatan.n_kec)
    $districts = DB::table('tbl_kecamatan')->where('n_kec', 'LIKE', '% %')->get();
    echo "Found " . count($districts) . " districts with spaces.\n";
    foreach ($districts as $d) {
        $newName = str_replace(' ', '', $d->n_kec);
        DB::table('tbl_kecamatan')->where('id', $d->id)->update(['n_kec' => $newName]);
        echo "Updated Kecamatan: '{$d->n_kec}' -> '{$newName}'\n";
    }

    // Clean Villages (tbl_desa.n_desa)
    $villages = DB::table('tbl_desa')->where('n_desa', 'LIKE', '% %')->get();
    echo "Found " . count($villages) . " villages with spaces.\n";
    foreach ($villages as $v) {
        $newName = str_replace(' ', '', $v->n_desa);
        DB::table('tbl_desa')->where('id', $v->id)->update(['n_desa' => $newName]);
        echo "Updated Desa: '{$v->n_desa}' -> '{$newName}'\n";
    }

    DB::commit();
    echo "--- DATABASE CLEANUP SUCCESSFUL ---\n";

} catch (\Exception $e) {
    DB::rollBack();
    echo "--- DATABASE CLEANUP FAILED ---\n";
    echo "Error: " . $e->getMessage() . "\n";
}
