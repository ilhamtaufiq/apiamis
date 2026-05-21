<?php
require __DIR__.'/vendor/autoload.php';
$app = require_once __DIR__.'/bootstrap/app.php';
$kernel = $app->make(Illuminate\Contracts\Console\Kernel::class);
$kernel->bootstrap();

$request = Illuminate\Http\Request::create('/api/spam-kelembagaan', 'GET', ['per_page' => 15]);
$controller = new App\Http\Controllers\SpamKelembagaanRawController();
$response = $controller->index($request);

$data = json_decode($response->getContent(), true)['data'];

foreach ($data as $item) {
    if (in_array(strtolower($item['desa_kelurahan_normalized']), ['campaka', 'margaluyu', 'sukajadi'])) {
        echo $item['desa_kelurahan_normalized'] . " target: " . $item['target_layanan'] . " | Jiwa: " . $item['total_jiwa_terlayani'] . "\n";
    }
}
