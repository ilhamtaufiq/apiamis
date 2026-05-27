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
        Schema::create('master_fase_pekerjaans', function (Blueprint $table) {
            $table->id();
            $table->string('jenis_proyek', 50); // e.g. 'sanitasi', 'air_minum'
            $table->string('kode_fase', 30);
            $table->string('nama_fase', 100);
            $table->integer('prioritas'); // 0, 1, 2, ...
            $table->integer('overlap_persen')->default(0); // 0-50
            $table->float('durasi_faktor')->default(1.0);
            $table->json('keywords');
            $table->text('deskripsi')->nullable();
            $table->boolean('is_active')->default(true);
            $table->timestamps();
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('master_fase_pekerjaans');
    }
};
