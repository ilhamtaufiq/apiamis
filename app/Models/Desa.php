<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class Desa extends Model
{
    use NotifiesAdminsOnChanges, Auditable;
    protected $table = 'tbl_desa';
    
    protected $fillable = [
        'n_desa',
        'luas',
        'jumlah_penduduk',
        'jumlah_kk',
        'target',
        'bjp_master',
        'kecamatan_id'
    ];

    protected $casts = [
        'luas' => 'double',
        'jumlah_penduduk' => 'integer',
        'jumlah_kk' => 'integer',
        'target' => 'integer',
        'bjp_master' => 'integer'
    ];

    /**
     * Relasi Many-to-One dengan Kecamatan
     */
    public function kecamatan(): BelongsTo
    {
        return $this->belongsTo(Kecamatan::class, 'kecamatan_id');
    }

    public function spmSanitasi(): HasMany
    {
        return $this->hasMany(SpmSanitasi::class, 'desa_id');
    }

    /** Desa/kecamatan placeholder konsultan (NULL / NULLs) — bukan wilayah resmi. */
    public function scopeRealWilayah(Builder $query): Builder
    {
        return $query
            ->whereNotNull('n_desa')
            ->where('n_desa', '!=', '')
            ->whereRaw('LOWER(TRIM(n_desa)) NOT IN (?, ?)', ['null', 'nulls'])
            ->whereHas('kecamatan', fn (Builder $kecQuery) => $kecQuery->realWilayah());
    }
}
