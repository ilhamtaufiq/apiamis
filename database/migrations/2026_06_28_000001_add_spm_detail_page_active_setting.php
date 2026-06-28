<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    public function up(): void
    {
        DB::table('app_settings')->updateOrInsert(
            ['key' => 'spm_detail_page_active'],
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
        DB::table('app_settings')->where('key', 'spm_detail_page_active')->delete();
    }
};