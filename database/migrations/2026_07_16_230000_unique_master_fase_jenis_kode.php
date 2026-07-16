<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('master_fase_pekerjaans', function (Blueprint $table) {
            $table->unique(['jenis_proyek', 'kode_fase'], 'master_fase_jenis_kode_unique');
            $table->index('is_active');
        });
    }

    public function down(): void
    {
        Schema::table('master_fase_pekerjaans', function (Blueprint $table) {
            $table->dropUnique('master_fase_jenis_kode_unique');
            $table->dropIndex(['is_active']);
        });
    }
};
