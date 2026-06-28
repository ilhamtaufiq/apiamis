<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class BeritaAcara extends Model
{
    protected $table = 'tbl_berita_acara';

    protected $fillable = [
        'pekerjaan_id',
        'data',
    ];

    protected $casts = [
        'pekerjaan_id' => 'integer',
        'data' => 'array',
    ];

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }
}