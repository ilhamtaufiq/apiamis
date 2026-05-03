<?php
require __DIR__.'/vendor/autoload.php';
$app = require_once __DIR__.'/bootstrap/app.php';
$kernel = $app->make(Illuminate\Contracts\Console\Kernel::class);
$kernel->bootstrap();

use Illuminate\Http\Request;
use App\Http\Controllers\AuthController;

$request = Request::create('/api/auth/login', 'POST', [
    'email' => 'admin@apiamis.test',
    'password' => 'password123'
]);

$controller = $app->make(AuthController::class);
try {
    $response = $controller->login($request);
    echo "Login Response Status: " . $response->getStatusCode() . "\n";
    echo "Response Data: " . $response->getContent() . "\n";
} catch (\Exception $e) {
    echo "Login Failed: " . $e->getMessage() . "\n";
}
