<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_spam_kelembagaan_raw', function (Blueprint $table) {
            $table->id();
            $table->string('jenis_jaringan', 10)->index();
            $table->string('kecamatan')->nullable()->index();
            $table->string('desa_kelurahan')->nullable()->index();
            $table->string('desa_kelurahan_normalized')->nullable();
            $table->string('lokasi_key')->nullable()->index();
            $table->string('tahun_pembangunan_raw', 100)->nullable();
            $table->smallInteger('tahun_pembangunan_awal')->nullable()->index();
            $table->smallInteger('tahun_pembangunan_akhir')->nullable();
            $table->string('sumber_dana_raw')->nullable()->index();
            $table->text('program_pembangunan')->nullable();
            $table->string('nama_pengelola')->nullable()->index();
            $table->string('perdes_pembentukan_pokmas')->nullable();
            $table->string('pengurus_kepala')->nullable();
            $table->string('pengurus_bendahara')->nullable();
            $table->string('pengurus_sekretaris')->nullable();
            $table->decimal('kapasitas_mata_air_l_det', 12, 2)->nullable();
            $table->string('sistem_aliran')->nullable();
            $table->decimal('kapasitas_air_tanah_l_det', 12, 2)->nullable();
            $table->decimal('kapasitas_lain_l_det', 12, 2)->nullable();
            $table->string('dasar_hukum_tarif')->nullable();
            $table->string('besaran_iuran')->nullable();
            $table->decimal('pendapatan_bulanan_rp', 18, 2)->nullable();
            $table->decimal('biaya_operasional_bulanan_rp', 18, 2)->nullable();
            $table->integer('sr_unit')->nullable();
            $table->integer('kk_terlayani')->nullable();
            $table->integer('jiwa_terlayani')->nullable();
            $table->integer('target_layanan')->nullable();
            $table->json('raw_payload')->nullable();
            $table->string('source_file')->nullable();
            $table->string('source_sheet', 100)->nullable();
            $table->unsignedInteger('source_row')->nullable();
            $table->timestamps();

            $table->index(['source_sheet', 'source_row']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_spam_kelembagaan_raw');
    }
};
