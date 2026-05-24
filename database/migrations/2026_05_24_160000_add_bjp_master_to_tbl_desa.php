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
        if (!Schema::hasColumn('tbl_desa', 'bjp_master')) {
            Schema::table('tbl_desa', function (Blueprint $table) {
                $table->integer('bjp_master')->default(0)->after('target');
            });
        }
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        if (Schema::hasColumn('tbl_desa', 'bjp_master')) {
            Schema::table('tbl_desa', function (Blueprint $table) {
                $table->dropColumn('bjp_master');
            });
        }
    }
};
