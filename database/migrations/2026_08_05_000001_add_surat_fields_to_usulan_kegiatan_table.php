<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_usulan_kegiatan', function (Blueprint $table) {
            $table->date('tanggal_surat_masuk')->after('ringkasan');
            $table->string('nomor_surat_masuk', 100)->after('tanggal_surat_masuk');
            $table->date('tanggal_surat')->after('nomor_surat_masuk');
        });
    }

    public function down(): void
    {
        Schema::table('tbl_usulan_kegiatan', function (Blueprint $table) {
            $table->dropColumn(['tanggal_surat_masuk', 'nomor_surat_masuk', 'tanggal_surat']);
        });
    }
};
