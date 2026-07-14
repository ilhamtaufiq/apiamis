<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        if (! Schema::hasColumn('tbl_desa', 'jumlah_kk')) {
            Schema::table('tbl_desa', function (Blueprint $table) {
                $table->unsignedInteger('jumlah_kk')->nullable()->after('jumlah_penduduk');
            });
        }
    }

    public function down(): void
    {
        if (Schema::hasColumn('tbl_desa', 'jumlah_kk')) {
            Schema::table('tbl_desa', function (Blueprint $table) {
                $table->dropColumn('jumlah_kk');
            });
        }
    }
};
