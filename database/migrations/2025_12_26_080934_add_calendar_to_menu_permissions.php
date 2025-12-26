<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * Run the migrations.
     */
    public function up(): void
    {
        \DB::table('menu_permissions')->insert([
            'menu_key' => 'calendar',
            'menu_label' => 'Kalender',
            'menu_parent' => 'dashboard',
            'is_active' => true,
            'allowed_roles' => json_encode(['admin', 'pengawas']),
            'created_at' => now(),
            'updated_at' => now(),
        ]);
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        \DB::table('menu_permissions')->where('menu_key', 'calendar')->delete();
    }
};
