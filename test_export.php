<?php
require __DIR__ . '/vendor/autoload.php';

$app = require_once __DIR__.'/bootstrap/app.php';
$kernel = $app->make(Illuminate\Contracts\Console\Kernel::class);
$kernel->bootstrap();

$pekerjaan = \App\Models\Pekerjaan::first();
$exporter = new \App\Services\DocumentExportService();
try {
    $pdf = $exporter->exportKontrak($pekerjaan, null, 'pdf');
    echo "PDF generated at: $pdf\n";
    echo "Size: " . filesize($pdf) . " bytes\n";
} catch (\Exception $e) {
    echo "Error: " . $e->getMessage() . "\n";
}
