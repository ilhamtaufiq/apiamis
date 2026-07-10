<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * Kolom pemantauan kelembagaan POKMAS (format workbook Kab. Cianjur).
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_unit_spam', function (Blueprint $table) {
            if (! Schema::hasColumn('tbl_unit_spam', 'tahun_pembangunan')) {
                $table->string('tahun_pembangunan', 10)->nullable()->after('lain_lain_kap');
            }
            if (! Schema::hasColumn('tbl_unit_spam', 'sumber_dana')) {
                $table->string('sumber_dana')->nullable()->after('tahun_pembangunan');
            }
            if (! Schema::hasColumn('tbl_unit_spam', 'program')) {
                $table->string('program')->nullable()->after('sumber_dana');
            }
            if (! Schema::hasColumn('tbl_unit_spam', 'tarif_dasar_hukum')) {
                $table->string('tarif_dasar_hukum')->nullable()->after('program');
            }
            if (! Schema::hasColumn('tbl_unit_spam', 'iuran_nominal')) {
                $table->string('iuran_nominal')->nullable()->after('tarif_dasar_hukum');
            }
            if (! Schema::hasColumn('tbl_unit_spam', 'pendapatan_bulan')) {
                $table->string('pendapatan_bulan')->nullable()->after('iuran_nominal');
            }
            if (! Schema::hasColumn('tbl_unit_spam', 'biaya_operasional')) {
                $table->string('biaya_operasional')->nullable()->after('pendapatan_bulan');
            }
        });
    }

    public function down(): void
    {
        Schema::table('tbl_unit_spam', function (Blueprint $table) {
            $cols = [
                'tahun_pembangunan',
                'sumber_dana',
                'program',
                'tarif_dasar_hukum',
                'iuran_nominal',
                'pendapatan_bulan',
                'biaya_operasional',
            ];
            foreach ($cols as $col) {
                if (Schema::hasColumn('tbl_unit_spam', $col)) {
                    $table->dropColumn($col);
                }
            }
        });
    }
};
