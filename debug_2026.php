<?php
require 'vendor/autoload.php';
$app = require_once 'bootstrap/app.php';
$app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap();

use App\Models\Pekerjaan;
use App\Models\Kegiatan;

$count = Pekerjaan::whereHas('kegiatan', function($q) {
    $q->where('tahun_anggaran', '2025');
})->count();

echo "TOTAL_DB_2025: " . $count . "\n";

$breakdown = Pekerjaan::whereHas('kegiatan', function($q) {
    $q->where('tahun_anggaran', '2025');
})
->select('kegiatan_id', \DB::raw('count(*) as total'))
->groupBy('kegiatan_id')
->with('kegiatan')
->get();

foreach ($breakdown as $b) {
    echo "- " . ($b->kegiatan->nama_sub_kegiatan ?? 'Unknown') . ": " . $b->total . "\n";
}
