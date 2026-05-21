<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('spm_air_minum_sources', function (Blueprint $table) {
            $table->string('sumber_dana_raw')->nullable()->after('tahun_pembangunan_raw');
            $table->decimal('anggaran_rp', 18, 2)->nullable()->after('sumber_dana_raw');
        });
    }

    public function down(): void
    {
        Schema::table('spm_air_minum_sources', function (Blueprint $table) {
            $table->dropColumn(['sumber_dana_raw', 'anggaran_rp']);
        });
    }
};
