<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class SpamKelembagaanRaw extends Model
{
    protected $table = 'tbl_spam_kelembagaan_raw';

    protected $guarded = [];

    protected $casts = [
        'tahun_pembangunan_awal' => 'integer',
        'tahun_pembangunan_akhir' => 'integer',
        'kapasitas_mata_air_l_det' => 'decimal:2',
        'kapasitas_air_tanah_l_det' => 'decimal:2',
        'kapasitas_lain_l_det' => 'decimal:2',
        'pendapatan_bulanan_rp' => 'decimal:2',
        'biaya_operasional_bulanan_rp' => 'decimal:2',
        'sr_unit' => 'integer',
        'kk_terlayani' => 'integer',
        'jiwa_terlayani' => 'integer',
        'target_layanan' => 'integer',
        'raw_payload' => 'array',
    ];
}
