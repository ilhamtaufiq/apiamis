<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class SpamTerbangunRaw extends Model
{
    protected $table = 'tbl_spam_terbangun_raw';

    protected $fillable = [
        'kecamatan',
        'jenis_wilayah',
        'desa_kelurahan',
        'nama_pengelola',
        'sumber_air_baku',
        'sistem_aliran',
        'debit_sumber_l_det',
        'debit_diambil_l_det',
        'penduduk_terlayani',
        'jumlah_penduduk',
        'hu_ku_unit',
        'sr_unit',
        'tanpa_meteran_air_unit',
        'sumber_dana_raw',
        'asal_proyek',
        'nilai_dak_apbn_rp',
        'nilai_apbd_rp',
        'nilai_banprov_rp',
        'tahun_pembangunan_raw',
        'tahun_pembangunan_awal',
        'tahun_pembangunan_akhir',
        'kondisi_raw',
        'kondisi_normalized',
        'tanggal_terakhir_laporan',
        'keterangan',
        'raw_payload',
        'source_file',
        'source_sheet',
        'source_row',
    ];

    protected $casts = [
        'debit_sumber_l_det' => 'decimal:2',
        'debit_diambil_l_det' => 'decimal:2',
        'penduduk_terlayani' => 'integer',
        'jumlah_penduduk' => 'integer',
        'hu_ku_unit' => 'integer',
        'sr_unit' => 'integer',
        'tanpa_meteran_air_unit' => 'integer',
        'nilai_dak_apbn_rp' => 'decimal:2',
        'nilai_apbd_rp' => 'decimal:2',
        'nilai_banprov_rp' => 'decimal:2',
        'tahun_pembangunan_awal' => 'integer',
        'tahun_pembangunan_akhir' => 'integer',
        'tanggal_terakhir_laporan' => 'date',
        'raw_payload' => 'array',
    ];
}
