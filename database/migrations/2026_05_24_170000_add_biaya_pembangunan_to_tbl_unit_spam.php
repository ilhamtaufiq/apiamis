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
        Schema::table('tbl_unit_spam', function (Blueprint $table) {
            $table->string('biaya_pembangunan')->nullable()->after('biaya_operasional');
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('tbl_unit_spam', function (Blueprint $table) {
            $table->dropColumn('biaya_pembangunan');
        });
    }
};
