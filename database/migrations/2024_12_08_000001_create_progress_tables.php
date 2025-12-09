<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_progress_items', function (Blueprint $table) {
            $table->id();
            $table->foreignId('pekerjaan_id')->constrained('tbl_pekerjaan')->onDelete('cascade');
            $table->string('nama_item');
            $table->string('rincian_item')->nullable();
            $table->string('satuan');
            $table->decimal('harga_satuan', 15, 2)->default(0);
            $table->decimal('bobot', 5, 2)->default(0);
            $table->decimal('target_volume', 15, 2)->default(0);
            $table->timestamps();
        });

        Schema::create('tbl_weekly_progress', function (Blueprint $table) {
            $table->id();
            $table->foreignId('progress_item_id')->constrained('tbl_progress_items')->onDelete('cascade');
            $table->integer('minggu');
            $table->decimal('rencana', 15, 2)->default(0);
            $table->decimal('realisasi', 15, 2)->nullable();
            $table->timestamps();

            $table->unique(['progress_item_id', 'minggu']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_weekly_progress');
        Schema::dropIfExists('tbl_progress_items');
    }
};
