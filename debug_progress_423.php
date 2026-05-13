<?php

use App\Models\Progress;

require __DIR__ . '/vendor/autoload.php';
$app = require_once __DIR__ . '/bootstrap/app.php';
$app->make(\Illuminate\Contracts\Console\Kernel::class)->bootstrap();

$pekerjaanId = 423;
$p = Progress::where('pekerjaan_id', $pekerjaanId)->first();

if (!$p) {
    echo "Progress not found\n";
    exit;
}

$items = $p->content['items'] ?? [];
$totalWeightedProgress = 0;
$hasAnyRealisasi = false;

foreach ($items as $index => $item) {
    $bobot = (float) ($item['bobot'] ?? 0);
    $weeklyData = $item['weekly_data'] ?? [];
    $itemTotalReal = 0;

    foreach ($weeklyData as $minggu => $data) {
        $realisasi = $data['realisasi'] ?? 0;
        if ($realisasi > 0) {
            $itemTotalReal += $realisasi;
            $hasAnyRealisasi = true;
        }
    }

    $targetVolume = (float) ($item['target_volume'] ?? 0);
    $progressPercent = $targetVolume > 0 ? ($itemTotalReal / $targetVolume) * 100 : 0;
    $weightedProgress = ($progressPercent * $bobot) / 100;
    $totalWeightedProgress += $weightedProgress;
    
    if ($itemTotalReal > 0) {
        echo "Item $index: Realisasi=$itemTotalReal, Target=$targetVolume, Bobot=$bobot%, Weighted=" . round($weightedProgress, 4) . "%\n";
    }
}

echo "---------------------------------\n";
echo "HAS ANY REALISASI: " . ($hasAnyRealisasi ? 'YES' : 'NO') . "\n";
echo "TOTAL WEIGHTED PROGRESS: " . round($totalWeightedProgress, 4) . "%\n";
