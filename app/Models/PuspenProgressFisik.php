<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class PuspenProgressFisik extends Model
{
    use HasFactory;

    protected $table = 'puspen_progress_fisik';

    protected $fillable = [
        'kontrak_id',
        'tahun_anggaran',
        'rencana',
        'realisasi',
        'pho_completed',
    ];

    protected $casts = [
        'kontrak_id' => 'integer',
        'tahun_anggaran' => 'integer',
        'rencana' => 'float',
        'realisasi' => 'float',
        'pho_completed' => 'boolean',
    ];

    public function kontrak(): BelongsTo
    {
        return $this->belongsTo(Kontrak::class, 'kontrak_id');
    }
}
