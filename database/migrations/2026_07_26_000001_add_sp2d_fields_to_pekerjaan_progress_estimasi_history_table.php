<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('pekerjaan_progress_estimasi_history', function (Blueprint $table) {
            $table->string('nomor_sp2d')->nullable()->after('persen');
            $table->date('tanggal_pembuatan')->nullable()->after('nomor_sp2d');
        });
    }

    public function down(): void
    {
        Schema::table('pekerjaan_progress_estimasi_history', function (Blueprint $table) {
            $table->dropColumn(['nomor_sp2d', 'tanggal_pembuatan']);
        });
    }
};
