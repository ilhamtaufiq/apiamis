<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    public function up(): void
    {
        DB::table('app_settings')->updateOrInsert(
            ['key' => 'capaian_publik_section_active'],
            [
                'value' => '1',
                'type' => 'text',
                'created_at' => now(),
                'updated_at' => now(),
            ]
        );
    }

    public function down(): void
    {
        DB::table('app_settings')->where('key', 'capaian_publik_section_active')->delete();
    }
};
