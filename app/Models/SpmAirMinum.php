<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;

class SpmAirMinum extends Model
{
    protected $table = 'spm_air_minum';

    protected $guarded = [];

    protected $casts = [
        'target_total_jiwa' => 'integer',
        'jp_jiwa_terlayani' => 'integer',
        'bjp_jiwa_terlayani' => 'integer',
        'total_jiwa_terlayani' => 'integer',
        'belum_terlayani' => 'integer',
        'persentase_layanan' => 'decimal:2',
        'tahun_data' => 'integer',
        'last_consolidated_at' => 'datetime',
    ];

    public function kecamatan(): BelongsTo
    {
        return $this->belongsTo(Kecamatan::class, 'kecamatan_id');
    }

    public function desa(): BelongsTo
    {
        return $this->belongsTo(Desa::class, 'desa_id');
    }

    public function sources(): HasMany
    {
        return $this->hasMany(SpmAirMinumSource::class, 'spm_air_minum_id');
    }
}
