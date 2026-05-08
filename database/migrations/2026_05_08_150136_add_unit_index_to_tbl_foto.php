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
        Schema::table('tbl_foto', function (Blueprint $table) {
            $table->integer('unit_index')->nullable()->after('penerima_id');
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('tbl_foto', function (Blueprint $table) {
            $table->dropColumn('unit_index');
        });
    }
};
