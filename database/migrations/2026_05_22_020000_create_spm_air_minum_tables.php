<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('spam_wilayah_matches', function (Blueprint $table) {
            $table->id();
            $table->string('source_type', 50);
            $table->unsignedBigInteger('source_id');
            $table->string('kecamatan_raw')->nullable();
            $table->string('desa_raw')->nullable();
            $table->foreignId('kecamatan_id')->nullable()->constrained('tbl_kecamatan')->nullOnDelete();
            $table->foreignId('desa_id')->nullable()->constrained('tbl_desa')->nullOnDelete();
            $table->string('match_status', 30)->index();
            $table->unsignedTinyInteger('match_score')->default(0);
            $table->text('notes')->nullable();
            $table->timestamps();

            $table->unique(['source_type', 'source_id']);
            $table->index(['desa_id', 'source_type']);
        });

        Schema::create('spm_air_minum', function (Blueprint $table) {
            $table->id();
            $table->foreignId('kecamatan_id')->constrained('tbl_kecamatan')->cascadeOnDelete();
            $table->foreignId('desa_id')->constrained('tbl_desa')->cascadeOnDelete();
            $table->integer('target_total_jiwa')->nullable();
            $table->integer('jp_jiwa_terlayani')->default(0);
            $table->integer('bjp_jiwa_terlayani')->default(0);
            $table->integer('total_jiwa_terlayani')->default(0);
            $table->integer('belum_terlayani')->nullable();
            $table->decimal('persentase_layanan', 6, 2)->nullable();
            $table->string('status_spm', 30)->index();
            $table->smallInteger('tahun_data')->nullable()->index();
            $table->timestamp('last_consolidated_at')->nullable();
            $table->timestamps();

            $table->unique('desa_id');
            $table->index(['kecamatan_id', 'status_spm']);
        });

        Schema::create('spm_air_minum_sources', function (Blueprint $table) {
            $table->id();
            $table->foreignId('spm_air_minum_id')->constrained('spm_air_minum')->cascadeOnDelete();
            $table->string('source_type', 50);
            $table->unsignedBigInteger('source_id');
            $table->string('jenis_jaringan', 10)->nullable();
            $table->integer('sr_unit')->nullable();
            $table->integer('kk_terlayani')->nullable();
            $table->integer('jiwa_terlayani')->nullable();
            $table->string('kondisi')->nullable();
            $table->string('nama_pengelola')->nullable();
            $table->string('tahun_pembangunan_raw')->nullable();
            $table->timestamps();

            $table->index(['source_type', 'source_id']);
        });
    }

    public function down(): void
    {
        Schema::dropIfExists('spm_air_minum_sources');
        Schema::dropIfExists('spm_air_minum');
        Schema::dropIfExists('spam_wilayah_matches');
    }
};
