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
        // tbl_kegiatan
        Schema::table('tbl_kegiatan', function (Blueprint $table) {
            $table->index('tahun_anggaran');
        });

        // tbl_pekerjaan - most foreign keys are already indexed if constrained() was used, 
        // but let's ensure composite indexes for common filters
        Schema::table('tbl_pekerjaan', function (Blueprint $table) {
            $table->index(['kegiatan_id', 'kecamatan_id']);
        });

        // tbl_document_registers
        Schema::table('tbl_document_registers', function (Blueprint $table) {
            $table->index('year');
        });

        // tbl_penerima
        Schema::table('tbl_penerima', function (Blueprint $table) {
            $table->index('is_komunal');
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('tbl_kegiatan', function (Blueprint $table) {
            $table->dropIndex(['tahun_anggaran']);
        });

        Schema::table('tbl_pekerjaan', function (Blueprint $table) {
            $table->dropIndex(['kegiatan_id', 'kecamatan_id']);
        });

        Schema::table('tbl_document_registers', function (Blueprint $table) {
            $table->dropIndex(['year']);
        });

        Schema::table('tbl_penerima', function (Blueprint $table) {
            $table->dropIndex(['is_komunal']);
        });
    }
};
