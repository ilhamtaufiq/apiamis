<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('puspen_progress_fisik_output', function (Blueprint $table) {
            $table->id();
            $table->foreignId('kontrak_id')->constrained('tbl_kontrak')->cascadeOnDelete();
            $table->foreignId('output_id')->constrained('tbl_output')->cascadeOnDelete();
            $table->unsignedSmallInteger('tahun_anggaran');
            $table->decimal('realisasi', 14, 2)->nullable();
            $table->timestamps();

            $table->unique(['kontrak_id', 'output_id', 'tahun_anggaran'], 'puspen_pf_output_unique');
            $table->index('tahun_anggaran');
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('puspen_progress_fisik_output');
    }
};