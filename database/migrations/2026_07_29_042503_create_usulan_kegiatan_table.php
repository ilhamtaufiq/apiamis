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
        Schema::create('tbl_usulan_kegiatan', function (Blueprint $table) {
            $table->id();
            $table->foreignId('user_id')->constrained('users')->onDelete('cascade');
            $table->enum('sub_bidang', ['air minum', 'sanitasi']);
            $table->string('nama_pengusul');
            $table->foreignId('kecamatan_id')->constrained('tbl_kecamatan');
            $table->foreignId('desa_id')->constrained('tbl_desa');
            $table->string('perihal');
            $table->text('ringkasan');
            $table->timestamps();
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('tbl_usulan_kegiatan');
    }
};
