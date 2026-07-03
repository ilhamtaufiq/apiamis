<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class PuspenProgressFisikOutput extends Model
{
    use HasFactory;

    protected $table = 'puspen_progress_fisik_output';

    protected $fillable = [
        'kontrak_id',
        'output_id',
        'tahun_anggaran',
        'realisasi',
    ];

    protected $casts = [
        'kontrak_id' => 'integer',
        'output_id' => 'integer',
        'tahun_anggaran' => 'integer',
        'realisasi' => 'float',
    ];

    public function kontrak(): BelongsTo
    {
        return $this->belongsTo(Kontrak::class, 'kontrak_id');
    }

    public function output(): BelongsTo
    {
        return $this->belongsTo(Output::class, 'output_id');
    }
}