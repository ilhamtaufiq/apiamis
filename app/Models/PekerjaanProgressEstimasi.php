<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class PekerjaanProgressEstimasi extends Model
{
    use HasFactory;

    protected $table = 'pekerjaan_progress_estimasi';

    protected $fillable = [
        'pekerjaan_id',
        'tahun_anggaran',
        'fisik_rencana_tanggal',
        'fisik_rencana_persen',
        'fisik_realisasi_tanggal',
        'fisik_realisasi_persen',
        'keuangan_rencana_tanggal',
        'keuangan_rencana_persen',
        'keuangan_realisasi_tanggal',
        'keuangan_realisasi_persen',
    ];

    protected $casts = [
        'pekerjaan_id' => 'integer',
        'tahun_anggaran' => 'integer',
        'fisik_rencana_tanggal' => 'date',
        'fisik_rencana_persen' => 'float',
        'fisik_realisasi_tanggal' => 'date',
        'fisik_realisasi_persen' => 'float',
        'keuangan_rencana_tanggal' => 'date',
        'keuangan_rencana_persen' => 'float',
        'keuangan_realisasi_tanggal' => 'date',
        'keuangan_realisasi_persen' => 'float',
    ];

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }
}