<?php
require __DIR__.'/vendor/autoload.php';
$app = require_once __DIR__.'/bootstrap/app.php';
$kernel = $app->make(Illuminate\Contracts\Console\Kernel::class);
$kernel->bootstrap();

use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Facades\DB;

$tables = DB::select('SHOW TABLES');
echo "Tables in database:\n";
foreach ($tables as $table) {
    $tableName = array_values((array)$table)[0];
    echo "- $tableName\n";
}

$sanctumTable = Schema::hasTable('personal_access_tokens') ? 'Exists' : 'MISSING';
echo "\npersonal_access_tokens: $sanctumTable\n";
