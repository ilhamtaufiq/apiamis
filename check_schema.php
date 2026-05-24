<?php

require __DIR__.'/vendor/autoload.php';
$app = require_once __DIR__.'/bootstrap/app.php';
$kernel = $app->make(Illuminate\Contracts\Console\Kernel::class);
$kernel->bootstrap();

use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Schema;

$tables = DB::select('SHOW TABLES');
$tableKey = 'Tables_in_' . env('DB_DATABASE');

$targetTables = [];
foreach ($tables as $table) {
    $tableName = $table->{$tableKey} ?? array_values((array)$table)[0];
    if (strpos(strtolower($tableName), 'kecamatan') !== false || strpos(strtolower($tableName), 'desa') !== false) {
        $targetTables[] = $tableName;
    }
}

foreach ($targetTables as $table) {
    echo "Table: $table\n";
    $columns = DB::select("SHOW COLUMNS FROM `$table`");
    foreach ($columns as $column) {
        echo "  - {$column->Field} ({$column->Type})\n";
    }
    echo "\n";
}
