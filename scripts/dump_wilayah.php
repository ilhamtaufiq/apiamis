<?php

require __DIR__ . '/../vendor/autoload.php';
$app = require_once __DIR__ . '/../bootstrap/app.php';
$kernel = $app->make(Illuminate\Contracts\Console\Kernel::class);
$kernel->bootstrap();

use App\Models\Kecamatan;

$kecamatans = Kecamatan::with('desa')->get();

$content = "# DAFTAR WILAYAH KABUPATEN CIANJUR (ARUMANIS)\n\n";
$content .= "Daftar ini berisi seluruh Kecamatan dan Desa yang terdaftar dalam sistem Arumanis.\n\n";

foreach ($kecamatans as $kec) {
    $content .= "## KECAMATAN: " . strtoupper($kec->n_kec) . "\n";
    $desaNames = $kec->desa->pluck('n_desa')->toArray();
    $content .= "Desa: " . implode(", ", $desaNames) . "\n\n";
}

$targetPath = __DIR__ . '/../docs/backend/WILAYAH.md';
if (!is_dir(dirname($targetPath))) {
    mkdir(dirname($targetPath), 0755, true);
}

file_put_contents($targetPath, $content);

echo "Berhasil mengekstrak data wilayah ke: " . $targetPath . "\n";
