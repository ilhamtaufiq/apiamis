<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_spam_terbangun_raw', function (Blueprint $table) {
            $table->id();
            $table->string('kecamatan')->nullable()->index();
            $table->string('jenis_wilayah', 30)->nullable();
            $table->string('desa_kelurahan')->nullable()->index();
            $table->string('nama_pengelola')->nullable();
            $table->string('sumber_air_baku')->nullable();
            $table->string('sistem_aliran')->nullable();
            $table->decimal('debit_sumber_l_det', 12, 2)->nullable();
            $table->decimal('debit_diambil_l_det', 12, 2)->nullable();
            $table->integer('penduduk_terlayani')->nullable();
            $table->integer('jumlah_penduduk')->nullable();
            $table->integer('hu_ku_unit')->nullable();
            $table->integer('sr_unit')->nullable();
            $table->integer('tanpa_meteran_air_unit')->nullable();
            $table->string('sumber_dana_raw')->nullable()->index();
            $table->string('asal_proyek')->nullable();
            $table->decimal('nilai_dak_apbn_rp', 18, 2)->nullable();
            $table->decimal('nilai_apbd_rp', 18, 2)->nullable();
            $table->decimal('nilai_banprov_rp', 18, 2)->nullable();
            $table->string('tahun_pembangunan_raw', 50)->nullable()->index();
            $table->smallInteger('tahun_pembangunan_awal')->nullable()->index();
            $table->smallInteger('tahun_pembangunan_akhir')->nullable();
            $table->string('kondisi_raw', 100)->nullable();
            $table->string('kondisi_normalized', 50)->nullable()->index();
            $table->date('tanggal_terakhir_laporan')->nullable();
            $table->text('keterangan')->nullable();
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
        Schema::dropIfExists('tbl_spam_terbangun_raw');
    }
};
