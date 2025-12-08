<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

class Foto extends Model implements HasMedia
{
    use InteractsWithMedia;

    protected $table = 'tbl_foto';

    protected $fillable = [
        'pekerjaan_id',
        'komponen_id',
        'penerima_id',
        'keterangan',
        'koordinat',
        'validasi_koordinat',
        'validasi_koordinat_message'
    ];

    protected $casts = [
        'pekerjaan_id' => 'integer',
        'komponen_id' => 'integer',
        'penerima_id' => 'integer',
        'validasi_koordinat' => 'boolean',
    ];

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    public function penerima(): BelongsTo
    {
        return $this->belongsTo(Penerima::class, 'penerima_id');
    }

    public function komponen(): BelongsTo
    {
        return $this->belongsTo(Output::class, 'komponen_id');
    }

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('foto/pekerjaan');
    }
}
