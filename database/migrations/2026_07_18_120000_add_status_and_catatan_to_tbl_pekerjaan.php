<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_pekerjaan', function (Blueprint $table) {
            if (! Schema::hasColumn('tbl_pekerjaan', 'status')) {
                $table->string('status', 32)->default('active')->after('is_konsultan');
            }
            if (! Schema::hasColumn('tbl_pekerjaan', 'catatan')) {
                $table->text('catatan')->nullable()->after('status');
            }
        });
    }

    public function down(): void
    {
        Schema::table('tbl_pekerjaan', function (Blueprint $table) {
            if (Schema::hasColumn('tbl_pekerjaan', 'catatan')) {
                $table->dropColumn('catatan');
            }
            if (Schema::hasColumn('tbl_pekerjaan', 'status')) {
                $table->dropColumn('status');
            }
        });
    }
};
