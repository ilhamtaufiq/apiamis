<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class PekerjaanProgressEstimasiHistory extends Model
{
    use HasFactory;

    protected $table = 'pekerjaan_progress_estimasi_history';

    protected $fillable = [
        'pekerjaan_id',
        'tahun_anggaran',
        'jenis',
        'tipe',
        'tanggal',
        'persen',
        'nilai',
        'nomor_sp2d',
        'tanggal_pembuatan',
        'tanggal_pencairan',
    ];

    protected $casts = [
        'pekerjaan_id' => 'integer',
        'tahun_anggaran' => 'integer',
        'tanggal' => 'date',
        'tanggal_pembuatan' => 'date',
        'tanggal_pencairan' => 'date',
        'persen' => 'float',
        'nilai' => 'float',
    ];

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }
}