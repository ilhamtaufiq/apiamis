<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class BeritaAcara extends Model
{
    protected $table = 'tbl_berita_acara';

    protected $fillable = [
        'pekerjaan_id',
        'data'
    ];

    protected $casts = [
        'data' => 'array'
    ];

    /**
     * Relasi Many-to-One dengan Pekerjaan
     */
    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    /**
     * Get default structure for data
     */
    public static function getDefaultData(): array
    {
        return [
            'ba_lpp' => [],
            'serah_terima_pertama' => [],
            'ba_php' => [],
            'ba_stp' => []
        ];
    }
}
