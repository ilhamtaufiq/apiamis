<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('puspen_progress_fisik', function (Blueprint $table) {
            $table->id();
            $table->foreignId('kontrak_id')->constrained('tbl_kontrak')->cascadeOnDelete();
            $table->unsignedSmallInteger('tahun_anggaran');
            $table->decimal('rencana', 5, 2)->nullable();
            $table->decimal('realisasi', 5, 2)->nullable();
            $table->timestamps();

            $table->unique(['kontrak_id', 'tahun_anggaran']);
            $table->index('tahun_anggaran');
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('puspen_progress_fisik');
    }
};
