<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('pekerjaan_progress_estimasi', function (Blueprint $table) {
            $table->id();
            $table->foreignId('pekerjaan_id')->constrained('tbl_pekerjaan')->cascadeOnDelete();
            $table->unsignedSmallInteger('tahun_anggaran');

            $table->date('fisik_rencana_tanggal')->nullable();
            $table->decimal('fisik_rencana_persen', 5, 2)->nullable();
            $table->date('fisik_realisasi_tanggal')->nullable();
            $table->decimal('fisik_realisasi_persen', 5, 2)->nullable();

            $table->date('keuangan_rencana_tanggal')->nullable();
            $table->decimal('keuangan_rencana_persen', 5, 2)->nullable();
            $table->date('keuangan_realisasi_tanggal')->nullable();
            $table->decimal('keuangan_realisasi_persen', 5, 2)->nullable();

            $table->timestamps();

            $table->unique(['pekerjaan_id', 'tahun_anggaran']);
            $table->index('tahun_anggaran');
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('pekerjaan_progress_estimasi');
    }
};