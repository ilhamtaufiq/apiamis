<?php
require 'vendor/autoload.php';
$app = require_once 'bootstrap/app.php';
$app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap();

$admin = \App\Models\User::whereHas('roles', function($q) { $q->where('name', 'admin'); })->first();
auth()->login($admin);

use App\Http\Controllers\ChatController;
$chat = new ChatController(app(\App\Services\OpenRouterService::class));
$reflection = new \ReflectionClass(ChatController::class);
$method = $reflection->getMethod('getDatabaseContext');
$method->setAccessible(true);

$q = "Berapa banyak paket Pembangunan Sistem Pengelolaan Air Limbah Domestik (SPALD) Terpusat Skala Permukiman di 2025?";
$context = $method->invoke($chat, $q);

echo "CONTEXT DUMP:\n";
echo "====================================\n";
echo $context;
echo "\n====================================\n";
