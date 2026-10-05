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
        Schema::create('tbl_survey_tugas', function (Blueprint $table) {
            $table->id();
            $table->foreignId('pekerjaan_id')->nullable()->constrained('tbl_pekerjaan')->nullOnDelete();
            $table->string('judul', 255);
            $table->integer('tahun_anggaran');
            $table->enum('jenis', ['spam', 'sumur_bor', 'mck'])->nullable();
            $table->foreignId('kecamatan_id')->nullable()->constrained('tbl_kecamatan')->nullOnDelete();
            $table->foreignId('desa_id')->nullable()->constrained('tbl_desa')->nullOnDelete();
            $table->text('lokasi_catatan')->nullable();
            $table->foreignId('assignee_id')->nullable()->constrained('users')->nullOnDelete();
            $table->enum('status', ['ditugaskan', 'dikerjakan', 'selesai'])->default('ditugaskan');
            $table->date('batas_waktu')->nullable();
            $table->text('catatan_admin')->nullable();
            $table->foreignId('created_by')->nullable()->constrained('users')->nullOnDelete();
            $table->timestamps();

            $table->index(['assignee_id', 'status']);
            $table->index('tahun_anggaran');
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('tbl_survey_tugas');
    }
};
