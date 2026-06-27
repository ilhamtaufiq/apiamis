<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('tbl_spm_sanitasi', function (Blueprint $table) {
            $table->id();
            $table->enum('jenis', ['spaldt', 'spalds', 'iplt']);
            $table->foreignId('desa_id')->nullable()->constrained('tbl_desa')->nullOnDelete();
            $table->string('skala_pelayanan')->nullable();
            $table->string('nama_infrastruktur');
            $table->decimal('latitude', 11, 8)->nullable();
            $table->decimal('longitude', 11, 8)->nullable();
            $table->text('alamat_lengkap')->nullable();
            $table->unsignedInteger('jumlah_pemanfaat_kk')->nullable();
            $table->unsignedInteger('jumlah_pemanfaat_jiwa')->nullable();
            $table->unsignedSmallInteger('tahun_konstruksi')->nullable();
            $table->decimal('pembiayaan_apbn', 18, 2)->nullable();
            $table->decimal('pembiayaan_apbd', 18, 2)->nullable();
            $table->decimal('pembiayaan_dak', 18, 2)->nullable();
            $table->decimal('pembiayaan_hibah', 18, 2)->nullable();
            $table->decimal('pembiayaan_csr', 18, 2)->nullable();
            $table->decimal('pembiayaan_lain', 18, 2)->nullable();
            $table->decimal('pembiayaan_total', 18, 2)->nullable();
            $table->string('status_keberfungsian')->nullable();
            $table->string('kualitas_keberfungsian')->nullable();
            $table->string('pengelola')->nullable();
            $table->decimal('kapasitas_desain', 12, 2)->nullable();
            $table->decimal('kapasitas_terpakai', 12, 2)->nullable();
            $table->decimal('kapasitas_tidak_terpakai', 12, 2)->nullable();
            $table->string('jenis_pengolahan')->nullable();
            $table->string('peta_cakupan')->nullable();
            $table->string('status_lahan')->nullable();
            $table->string('luas_lahan_ha')->nullable();
            $table->string('opsi_teknologi')->nullable();
            $table->string('jumlah_stasiun_pompa')->nullable();
            $table->decimal('biaya_operasional', 18, 2)->nullable();
            $table->string('jenis_pengelola')->nullable();
            $table->string('sistem_pengolahan')->nullable();
            $table->unsignedSmallInteger('truk_tinja_unit')->nullable();
            $table->decimal('kapasitas_truk_m3', 10, 2)->nullable();
            $table->unsignedSmallInteger('jumlah_ritasi')->nullable();
            $table->decimal('jarak_maksimal_pelayanan_km', 10, 2)->nullable();
            $table->decimal('alokasi_biaya_operasional', 18, 2)->nullable();
            $table->timestamps();

            $table->index(['jenis', 'desa_id']);
            $table->index('tahun_konstruksi');
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('tbl_spm_sanitasi');
    }
};