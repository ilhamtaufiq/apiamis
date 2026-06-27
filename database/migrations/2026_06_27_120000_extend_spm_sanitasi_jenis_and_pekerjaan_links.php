<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        DB::statement("ALTER TABLE tbl_spm_sanitasi MODIFY jenis VARCHAR(30) NOT NULL");

        Schema::create('tbl_spm_sanitasi_pekerjaan', function (Blueprint $table) {
            $table->id();
            $table->foreignId('spm_sanitasi_id')->constrained('tbl_spm_sanitasi')->cascadeOnDelete();
            $table->foreignId('pekerjaan_id')->constrained('tbl_pekerjaan')->cascadeOnDelete();
            $table->foreignId('output_id')->nullable()->constrained('tbl_output')->nullOnDelete();
            $table->timestamps();

            $table->unique(['spm_sanitasi_id', 'pekerjaan_id'], 'spm_sanitasi_pekerjaan_unique');
            $table->index('pekerjaan_id');
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_spm_sanitasi_pekerjaan');
        DB::statement("ALTER TABLE tbl_spm_sanitasi MODIFY jenis ENUM('spaldt','spalds','iplt') NOT NULL");
    }
};