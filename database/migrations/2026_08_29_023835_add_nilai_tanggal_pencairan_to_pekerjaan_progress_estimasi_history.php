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
        Schema::table('pekerjaan_progress_estimasi_history', function (Blueprint $table) {
            $table->decimal('nilai', 18, 2)->nullable()->after('persen');
            $table->date('tanggal_pencairan')->nullable()->after('tanggal_pembuatan');
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('pekerjaan_progress_estimasi_history', function (Blueprint $table) {
            $table->dropColumn(['nilai', 'tanggal_pencairan']);
        });
    }
};
