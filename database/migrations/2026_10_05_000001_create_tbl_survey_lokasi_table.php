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
        Schema::create('tbl_survey_lokasi', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->constrained('users')->cascadeOnDelete();
            $table->enum('jenis', ['spam', 'sumur_bor', 'mck']);
            $table->string('nama_lokasi', 255);
            $table->foreignId('kecamatan_id')->nullable()->constrained('tbl_kecamatan')->nullOnDelete();
            $table->foreignId('desa_id')->nullable()->constrained('tbl_desa')->nullOnDelete();
            $table->text('alamat')->nullable();
            $table->decimal('latitude', 10, 7)->nullable();
            $table->decimal('longitude', 10, 7)->nullable();
            $table->json('detail')->nullable();
            $table->enum('status', ['diajukan', 'diverifikasi', 'ditolak'])->default('diajukan');
            $table->text('catatan_verifikasi')->nullable();
            $table->foreignId('verified_by')->nullable()->constrained('users')->nullOnDelete();
            $table->timestamp('verified_at')->nullable();
            $table->timestamps();
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('tbl_survey_lokasi');
    }
};
