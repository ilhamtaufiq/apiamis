<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('pekerjaan_progress_estimasi_history', function (Blueprint $table) {
            $table->id();
            $table->foreignId('pekerjaan_id')->constrained('tbl_pekerjaan')->cascadeOnDelete();
            $table->unsignedSmallInteger('tahun_anggaran');
            $table->enum('jenis', ['fisik', 'keuangan']);
            $table->enum('tipe', ['rencana', 'realisasi']);
            $table->date('tanggal');
            $table->decimal('persen', 5, 2);
            $table->timestamps();

            $table->index(['pekerjaan_id', 'tahun_anggaran', 'jenis', 'tipe'], 'ppe_history_lookup_idx');
            $table->index(['tanggal']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('pekerjaan_progress_estimasi_history');
    }
};