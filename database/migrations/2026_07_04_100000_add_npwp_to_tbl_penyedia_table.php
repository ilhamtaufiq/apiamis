<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        if (! Schema::hasColumn('tbl_penyedia', 'npwp')) {
            Schema::table('tbl_penyedia', function (Blueprint $table) {
                $table->string('npwp', 32)->nullable()->after('alamat');
            });
        }
    }

    public function down(): void
    {
        if (Schema::hasColumn('tbl_penyedia', 'npwp')) {
            Schema::table('tbl_penyedia', function (Blueprint $table) {
                $table->dropColumn('npwp');
            });
        }
    }
};