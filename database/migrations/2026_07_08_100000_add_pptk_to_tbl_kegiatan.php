<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_kegiatan', function (Blueprint $table) {
            $table->string('nama_pptk')->nullable()->after('kode_rekening');
            $table->string('nip_pptk')->nullable()->after('nama_pptk');
        });
    }

    public function down(): void
    {
        Schema::table('tbl_kegiatan', function (Blueprint $table) {
            $table->dropColumn(['nama_pptk', 'nip_pptk']);
        });
    }
};