<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class Kecamatan extends Model
{
    use NotifiesAdminsOnChanges, Auditable;
    protected $table = 'tbl_kecamatan';
    
    protected $fillable = [
        'n_kec'
    ];

    /**
     * Relasi One-to-Many dengan Desa
     */
    public function desa(): HasMany
    {
        return $this->hasMany(Desa::class, 'kecamatan_id');
    }

    /** Kecamatan placeholder konsultan (NULL / NULLs) — bukan wilayah resmi. */
    public function scopeRealWilayah(Builder $query): Builder
    {
        return $query
            ->whereNotNull('n_kec')
            ->where('n_kec', '!=', '')
            ->whereRaw('LOWER(TRIM(n_kec)) NOT IN (?, ?)', ['null', 'nulls']);
    }
}
