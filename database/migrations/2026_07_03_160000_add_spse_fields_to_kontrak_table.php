<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_kontrak', function (Blueprint $table) {
            $table->string('spse_sppbj_id', 32)->nullable()->after('spmk');
            $table->string('spse_spk_id', 32)->nullable()->after('spse_sppbj_id');
            $table->string('spse_rekanan_id', 32)->nullable()->after('spse_spk_id');
            $table->timestamp('spse_pushed_at')->nullable()->after('spse_rekanan_id');
            $table->json('spse_push_log')->nullable()->after('spse_pushed_at');
        });
    }

    public function down(): void
    {
        Schema::table('tbl_kontrak', function (Blueprint $table) {
            $table->dropColumn([
                'spse_sppbj_id',
                'spse_spk_id',
                'spse_rekanan_id',
                'spse_pushed_at',
                'spse_push_log',
            ]);
        });
    }
};