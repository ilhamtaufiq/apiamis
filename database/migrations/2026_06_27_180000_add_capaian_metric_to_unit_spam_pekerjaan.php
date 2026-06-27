<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_unit_spam_pekerjaan', function (Blueprint $table) {
            $table->string('capaian_metric', 8)->default('jp')->after('output_id');
        });
    }

    public function down(): void
    {
        Schema::table('tbl_unit_spam_pekerjaan', function (Blueprint $table) {
            $table->dropColumn('capaian_metric');
        });
    }
};