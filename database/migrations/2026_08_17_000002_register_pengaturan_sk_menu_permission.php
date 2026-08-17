<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    public function up(): void
    {
        DB::table('menu_permissions')->updateOrInsert(
            ['menu_key' => 'pengaturan_sk'],
            [
                'menu_label' => 'Pengaturan SK',
                'menu_parent' => 'Pengaturan',
                'allowed_roles' => json_encode(['admin']),
                'is_active' => true,
                'created_at' => now(),
                'updated_at' => now(),
            ],
        );
    }

    public function down(): void
    {
        DB::table('menu_permissions')->where('menu_key', 'pengaturan_sk')->delete();
    }
};
