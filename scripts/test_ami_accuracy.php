<?php
require 'vendor/autoload.php';
$app = require_once 'bootstrap/app.php';
$app->make(Illuminate\Contracts\Console\Kernel::class)->bootstrap();

use App\Models\Pekerjaan;
use App\Models\Kegiatan;
use App\Services\OpenRouterService;
use App\Http\Controllers\ChatController;
use Illuminate\Support\Facades\DB;

// 0. Mock Auth for CLI (bypass byUserRole restriction)
$admin = \App\Models\User::whereHas('roles', function($q) { $q->where('name', 'admin'); })->first();
if ($admin) {
    auth()->login($admin);
    echo "🔑 Authenticated as Admin: " . $admin->name . "\n";
} else {
    echo "⚠️ Warning: Admin user not found. Results might be empty due to role restrictions.\n";
}

// 1. Setup Test Questions
$testCases = [
    [
        'question' => "Berapa jumlah total pekerjaan di tahun anggaran 2025?",
        'db_query' => function() {
            return Pekerjaan::whereHas('kegiatan', function($q) { $q->where('tahun_anggaran', '2025'); })->count();
        },
        'validator' => function($answer, $actual) {
            return str_contains($answer, (string)$actual);
        }
    ],
    [
        'question' => "Berapa banyak paket Pembangunan Sistem Pengelolaan Air Limbah Domestik (SPALD) Terpusat Skala Permukiman di 2025?",
        'db_query' => function() {
            return Pekerjaan::whereHas('kegiatan', function($q) { 
                $q->where('tahun_anggaran', '2025')
                  ->where('nama_sub_kegiatan', 'LIKE', '%SPALD%Terpusat%Skala%Permukiman%'); 
            })->count();
        },
        'validator' => function($answer, $actual) {
            return str_contains($answer, (string)$actual);
        }
    ]
];

$openRouter = app(OpenRouterService::class);
$chatController = new ChatController($openRouter);

// Ensure we have the API key available for the bridge
$apiKey = env('OPENROUTER_API_KEY');
if (!$apiKey) {
    echo "❌ Error: OPENROUTER_API_KEY not found in .env\n";
    exit(1);
}

echo "🚀 MEMULAI TEST AKURASI AMI (Using OpenRouter)...\n";
echo "--------------------------------------------------\n";

foreach ($testCases as $i => $test) {
    echo "TEST #" . ($i+1) . ": " . $test['question'] . "\n";
    
    // Get DB Truth
    $actualCount = ($test['db_query'])();
    
    // Get Ami's Answer (Simulate Context + AI Call)
    // We call the controller logic directly to avoid full HTTP overhead
    $reflection = new \ReflectionClass(ChatController::class);
    $method = $reflection->getMethod('getDatabaseContext');
    $method->setAccessible(true);
    $context = $method->invoke($chatController, $test['question']);
    
    echo "🔍 Context Generated (Summary): " . substr($context, 0, 100) . "...\n";
    
    $result = $openRouter->chatWithLangChain($test['question'], $context, []);
    
    if (!$result['success']) {
        echo "❌ AI Error: " . $result['message'] . "\n";
        continue;
    }
    
    $amiAnswer = $result['content'];
    $isValid = ($test['validator'])($amiAnswer, $actualCount);
    
    echo "🤖 Jawaban Ami: " . substr($amiAnswer, 0, 150) . "...\n";
    echo "📊 Data DB: " . $actualCount . "\n";
    echo ($isValid ? "✅ VALID" : "❌ TIDAK VALID") . "\n";
    echo "--------------------------------------------------\n";
}
