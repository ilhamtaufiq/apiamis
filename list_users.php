<?php
require __DIR__.'/vendor/autoload.php';
$app = require_once __DIR__.'/bootstrap/app.php';
$kernel = $app->make(Illuminate\Contracts\Console\Kernel::class);
$kernel->bootstrap();

use App\Models\User;

$users = User::all();
echo "Total Users: " . $users->count() . "\n";
foreach ($users as $user) {
    echo "- {$user->name} ({$user->email})\n";
}
