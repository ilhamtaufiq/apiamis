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
        Schema::table('tbl_draft_pekerjaan', function (Blueprint $table) {
            $table->string('kode_rup')->nullable()->after('penyedia_id');
            $table->string('kode_paket')->nullable()->after('kode_rup');
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('tbl_draft_pekerjaan', function (Blueprint $table) {
            $table->dropColumn(['kode_rup', 'kode_paket']);
        });
    }
};
