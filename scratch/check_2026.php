<?php
require __DIR__ . '/vendor/autoload.php';
$app = require_once __DIR__ . '/bootstrap/app.php';
$kernel = $app->make(Illuminate\Contracts\Console\Kernel::class);
$kernel->bootstrap();

use App\Models\Pekerjaan;
use App\Models\Kegiatan;

$count = Pekerjaan::whereHas('kegiatan', function($q) {
    $q->where('tahun_anggaran', '2026');
})->count();

echo "Total Pekerjaan 2026: " . $count . "\n";

$breakdown = Pekerjaan::whereHas('kegiatan', function($q) {
    $q->where('tahun_anggaran', '2026');
})
->select('kegiatan_id', \DB::raw('count(*) as total'))
->groupBy('kegiatan_id')
->with('kegiatan')
->get();

foreach ($breakdown as $b) {
    echo "- " . ($b->kegiatan->nama_sub_kegiatan ?? 'Unknown') . ": " . $b->total . "\n";
}
