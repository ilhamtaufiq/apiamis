<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    public function up(): void
    {
        DB::table('tbl_usulan_kegiatan')
            ->whereNull('tanggal_surat_masuk')
            ->orWhereNull('nomor_surat_masuk')
            ->orWhereNull('tanggal_surat')
            ->update([
                'tanggal_surat_masuk' => now(),
                'nomor_surat_masuk' => '-',
                'tanggal_surat' => now(),
            ]);

        Schema::table('tbl_usulan_kegiatan', function (Blueprint $table) {
            $table->date('tanggal_surat_masuk')->nullable(false)->change();
            $table->string('nomor_surat_masuk', 100)->nullable(false)->change();
            $table->date('tanggal_surat')->nullable(false)->change();
        });
    }

    public function down(): void
    {
        Schema::table('tbl_usulan_kegiatan', function (Blueprint $table) {
            $table->date('tanggal_surat_masuk')->nullable()->change();
            $table->string('nomor_surat_masuk', 100)->nullable()->change();
            $table->date('tanggal_surat')->nullable()->change();
        });
    }
};
