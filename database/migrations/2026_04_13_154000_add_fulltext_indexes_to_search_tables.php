<?php

use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Facades\DB;

return new class extends Migration
{
    /**
     * Run the migrations.
     */
    public function up(): void
    {
        // tbl_pekerjaan
        if (! $this->hasIndex('tbl_pekerjaan', 'ft_pekerjaan_search')) {
            Schema::table('tbl_pekerjaan', function (Blueprint $table) {
                $table->fullText(['nama_paket', 'kode_rekening'], 'ft_pekerjaan_search');
            });
        }

        // tbl_kontrak
        if (! $this->hasIndex('tbl_kontrak', 'ft_kontrak_search')) {
            Schema::table('tbl_kontrak', function (Blueprint $table) {
                $table->fullText(['spk', 'spmk', 'kode_paket'], 'ft_kontrak_search');
            });
        }

        // tbl_penyedia
        if (! $this->hasIndex('tbl_penyedia', 'ft_penyedia_search')) {
            Schema::table('tbl_penyedia', function (Blueprint $table) {
                $table->fullText(['nama', 'direktur'], 'ft_penyedia_search');
            });
        }

        // tbl_kegiatan
        if (! $this->hasIndex('tbl_kegiatan', 'ft_kegiatan_search')) {
            Schema::table('tbl_kegiatan', function (Blueprint $table) {
                $table->fullText(['nama_kegiatan', 'nama_sub_kegiatan', 'nama_program'], 'ft_kegiatan_search');
            });
        }

        // tbl_desa
        if (! $this->hasIndex('tbl_desa', 'ft_desa_search')) {
            Schema::table('tbl_desa', function (Blueprint $table) {
                $table->fullText('n_desa', 'ft_desa_search');
            });
        }

        // tbl_kecamatan
        if (! $this->hasIndex('tbl_kecamatan', 'ft_kecamatan_search')) {
            Schema::table('tbl_kecamatan', function (Blueprint $table) {
                $table->fullText('n_kec', 'ft_kecamatan_search');
            });
        }

        // tbl_penerima
        if (! $this->hasIndex('tbl_penerima', 'ft_penerima_search')) {
            Schema::table('tbl_penerima', function (Blueprint $table) {
                $table->fullText(['nama', 'nik', 'alamat'], 'ft_penerima_search');
            });
        }

        // tbl_output
        if (! $this->hasIndex('tbl_output', 'ft_output_search')) {
            Schema::table('tbl_output', function (Blueprint $table) {
                $table->fullText(['komponen', 'satuan'], 'ft_output_search');
            });
        }
    }

    private function hasIndex($table, $index)
    {
        $indexes = DB::select("SHOW INDEX FROM {$table} WHERE Key_name = ?", [$index]);
        return count($indexes) > 0;
    }

    /**
     * Reverse the migrations.
     */
    public function down(): void
    {
        Schema::table('tbl_pekerjaan', function (Blueprint $table) {
            $table->dropFullText('ft_pekerjaan_search');
        });

        Schema::table('tbl_kontrak', function (Blueprint $table) {
            $table->dropFullText('ft_kontrak_search');
        });

        Schema::table('tbl_penyedia', function (Blueprint $table) {
            $table->dropFullText('ft_penyedia_search');
        });

        Schema::table('tbl_kegiatan', function (Blueprint $table) {
            $table->dropFullText('ft_kegiatan_search');
        });

        Schema::table('tbl_desa', function (Blueprint $table) {
            $table->dropFullText('ft_desa_search');
        });

        Schema::table('tbl_kecamatan', function (Blueprint $table) {
            $table->dropFullText('ft_kecamatan_search');
        });

        Schema::table('tbl_penerima', function (Blueprint $table) {
            $table->dropFullText('ft_penerima_search');
        });

        Schema::table('tbl_output', function (Blueprint $table) {
            $table->dropFullText('ft_output_search');
        });
    }
};
