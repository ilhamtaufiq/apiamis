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
        $columns = [
            'sumber_dana',
            'program',
            'tarif_dasar_hukum',
            'iuran_nominal',
            'biaya_operasional',
            'biaya_pembangunan'
        ];

        Schema::table('tbl_unit_spam', function (Blueprint $table) use ($columns) {
            foreach ($columns as $column) {
                if (Schema::hasColumn('tbl_unit_spam', $column)) {
                    $table->dropColumn($column);
                }
            }
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('tbl_unit_spam', function (Blueprint $table) {
            $table->string('sumber_dana')->nullable();
            $table->string('program')->nullable();
            $table->string('tarif_dasar_hukum')->nullable();
            $table->string('iuran_nominal')->nullable();
            $table->string('biaya_operasional')->nullable();
            $table->string('biaya_pembangunan')->nullable();
        });
    }
};
