<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    /**
     * Run the migrations.
     */
    public function up(): void
    {
        Schema::create('app_settings', function (Blueprint $table) {
            $table->id();
            $table->string('key')->unique();
            $table->text('value')->nullable();
            $table->string('type')->default('text'); // text, file
            $table->timestamps();
        });

        // Insert default settings
        DB::table('app_settings')->insert([
            ['key' => 'app_name', 'value' => 'Arumanis', 'type' => 'text', 'created_at' => now(), 'updated_at' => now()],
            ['key' => 'app_description', 'value' => 'Bidang Air Minum dan Sanitasi', 'type' => 'text', 'created_at' => now(), 'updated_at' => now()],
            ['key' => 'tahun_anggaran', 'value' => date('Y'), 'type' => 'text', 'created_at' => now(), 'updated_at' => now()],
        ]);
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('app_settings');
    }
};
