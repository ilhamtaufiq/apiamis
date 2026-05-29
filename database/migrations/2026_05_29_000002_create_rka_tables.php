<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_rka_documents', function (Blueprint $table) {
            $table->id();
            $table->string('jenis', 20);
            $table->string('nama_file');
            $table->string('path_file');
            $table->string('path_text')->nullable();
            $table->string('nomor_dokumen')->nullable();
            $table->string('tahun_anggaran', 10)->nullable();
            $table->text('program')->nullable();
            $table->text('kegiatan')->nullable();
            $table->text('sub_kegiatan')->nullable();
            $table->json('sumber_pendanaan')->nullable();
            $table->decimal('total_sebelum', 18, 2)->nullable();
            $table->decimal('total_setelah', 18, 2)->nullable();
            $table->decimal('total_selisih', 18, 2)->nullable();
            $table->foreignId('imported_by')->nullable()->constrained('users')->nullOnDelete();
            $table->timestamps();
        });

        Schema::create('tbl_rka_items', function (Blueprint $table) {
            $table->id();
            $table->foreignId('rka_document_id')->constrained('tbl_rka_documents')->cascadeOnDelete();
            $table->string('kode_rekening')->nullable();
            $table->string('tipe', 30)->default('baris');
            $table->text('uraian');
            $table->string('sumber_dana')->nullable();
            $table->string('koefisien')->nullable();
            $table->string('satuan')->nullable();
            $table->decimal('harga', 18, 2)->nullable();
            $table->decimal('jumlah', 18, 2)->nullable();
            $table->decimal('jumlah_sebelum', 18, 2)->nullable();
            $table->decimal('jumlah_setelah', 18, 2)->nullable();
            $table->decimal('selisih', 18, 2)->nullable();
            $table->text('raw_line')->nullable();
            $table->unsignedInteger('sort_order')->default(0);
            $table->timestamps();
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_rka_items');
        Schema::dropIfExists('tbl_rka_documents');
    }
};
