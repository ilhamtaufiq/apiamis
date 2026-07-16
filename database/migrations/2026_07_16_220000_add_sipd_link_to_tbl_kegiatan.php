<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('tbl_kegiatan', function (Blueprint $table) {
            if (! Schema::hasColumn('tbl_kegiatan', 'sipd_id_sub_bl')) {
                $table->unsignedBigInteger('sipd_id_sub_bl')->nullable()->after('nip_pptk');
                $table->index('sipd_id_sub_bl');
            }
            if (! Schema::hasColumn('tbl_kegiatan', 'kode_sub_giat')) {
                $table->string('kode_sub_giat', 64)->nullable()->after('sipd_id_sub_bl');
                $table->index('kode_sub_giat');
            }
        });
    }

    public function down(): void
    {
        Schema::table('tbl_kegiatan', function (Blueprint $table) {
            if (Schema::hasColumn('tbl_kegiatan', 'kode_sub_giat')) {
                $table->dropIndex(['kode_sub_giat']);
                $table->dropColumn('kode_sub_giat');
            }
            if (Schema::hasColumn('tbl_kegiatan', 'sipd_id_sub_bl')) {
                $table->dropIndex(['sipd_id_sub_bl']);
                $table->dropColumn('sipd_id_sub_bl');
            }
        });
    }
};
