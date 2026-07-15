<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * MySQL FLOAT only has ~7 significant digits, so values like
 * 198800000.54 were stored as 198800000. Money fields must use DECIMAL.
 */
return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_kontrak', function (Blueprint $table) {
            $table->decimal('nilai_kontrak', 18, 2)->nullable()->default(null)->change();
        });
    }

    public function down(): void
    {
        Schema::table('tbl_kontrak', function (Blueprint $table) {
            $table->float('nilai_kontrak')->nullable()->default(0)->change();
        });
    }
};
