<?php
require __DIR__.'/vendor/autoload.php';
$app = require_once __DIR__.'/bootstrap/app.php';
$kernel = $app->make(Illuminate\Contracts\Console\Kernel::class);
$kernel->bootstrap();

$query = App\Models\SpamKelembagaanRaw::query();
$allRecords = $query->get();
$grouped = $allRecords->groupBy('desa_kelurahan_normalized');
$desaNames = $grouped->keys()->toArray();

echo "Desa names from grouped: " . json_encode(array_slice($desaNames, 0, 5)) . "\n";

$desasRaw = \App\Models\Desa::whereIn('n_desa', $desaNames)
    ->pluck('jumlah_penduduk', 'n_desa');

echo "Desas raw count: " . $desasRaw->count() . "\n";
echo "Desas raw keys: " . json_encode(array_slice($desasRaw->keys()->toArray(), 0, 5)) . "\n";

$desas = collect();
foreach ($desasRaw as $k => $v) {
    $desas->put(trim(strtoupper($k)), $v);
}

echo "CAMPAKA target: " . $desas->get('CAMPAKA', 0) . "\n";
echo "MARGALUYU target: " . $desas->get('MARGALUYU', 0) . "\n";
