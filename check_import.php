<?php

require 'vendor/autoload.php';
$app = require_once 'bootstrap/app.php';
$kernel = $app->make(Illuminate\Contracts\Console\Kernel::class);
$kernel->bootstrap();

use App\Models\Pekerjaan;
use App\Models\Penyedia;
use Maatwebsite\Excel\Facades\Excel;
use Maatwebsite\Excel\HeadingRowImport;

$file = 'c:/laragon/www/bun/template_kontrak-1.xlsx';

$rows = Excel::toArray(new class implements \Maatwebsite\Excel\Concerns\WithHeadingRow {}, $file)[0];

echo "Total rows in Excel: " . count($rows) . "\n";

$pekerjaanIds = [];
$importedCount = 0;

foreach ($rows as $index => $row) {
    $pekerjaanName = $row['nama_paket'] ?? null;
    $pekerjaan = null;
    if ($pekerjaanName) {
        $pekerjaan = Pekerjaan::where('nama_paket', 'LIKE', '%' . $pekerjaanName . '%')->first();
    }

    $penyediaName = $row['nama_penyedia'] ?? null;
    $penyedia = null;
    if ($penyediaName) {
        $penyedia = Penyedia::where('nama', 'LIKE', '%' . $penyediaName . '%')->first();
    }

    if (!$pekerjaan) {
        echo "Row " . ($index + 2) . ": Pekerjaan NOT FOUND for [" . $pekerjaanName . "]\n";
        continue;
    }
    if (!$penyedia) {
        echo "Row " . ($index + 2) . ": Penyedia NOT FOUND for [" . $penyediaName . "]\n";
        continue;
    }

    if (isset($pekerjaanIds[$pekerjaan->id])) {
        echo "Row " . ($index + 2) . ": DUPLICATE Pekerjaan ID [" . $pekerjaan->id . "] (Packages: [" . $pekerjaanIds[$pekerjaan->id] . "] and [" . $pekerjaanName . "])\n";
    } else {
        $pekerjaanIds[$pekerjaan->id] = $pekerjaanName;
        $importedCount++;
    }
}

echo "Total unique Pekerjaan to be imported: " . $importedCount . "\n";
