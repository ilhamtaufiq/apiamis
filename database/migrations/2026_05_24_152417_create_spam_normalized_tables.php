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
        // 1. Add target to tbl_desa
        if (!Schema::hasColumn('tbl_desa', 'target')) {
            Schema::table('tbl_desa', function (Blueprint $table) {
                $table->integer('target')->default(0)->after('jumlah_penduduk');
            });
        }

        // 2. Create tbl_unit_spam
        Schema::create('tbl_unit_spam', function (Blueprint $table) {
            $table->id();
            $table->unsignedBigInteger('desa_id');
            $table->string('name')->nullable();
            $table->boolean('is_simspam')->default(false);
            $table->string('sistem_layanan')->nullable();
            $table->string('sumber_mata_air_kap')->nullable();
            $table->string('sumber_air_tanah_kap')->nullable();
            $table->string('lain_lain_kap')->nullable();
            $table->string('sumber_dana')->nullable();
            $table->string('program')->nullable();
            $table->string('tarif_dasar_hukum')->nullable();
            $table->string('iuran_nominal')->nullable();
            $table->string('biaya_operasional')->nullable();
            $table->timestamps();

            $table->foreign('desa_id')->references('id')->on('tbl_desa')->onDelete('cascade');
        });

        // 3. Create tbl_pengelola
        Schema::create('tbl_pengelola', function (Blueprint $table) {
            $table->id();
            $table->unsignedBigInteger('unit_spam_id')->unique();
            $table->string('pokmas')->nullable();
            $table->string('perdes')->nullable();
            $table->string('kepala')->nullable();
            $table->string('bendahara')->nullable();
            $table->string('sekretaris')->nullable();
            $table->timestamps();

            $table->foreign('unit_spam_id')->references('id')->on('tbl_unit_spam')->onDelete('cascade');
        });

        // 4. Create tbl_unit_checklists
        Schema::create('tbl_unit_checklists', function (Blueprint $table) {
            $table->id();
            $table->unsignedBigInteger('unit_spam_id');
            $table->string('item');
            $table->boolean('is_checked')->default(false);
            $table->timestamps();

            $table->foreign('unit_spam_id')->references('id')->on('tbl_unit_spam')->onDelete('cascade');
        });

        // 5. Create tbl_spam_achievements
        Schema::create('tbl_spam_achievements', function (Blueprint $table) {
            $table->id();
            $table->unsignedBigInteger('unit_spam_id');
            $table->string('tahun');
            $table->integer('jumlah_sr')->default(0);
            $table->integer('jumlah_kk')->default(0);
            $table->integer('jumlah_jiwa')->default(0);
            $table->integer('jumlah_bjp_kk')->default(0);
            $table->integer('jumlah_bjp_jiwa')->default(0);
            $table->text('catatan')->nullable();
            $table->timestamps();

            $table->unique(['unit_spam_id', 'tahun'], 'spam_unit_tahun_unique');
            $table->foreign('unit_spam_id')->references('id')->on('tbl_unit_spam')->onDelete('cascade');
        });
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::dropIfExists('tbl_spam_achievements');
        Schema::dropIfExists('tbl_unit_checklists');
        Schema::dropIfExists('tbl_pengelola');
        Schema::dropIfExists('tbl_unit_spam');

        if (Schema::hasColumn('tbl_desa', 'target')) {
            Schema::table('tbl_desa', function (Blueprint $table) {
                $table->dropColumn('target');
            });
        }
    }
};
