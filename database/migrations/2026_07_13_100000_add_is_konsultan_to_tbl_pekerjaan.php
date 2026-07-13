<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_pekerjaan', function (Blueprint $table) {
            if (! Schema::hasColumn('tbl_pekerjaan', 'is_konsultan')) {
                $table->boolean('is_konsultan')->default(false)->after('pagu');
            }
        });
    }

    public function down(): void
    {
        Schema::table('tbl_pekerjaan', function (Blueprint $table) {
            if (Schema::hasColumn('tbl_pekerjaan', 'is_konsultan')) {
                $table->dropColumn('is_konsultan');
            }
        });
    }
};
