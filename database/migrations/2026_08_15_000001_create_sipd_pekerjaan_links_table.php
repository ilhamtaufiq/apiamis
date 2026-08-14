<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    /**
     * Manual link SIPD rincian → pekerjaan (Status Arumanis).
     */
    public function up(): void
    {
        Schema::create('tbl_sipd_pekerjaan_links', function (Blueprint $table) {
            $table->id();
            $table->unsignedBigInteger('id_sub_bl');
            $table->unsignedBigInteger('id_rinci_sub_bl');
            $table->unsignedBigInteger('pekerjaan_id');
            $table->timestamps();
            $table->unique(['id_sub_bl', 'id_rinci_sub_bl']);
            $table->index('pekerjaan_id');
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_sipd_pekerjaan_links');
    }
};
