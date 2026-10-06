<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Schema;

/**
 * Pisahkan data manual dari data hasil integrasi paket pekerjaan:
 * - tbl_spam_achievements.sumber: rekam manual & integrasi pada tahun yang sama
 *   tidak lagi saling menimpa (unique unit+tahun+sumber).
 * - tbl_spam_budgets.pekerjaan_id: anggaran integrasi dikunci per paket, bukan
 *   per nama paket (dua paket bernama sama tidak saling menimpa).
 * - tbl_spm_sanitasi.*_dari_integrasi: menandai pemanfaat/pembiayaan yang diisi
 *   dari paket tertaut, agar bisa diperbarui / direset tanpa menimpa isian manual.
 */
return new class extends Migration
{
    private const INTEGRASI_CATATAN = 'Akumulasi dari paket pekerjaan tertaut';

    public function up(): void
    {
        if (! Schema::hasColumn('tbl_spam_achievements', 'sumber')) {
            Schema::table('tbl_spam_achievements', function (Blueprint $table) {
                $table->string('sumber', 20)->default('manual')->after('tahun');
            });

            DB::table('tbl_spam_achievements')
                ->where('catatan', self::INTEGRASI_CATATAN)
                ->update(['sumber' => 'integrasi']);

            // Index baru dibuat dulu: FK unit_spam_id butuh index berawalan kolom tsb.
            Schema::table('tbl_spam_achievements', function (Blueprint $table) {
                $table->unique(['unit_spam_id', 'tahun', 'sumber'], 'spam_unit_tahun_sumber_unique');
            });
            Schema::table('tbl_spam_achievements', function (Blueprint $table) {
                $table->dropUnique('spam_unit_tahun_unique');
            });
        }

        if (! Schema::hasColumn('tbl_spam_budgets', 'pekerjaan_id')) {
            Schema::table('tbl_spam_budgets', function (Blueprint $table) {
                $table->unsignedBigInteger('pekerjaan_id')->nullable()->after('unit_spam_id');
                $table->index(['unit_spam_id', 'pekerjaan_id'], 'spam_budget_unit_pekerjaan_idx');
            });
        }

        Schema::table('tbl_spm_sanitasi', function (Blueprint $table) {
            if (! Schema::hasColumn('tbl_spm_sanitasi', 'pemanfaat_dari_integrasi')) {
                $table->boolean('pemanfaat_dari_integrasi')->default(false);
            }
            if (! Schema::hasColumn('tbl_spm_sanitasi', 'pembiayaan_dari_integrasi')) {
                $table->boolean('pembiayaan_dari_integrasi')->default(false);
            }
        });

        // Perilaku lama: pembiayaan_total selalu ditimpa dari paket tertaut.
        // Pemanfaat lama dibiarkan manual agar data SPM yang sudah ada tidak berubah.
        DB::table('tbl_spm_sanitasi')
            ->whereIn('id', DB::table('tbl_spm_sanitasi_pekerjaan')->select('spm_sanitasi_id'))
            ->update(['pembiayaan_dari_integrasi' => true]);
    }

    public function down(): void
    {
        Schema::table('tbl_spm_sanitasi', function (Blueprint $table) {
            $table->dropColumn(['pemanfaat_dari_integrasi', 'pembiayaan_dari_integrasi']);
        });

        // FK unit_spam_id bisa memakai index komposit ini — sediakan index pengganti dulu.
        Schema::table('tbl_spam_budgets', function (Blueprint $table) {
            $table->index('unit_spam_id', 'spam_budget_unit_idx');
        });
        Schema::table('tbl_spam_budgets', function (Blueprint $table) {
            $table->dropIndex('spam_budget_unit_pekerjaan_idx');
            $table->dropColumn('pekerjaan_id');
        });

        // Gabungkan kembali ke satu baris per unit+tahun (manual menang).
        // ID dikumpulkan dulu: MySQL menolak DELETE dengan subquery ke tabel yang sama.
        $duplicateIds = DB::table('tbl_spam_achievements as i')
            ->join('tbl_spam_achievements as m', function ($join) {
                $join->on('m.unit_spam_id', '=', 'i.unit_spam_id')
                    ->on('m.tahun', '=', 'i.tahun')
                    ->where('m.sumber', '=', 'manual');
            })
            ->where('i.sumber', 'integrasi')
            ->pluck('i.id');
        DB::table('tbl_spam_achievements')->whereIn('id', $duplicateIds)->delete();

        Schema::table('tbl_spam_achievements', function (Blueprint $table) {
            $table->unique(['unit_spam_id', 'tahun'], 'spam_unit_tahun_unique');
        });
        Schema::table('tbl_spam_achievements', function (Blueprint $table) {
            $table->dropUnique('spam_unit_tahun_sumber_unique');
            $table->dropColumn('sumber');
        });
    }
};
