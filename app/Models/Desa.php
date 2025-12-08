<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class Desa extends Model
{
    protected $table = 'tbl_desa';
    
    protected $fillable = [
        'n_desa',
        'luas',
        'jumlah_penduduk',
        'kecamatan_id'
    ];

    protected $casts = [
        'luas' => 'double',
        'jumlah_penduduk' => 'integer'
    ];

    /**
     * Relasi Many-to-One dengan Kecamatan
     */
    public function kecamatan(): BelongsTo
    {
        return $this->belongsTo(Kecamatan::class, 'kecamatan_id');
    }
}
