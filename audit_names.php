<?php

use Illuminate\Support\Facades\DB;

// Only if we can bootstrap Laravel
function bootstrap() {
    if (!file_exists('vendor/autoload.php')) return false;
    require 'vendor/autoload.php';
    if (!file_exists('bootstrap/app.php')) return false;
    $app = require_once 'bootstrap/app.php';
    $app->make('Illuminate\Contracts\Console\Kernel')->bootstrap();
    return true;
}

function auditGeoJson($filePath) {
    if (!file_exists($filePath)) {
        echo "GeoJSON File not found: $filePath\n";
        return [];
    }

    $json = json_decode(file_get_contents($filePath), true);
    if (!$json) {
        echo "Failed to decode JSON\n";
        return [];
    }

    $found = [];
    foreach ($json['features'] as $feature) {
        $district = $feature['properties']['district'] ?? '';
        $village = $feature['properties']['village'] ?? '';

        if (strpos($district, ' ') !== false) $found["District: $district"] = true;
        if (strpos($village, ' ') !== false) $found["Village: $village"] = true;
    }
    return array_keys($found);
}

function auditDatabase() {
    try {
        $districts = DB::table('tbl_kecamatan')->where('nama_kecamatan', 'LIKE', '% %')->pluck('nama_kecamatan')->toArray();
        $villages = DB::table('tbl_desa')->where('nama_desa', 'LIKE', '% %')->pluck('nama_desa')->toArray();
        
        $found = [];
        foreach ($districts as $d) $found["DB District: $d"] = true;
        foreach ($villages as $v) $found["DB Village: $v"] = true;
        return array_keys($found);
    } catch (\Exception $e) {
        return ["Database error: " . $e->getMessage()];
    }
}

echo "--- AUDIT START ---\n";
echo "GeoJSON Findings:\n";
print_r(auditGeoJson('c:\\laragon\\www\\bun\\src\\assets\\geojson\\kecamatan\\id3203_cianjur_simplified.geojson'));

if (bootstrap()) {
    echo "\nDatabase Findings:\n";
    print_r(auditDatabase());
} else {
    echo "\nSkipping Database Audit (Not in a Laravel root or bootstrap failed).\n";
}
echo "--- AUDIT END ---\n";
